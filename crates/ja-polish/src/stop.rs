//! 文書ファイルの関所(Stop)。`ja-polish stop-gate` が標準入力の Stop の入力を読んで呼ぶ。
//!
//! セッションの終わりに、触れた日本語の文書ファイルに、通していない日本語の段落が残っていないかを
//! 見る。LLM は呼ばず、記録を引くだけ。
//!
//! 見るファイル(拡張子 `.md` `.markdown` `.txt`)は 2 つの和集合:
//! - このセッションで `Write` / `Edit` / `MultiEdit` した文書([`crate::snapshot::touched`])
//! - cwd の git の作業ツリーで、origin の既定ブランチとの merge-base から変わった、または
//!   未追跡の文書
//!
//! 各ファイルは、基点([`crate::baseline::baseline_for`])に無い(追加・変更した)日本語の段落のうち、
//! 記録に無いものを数える。1 つでもあれば `{"decision":"block"}` を出し、`ja-polish <ファイル>`
//! に通すよう促す。
//!
//! 無限に止めない: `stop_hook_active`(block の直後の再呼び出し)では黙る。通せなかった段落は
//! `ja-polish` が素通しとして記録するので、直そうとして直せない状態でも止まり続けない。

use crate::{baseline, config, polish, records, snapshot};
use agent_session::{Reentry, StopInput, StopOutcome, StopResult};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 1 回に見るファイル数の上限(ブランチが大きいとき、Stop の持ち時間を使い切らないため)。
const MAX_FILES: usize = 40;

/// 通していない段落を持つ文書 1 つ分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpolished {
    pub path: PathBuf,
    pub count: usize,
    /// 最初の段落の先頭(40 字)。
    pub head: String,
}

/// 見る候補のファイル: 触れた文書と、git の変更。文書の拡張子で絞り、実在するものを、重複なく。
fn candidates(cwd: &Path, session_id: &str) -> Vec<PathBuf> {
    let mut all = snapshot::touched(session_id);
    all.extend(baseline::git_changed_files(cwd));
    let mut seen = std::collections::BTreeSet::new();
    all.into_iter()
        .filter(|p| snapshot::is_doc(p) && p.is_file())
        .filter(|p| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
        .take(MAX_FILES)
        .collect()
}

/// 通していない段落を持つ文書の一覧。
pub fn unpolished(
    cwd: &Path,
    session_id: &str,
    known: &std::collections::HashSet<String>,
) -> Vec<Unpolished> {
    candidates(cwd, session_id)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let base = baseline::baseline_for(&path);
            let pending = polish::pending(&text, base.as_deref(), known);
            let first = pending.first()?;
            Some(Unpolished {
                head: first.text.chars().take(40).collect(),
                count: pending.len(),
                path,
            })
        })
        .collect()
}

fn reason(cwd: &Path, files: &[Unpolished]) -> String {
    let mut s =
        String::from("[ja-polish] 日本語の文書に、書き直しを通していない段落が残っています:\n");
    for f in files {
        let shown = f.path.strip_prefix(cwd).unwrap_or(&f.path);
        s.push_str(&format!(
            "- {} ({} 段落。最初: 「{}」)\n",
            shown.display(),
            f.count,
            f.head
        ));
    }
    s.push_str(
        "`ja-polish <ファイル>` で書き直してください(追加・変更した段落だけが対象で、ファイルはその場で書き直されます)。\
`ja-polish` は単体で呼びます(リダイレクトや `$(…)` を足すと sandbox の中に残ってキーに届きません)。\
出力の差分を読み、意味がずれていたら、直したファイルでなく元の文を直して通し直します。\
`ja-polish` が通せなかった段落は素通しとして記録されるので、この検査は通ります。設計: docs/agents/ja-polish.md",
    );
    s
}

/// block の直後の再呼び出しでは黙る(無限 block 対策)。入口(`StopSpec`)がこの宣言を適用する。
pub const REENTRY: Reentry = Reentry::Quiet;

/// Stop の入力(JSON)を判定する。止めるときは `Block`、通すときは `None`。
pub fn hook(input: &StopInput) -> StopResult {
    Ok(check(input.raw()))
}

fn check(raw: &str) -> Option<StopOutcome> {
    if agent_session::escape::JA_POLISH_GATE
        .active(None, None)
        .is_some()
    {
        return None;
    }
    config::load()?;
    let input: Value = serde_json::from_str(raw).ok()?;
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let session_id = input
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let known = records::records_path()
        .map(|p| records::load(&p))
        .unwrap_or_default();
    let files = unpolished(&cwd, session_id, &known);
    if files.is_empty() {
        return None;
    }
    Some(StopOutcome::block(reason(&cwd, &files)))
}
