//! feedback-target-guard — `gh issue create --label feedback` に `Target:` 行
//! (規範・skill・hook への実在ポインタ)を要求する PreToolUse(Bash) hook
//! (ADR-543「既存手段の前倒し接地と、決定論への昇格導線」段3、
//! ADR-0024 Stage 4a #415)。
//!
//! bash 版 `config/claude/hooks/feedback-target-guard.sh` の移植。コマンド解析と
//! 本文抽出は `guard-core`(`gh::extract_body`)にあり、ここには feedback ラベルの
//! 検出と `Target:` の実在照合だけを置く。
//!
//! 動機: Q2(LLM/散文 → 決定論への昇格)の兆候の 1 つは「同じ規範・skill に
//! 対する feedback Issue の再発」。再発を機械的に集計するには、各 feedback
//! Issue がどの規範・skill・hook を指しているかを本文から決定論で読み取れる
//! 必要がある。自由記述のまま起票させると集計不能になる(閉語彙 > 自由記述
//! +事後 lint、ADR-0035 D1)ため、起票の瞬間に `Target:` を要求する。
//!
//! `Target:` の閉じた語彙(いずれか 1 つ、実在照合まで行う):
//! - `Target: skill/<name>` — `~/.claude/skills/<name>/SKILL.md`
//! - `Target: agents-md/<節見出し>` — `~/.agents/AGENTS.md` または
//!   `~/.claude/CLAUDE.md` に実在する見出し文字列(部分文字列の一致)
//! - `Target: hook/<name>` — `~/.claude/hooks/<name>*` に実在するファイル
//!
//! 過去の feedback Issue(#516 等、この hook の導入前に起票されたもの)は
//! 遡及修正しない(ADR-0007 の grandfathering 前例、ADR-0035 D5 と同型)。
//!
//! `--body-file`(`-F`)の本文も読む(#675): bleep の正準形(ADR-0003)は本文を
//! `--body-file` の絶対パスだけで渡すので、実運用で効くのはこちら。
//!
//! 判定不能は通す(bash 版と同じ): feedback ラベル無し・本文フラグ無し・
//! 本文がコマンド置換のみ・空白のみ・トークン化できない。

use guard_core::command::{first_deny, gh_command_at};
use guard_core::gh::extract_body;
use guard_core::shell::is_posix_space;
use guard_core::Range;
use std::path::{Path, PathBuf};

const FEEDBACK_LABEL: &str = "feedback";

/// bash の `is_target_at`: `gh issue create` のみ。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<()> {
    gh_command_at(tokens, i, &["issue", "create"]).then_some(())
}

/// bash の `has_feedback_label`: `--label` / `-l`(次のトークン)と `--label=…`
/// のどれかにカンマ区切りで `feedback` が含まれるか。値は `read -r -a` で読む
/// ので最初の改行までしか見ない。
pub fn has_feedback_label(tokens: &[String]) -> bool {
    let n = tokens.len();
    let hit = |val: &str| {
        val.split('\n')
            .next()
            .unwrap_or("")
            .split(',')
            .any(|p| p == FEEDBACK_LABEL)
    };
    let mut i = 0;
    while i < n {
        let t = tokens[i].as_str();
        if t == "--label" || t == "-l" {
            if i + 1 < n {
                if hit(&tokens[i + 1]) {
                    return true;
                }
                i += 2;
                continue;
            }
        } else if let Some(v) = t.strip_prefix("--label=") {
            if hit(v) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// bash の `find_target_value`: 最初の `Target: <値>` 行の値(値は空白を含まない
/// 連続した文字列)。行単位 — 改行をまたがない。
pub fn find_target_value(text: &str) -> Option<String> {
    let sp = |c: char| c.is_ascii() && is_posix_space(c as u8);
    text.split('\n').find_map(|line| {
        let rest = line.trim_start_matches(sp).strip_prefix("Target:")?;
        let value: String = rest
            .trim_start_matches(sp)
            .chars()
            .take_while(|&c| !sp(c))
            .collect();
        (!value.is_empty()).then_some(value)
    })
}

/// `Target:` の実在照合に使う `$HOME`(bash の `$HOME` と同じく環境変数)。
#[derive(Debug, Clone)]
pub struct Home(pub PathBuf);

impl Home {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("HOME").map(|h| Home(PathBuf::from(h)))
    }

    /// bash の `validate_target`: `<種別>/<名前>` が実在すれば真。
    pub fn validate_target(&self, value: &str) -> bool {
        let Some((kind, name)) = value.split_once('/') else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        match kind {
            "skill" => self
                .0
                .join(".claude/skills")
                .join(name)
                .join("SKILL.md")
                .is_file(),
            "hook" => hook_exists(&self.0.join(".claude/hooks"), name),
            "agents-md" => [".agents/AGENTS.md", ".claude/CLAUDE.md"]
                .iter()
                .any(|f| file_contains(&self.0.join(f), name)),
            _ => false,
        }
    }
}

/// `compgen -G "$HOME/.claude/hooks/${name}*"` が 1 件以上に一致するか。
/// 最後の `/` までをディレクトリ、残りを接頭辞として、その接頭辞で始まる
/// エントリを探す(`*` は先頭が `.` のエントリに一致しない — 接頭辞が空のとき
/// だけ効く)。
///
/// bash 版は `name` を glob パターンの一部としてそのまま展開するので `*` / `?` /
/// `[…]` が効いていた(`Target: hook/*` で実在しない hook を指せてしまう)。
/// ここでは `name` を字面どおりに扱う(意図的な変更、docs/claude/ の注記参照)。
fn hook_exists(hooks_dir: &Path, name: &str) -> bool {
    let (dir, prefix) = match name.rfind('/') {
        Some(p) => (hooks_dir.join(&name[..=p]), &name[p + 1..]),
        None => (hooks_dir.to_path_buf(), name),
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.filter_map(Result::ok).any(|e| {
        let fname = e.file_name();
        let fname = fname.to_string_lossy();
        if prefix.is_empty() {
            !fname.starts_with('.')
        } else {
            fname.starts_with(prefix)
        }
    })
}

/// `grep -qF -- "$name" file`(読めない・無いなら偽)。
fn file_contains(path: &Path, needle: &str) -> bool {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).contains(needle))
        .unwrap_or(false)
}

fn deny_reason_feedback(why: &str) -> String {
    format!(
        "feedback ラベル付き Issue の起票に問題があります: {why}。

本文に次のいずれかの形式で1行追加してください(実在照合されます):

  Target: skill/<name>          — config/claude/skills/<name>/SKILL.md
  Target: agents-md/<節見出し>   — ~/.agents/AGENTS.md・~/.claude/CLAUDE.md の実在する見出し
  Target: hook/<name>            — config/claude/hooks/<name>* の実在するファイル

(共有 AGENTS.md「ユーザーからのフィードバックは不可視なローカルメモに
閉じ込めない」、ADR-543「既存手段の前倒し接地と、決定論への昇格導線」参照)"
    )
}

/// bash の `decide_tokens`: 1 投稿ぶんのトークン列。feedback ラベル + `Target:`
/// を検査し、deny なら理由文。
pub fn decide_tokens(
    home: Option<&Home>,
    tokens: &[String],
    heredoc_bodies: &str,
) -> Option<String> {
    if !has_feedback_label(tokens) {
        return None; // feedback で無ければ対象外
    }
    // 本文フラグ無し・コマンド置換のみ・空白のみ → 判定不能で通す
    // (feedback は付いているが body 無しは gh 側で別途弾かれる)
    let text = extract_body(tokens, heredoc_bodies)?;
    let Some(target) = find_target_value(&text) else {
        return Some(deny_reason_feedback("Target: 行がありません"));
    };
    if !home.is_some_and(|h| h.validate_target(&target)) {
        return Some(deny_reason_feedback(&format!(
            "Target: {target} が実在しません"
        )));
    }
    None
}

/// bash の `decide`: Bash ツールのコマンド文字列全体。
pub fn decide(home: Option<&Home>, cmd: &str) -> Option<String> {
    first_deny(cmd, is_target_at, |r: &Range<'_, ()>| {
        decide_tokens(home, r.tokens, r.heredoc_bodies)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn labels() {
        assert!(has_feedback_label(&v(&["--label", "bug,feedback"])));
        assert!(has_feedback_label(&v(&["-l", "feedback"])));
        assert!(has_feedback_label(&v(&["--label=x,feedback"])));
        assert!(!has_feedback_label(&v(&["--label", "feedbacks"])));
        assert!(!has_feedback_label(&v(&["--title", "feedback"])));
        assert!(!has_feedback_label(&v(&["--label"])));
        // 値の 2 行目以降は見ない(`read -r -a` は 1 行だけ読む)
        assert!(!has_feedback_label(&v(&["--label", "bug\nfeedback"])));
    }

    #[test]
    fn target_value() {
        assert_eq!(
            find_target_value("a\n  Target:   skill/x y\n").as_deref(),
            Some("skill/x")
        );
        assert_eq!(find_target_value("Target:\nskill/x"), None);
        assert_eq!(find_target_value("not Target: skill/x"), None);
        assert_eq!(
            find_target_value("Target: skill/a\nTarget: skill/b").as_deref(),
            Some("skill/a")
        );
        assert_eq!(
            find_target_value("Target:\tskill/x\r\n").as_deref(),
            Some("skill/x")
        );
    }
}
