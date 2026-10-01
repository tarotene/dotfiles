//! stack-base-guard — セッション内の複数 PR を常時単一チェーンに積むことを
//! 作成時に機械強制する PreToolUse hook(ADR-0027: uncertainty-first
//! stacking、ADR-0024 Stage 4a #415)。
//!
//! 設計と根拠: docs/claude/stack-base-guard.md
//!
//! bash 版 `config/claude/hooks/stack-base-guard.sh` と Codex adapter
//! `config/codex/hooks/stack-base-guard.sh` の移植。bash 版は
//! attribution-guard.sh を `source` して `is_target_at` / `TARGET_KIND` を
//! 上書きしていた — ここでは `guard_core::command::first_deny` に対象判定
//! 関数を渡す形になった。
//!
//! 判定は 2 層:
//!   層(i)  状態レスの祖先一致検査 — HEAD(または編集対象 PR の head)が他の
//!          open PR のコミットを祖先として含むなら、base はその PR の head
//!          branch でなければならない。タグでも抜けられない(物理的必然)。
//!   層(ii) セッション ID 単位の状態 — セッション内で作成した PR の head
//!          branch を記録し、2 本目以降でチェーン外のブランチから PR を
//!          作ろうとした場合は本文 `Independent-PR: <理由>` を要求する。
//!
//! 対象コマンド: コマンド位置の `gh pr create` と `gh pr edit ... --base ...`
//! (`-R/--repo` によるクロスリポジトリ指定にも対応)。判定不能はすべて
//! fail-open(通す) — ADR-0005 の binary-existence gating と同じ縮退方針。

use guard_core::command::{first_deny, gh_command_at};
use guard_core::gh::{command_substitution, has_heredoc, read_body_file};
use guard_core::hook::{jq_r_path, ToolCall};
use guard_core::marker::has_reasoned_tag;
use guard_core::repo::{default_branch_or_gh, owner_repo};
use hook_io::ledger::SessionLedger;
use hook_io::Agent;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 理由を伴って初めて成立する(No-Issue: / No-Attribution: と同型)。
pub const INDEPENDENT_TAG: &str = "Independent-PR:";

/// 対象コマンドの種別(bash の `TARGET_KIND`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Create,
    Edit,
}

/// bash の `is_target_at`(attribution-guard.sh の同名関数を上書きしていたもの)。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<Kind> {
    if gh_command_at(tokens, i, &["pr", "create"]) {
        Some(Kind::Create)
    } else if gh_command_at(tokens, i, &["pr", "edit"]) {
        Some(Kind::Edit)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 1 コマンド範囲のトークンからフラグを抜き出す
// ---------------------------------------------------------------------------

/// `parse_pr_tokens` の結果(bash のグローバル `F_*`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrFlags {
    pub base: String,
    pub head: String,
    /// `gh pr edit` の対象識別子(番号またはブランチ名)。
    pub target: String,
    pub repo: String,
    pub has_base: bool,
    pub has_body: bool,
    /// 本文の値(複数あれば改行で連結、heredoc 本体を含む)。
    pub body_text: String,
}

/// 値を取ることが分かっている他の主要フラグ。値トークンが `target` に
/// 誤って捕捉されないよう明示的に読み飛ばす。
const SKIP_VALUE_FLAGS: &[&str] = &[
    "--title",
    "-t",
    "--add-label",
    "--remove-label",
    "--add-assignee",
    "--remove-assignee",
    "--add-reviewer",
    "--remove-reviewer",
    "--add-project",
    "--remove-project",
    "--milestone",
    "-m",
];

/// bash の `parse_pr_tokens`: 1 つの `gh pr create|edit` 呼び出しぶんの
/// トークン列から、本文・値フラグ・読み飛ばしリスト・位置引数を 1 ループで
/// 読む(guard-core の `scan_value_flags` に収まらないので自前、
/// docs/claude/guard-core.md)。
///
/// 既知の限界: `--title` 等、値を取る未知フラグの値が `target`(edit の対象
/// 識別子)に誤って捕捉されることがある。誤捕捉は「対象 PR が解決できない」
/// 方向に倒れ、判定不能(pass)になるだけなので安全側。
pub fn parse_pr_tokens(tok: &[String], heredoc_bodies: &str) -> PrFlags {
    let n = tok.len();
    let mut f = PrFlags::default();
    let mut body_texts: Vec<String> = Vec::new();
    let has_hd = has_heredoc(tok);

    let mut i = 0;
    while i < n {
        let t = tok[i].as_str();
        let next = (i + 1 < n).then(|| tok[i + 1].clone());
        match t {
            "--base" | "-B" => {
                f.has_base = true;
                if let Some(v) = next {
                    f.base = v;
                    i += 2;
                    continue;
                }
            }
            "--head" | "-H" => {
                if let Some(v) = next {
                    f.head = v;
                    i += 2;
                    continue;
                }
            }
            "--repo" | "-R" => {
                if let Some(v) = next {
                    f.repo = v;
                    i += 2;
                    continue;
                }
            }
            "--body" | "-b" => {
                f.has_body = true;
                if let Some(v) = next {
                    body_texts.push(v);
                    i += 2;
                    continue;
                }
            }
            "--body-file" | "-F" => {
                f.has_body = true;
                if let Some(p) = next {
                    if let Some(s) = read_body_file(&p) {
                        body_texts.push(s);
                    }
                    i += 2;
                    continue;
                }
            }
            _ if SKIP_VALUE_FLAGS.contains(&t) => {
                if i + 1 < n {
                    i += 1;
                }
            }
            _ => {
                if let Some(v) = t.strip_prefix("--base=") {
                    f.has_base = true;
                    f.base = v.to_string();
                } else if let Some(v) = t.strip_prefix("--head=") {
                    f.head = v.to_string();
                } else if let Some(v) = t.strip_prefix("--repo=") {
                    f.repo = v.to_string();
                } else if let Some(v) = t.strip_prefix("--body=") {
                    f.has_body = true;
                    body_texts.push(v.to_string());
                } else if let Some(p) = t.strip_prefix("--body-file=") {
                    f.has_body = true;
                    if let Some(s) = read_body_file(p) {
                        body_texts.push(s);
                    }
                } else if t.starts_with('-') {
                    // 未知フラグ(値なし想定) — そのままスキップ
                } else if f.target.is_empty() && i >= 3 {
                    f.target = t.to_string();
                }
            }
        }
        i += 1;
    }

    if has_hd && !heredoc_bodies.is_empty() {
        body_texts.push(heredoc_bodies.to_string());
    }
    // `$(printf '%s\n' "${body_texts[@]}")` — 末尾の改行は落ちる。
    if !body_texts.is_empty() {
        f.body_text = command_substitution(&body_texts.join("\n"));
    }
    f
}

// ---------------------------------------------------------------------------
// open PR 一覧(bash では jq で読んでいた部分)
// ---------------------------------------------------------------------------

/// open PR 1 件(`[(.number|tostring), .headRefName, .headRefOid,
/// .baseRefName] | @tsv` を `IFS=$'\t' read -r num head oid base` で読んだもの)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrRow {
    pub num: String,
    pub head: String,
    pub oid: String,
    pub base: String,
}

static JSON_NULL: Value = Value::Null;

/// jq の `.key`: null は null、object は値(無ければ null)、それ以外はエラー。
fn jq_index<'a>(v: &'a Value, k: &str) -> Result<&'a Value, ()> {
    match v {
        Value::Null => Ok(&JSON_NULL),
        Value::Object(m) => Ok(m.get(k).unwrap_or(&JSON_NULL)),
        _ => Err(()),
    }
}

/// jq の `.[]`: 配列の要素・object の値。それ以外はエラー。
fn jq_iter(v: &Value) -> Result<Vec<&Value>, ()> {
    match v {
        Value::Array(a) => Ok(a.iter().collect()),
        Value::Object(m) => Ok(m.values().collect()),
        _ => Err(()),
    }
}

/// jq の `tostring`。
fn jq_tostring(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// jq の `@tsv` の 1 フィールド。配列・object はエラー。
fn tsv_field(v: &Value) -> Result<String, ()> {
    Ok(match v {
        Value::Null => String::new(),
        Value::String(s) => s
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r"),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return Err(()),
    })
}

/// 1 要素を TSV 行にしてから bash の `IFS=$'\t' read -r num head oid base`
/// で読み戻す。タブは IFS 空白なので空フィールドは潰れ、後ろのフィールドが
/// 前に詰まる(bash と同じ — GitHub の応答では空フィールドは起きない)。
fn row_of(elem: &Value) -> Result<PrRow, ()> {
    let num = tsv_field(&Value::String(jq_tostring(jq_index(elem, "number")?)))?;
    let fields = [
        num,
        tsv_field(jq_index(elem, "headRefName")?)?,
        tsv_field(jq_index(elem, "headRefOid")?)?,
        tsv_field(jq_index(elem, "baseRefName")?)?,
    ];
    let mut it = fields.into_iter().filter(|s| !s.is_empty());
    Ok(PrRow {
        num: it.next().unwrap_or_default(),
        head: it.next().unwrap_or_default(),
        oid: it.next().unwrap_or_default(),
        base: it.next().unwrap_or_default(),
    })
}

/// `jq -r '.[] | [...] | @tsv'` を `while read` で回したときに読める行。
/// jq は要素ごとに出力するので、途中でエラーになればそこまでの行だけが残る。
pub fn stream_rows(prs_json: &str) -> Vec<PrRow> {
    let Ok(v) = serde_json::from_str::<Value>(prs_json) else {
        return Vec::new();
    };
    let Ok(elems) = jq_iter(&v) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in elems {
        match row_of(e) {
            Ok(r) => out.push(r),
            Err(()) => break,
        }
    }
    out
}

/// `jq -e --arg b "$b" '[.[] | select(.headRefName == $b)] | length > 0'`。
fn has_head(prs_json: &str, b: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(prs_json) else {
        return false;
    };
    let Ok(elems) = jq_iter(&v) else {
        return false;
    };
    let mut hit = false;
    for e in elems {
        match jq_index(e, "headRefName") {
            Ok(Value::String(s)) if s == b => hit = true,
            Ok(_) => {}
            Err(()) => return false,
        }
    }
    hit
}

/// judge_edit の対象 PR 行: `[.[] | select(<pred>)] | .[0] // empty | … @tsv`。
/// jq は配列を作り切ってから先頭を取るので、どこかでエラーなら何も出ない。
fn first_row_where(
    prs_json: &str,
    mut pred: impl FnMut(&Value) -> Result<bool, ()>,
) -> Option<PrRow> {
    let v = serde_json::from_str::<Value>(prs_json).ok()?;
    let elems = jq_iter(&v).ok()?;
    let mut first = None;
    for e in elems {
        if pred(e).ok()? && first.is_none() {
            first = Some(e);
        }
    }
    let r = row_of(first?).ok()?;
    // `read` が 4 フィールドとも空で何も読めない行は bash では `[[ -n $row ]]`
    // で弾かれる(TSV 行は少なくともタブを含むので実際には起きない)。
    Some(r)
}

// ---------------------------------------------------------------------------
// 外部コマンド
// ---------------------------------------------------------------------------

fn git(project: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(project)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn git_ok(project: &Path, args: &[&str]) -> bool {
    git(project)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `$(git -C <project> …)`。失敗は `None`(空出力は `Some("")`)。
fn git_out(project: &Path, args: &[&str]) -> Option<String> {
    let out = git(project).args(args).output().ok()?;
    out.status
        .success()
        .then(|| command_substitution(&String::from_utf8_lossy(&out.stdout)))
}

/// `gh pr list -R <nwo> --state open --limit 100 --json …`。失敗・空は `None`。
fn open_prs(nwo: &str) -> Option<String> {
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "-R",
            nwo,
            "--state",
            "open",
            "--limit",
            "100",
            "--json",
            "number,headRefName,headRefOid,baseRefName",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = command_substitution(&String::from_utf8_lossy(&out.stdout));
    (!s.is_empty()).then_some(s)
}

// ---------------------------------------------------------------------------
// 判定本体
// ---------------------------------------------------------------------------

/// セッション状態(bash の `STACK_BASE_GUARD_DIR` / `SESSION_ID`)。
#[derive(Debug, Clone)]
pub struct Guard {
    /// `${STACK_BASE_GUARD_DIR:-$HOME/.claude/stack-base-guard}`。
    pub dir: PathBuf,
    /// `${SESSION_ID:-unknown}` に当たる値(空なら `unknown` として扱う)。
    pub session_id: String,
}

impl Guard {
    /// 環境変数から(bash の既定値の解決と同じ)。`session_id` は呼び出し側が
    /// 決める(hook では stdin の `.session_id`、`--check` では `$SESSION_ID`)。
    pub fn from_env(session_id: String) -> Self {
        let dir = std::env::var("STACK_BASE_GUARD_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join(".claude/stack-base-guard")
            });
        Guard { dir, session_id }
    }

    /// `$STACK_BASE_GUARD_DIR/skip` があるか `SKIP_STACK_BASE_GUARD=1` なら真。
    pub fn skipped(&self) -> bool {
        self.dir.join("skip").exists()
            || std::env::var("SKIP_STACK_BASE_GUARD").as_deref() == Ok("1")
    }

    fn sid(&self) -> &str {
        if self.session_id.is_empty() {
            "unknown"
        } else {
            &self.session_id
        }
    }

    /// 台帳 `<dir>/state/<sid>.chain`(bash の `state_file()`。保存形式は
    /// bash 版と同じ — 1 行 1 ブランチ、ディレクトリは 0700)。
    fn ledger(&self) -> SessionLedger {
        SessionLedger::new(self.dir.join("state"), "chain")
    }

    /// 楽観的にセッション状態へ追記する(pass した head を記録)。失敗しても
    /// 致命的ではない(次回の判定は state 無しとして扱われるだけ)。
    fn record_chain_head(&self, branch: &str) {
        if branch.is_empty() {
            return;
        }
        let _ = self.ledger().append(self.sid(), branch);
    }

    /// bash の `decide_stack`: コマンド文字列全体から `gh pr create|edit` の
    /// 範囲を切り出して範囲ごとに判定する。deny なら理由。
    pub fn decide_stack(&self, cmd: &str, project: &Path) -> Option<String> {
        first_deny(cmd, is_target_at, |r| {
            let f = parse_pr_tokens(r.tokens, r.heredoc_bodies);
            self.judge_range(r.kind, project, &f)
        })
    }

    fn judge_range(&self, kind: Kind, project: &Path, f: &PrFlags) -> Option<String> {
        let nwo = if f.repo.is_empty() {
            owner_repo(project)?
        } else {
            f.repo.clone()
        };
        let prs = open_prs(&nwo)?;
        match kind {
            Kind::Create => self.judge_create(project, &nwo, &prs, f),
            Kind::Edit => {
                // base に触れていない edit は対象外
                if !f.has_base {
                    return None;
                }
                judge_edit(project, &prs, f)
            }
        }
    }

    fn judge_create(&self, project: &Path, nwo: &str, prs: &str, f: &PrFlags) -> Option<String> {
        let head_branch = if f.head.is_empty() {
            git_out(project, &["branch", "--show-current"]).unwrap_or_default()
        } else {
            f.head.clone()
        };
        if head_branch.is_empty() {
            return None;
        }
        let head_sha = git_out(project, &["rev-parse", "HEAD"])?;

        let declared_base = if f.has_base {
            f.base.clone()
        } else {
            default_branch_or_gh(project, nwo).filter(|b| !b.is_empty())?
        };

        // --- 層(i): 状態レス祖先一致検査 ---
        if let Some((parent_num, parent_head)) =
            find_nearest_ancestor_pr(project, &head_sha, prs, &head_branch, "")
        {
            if declared_base != parent_head {
                return Some(format!(
                    "HEAD は open PR #{parent_num}({parent_head})のコミットを含んでいます。base を {declared_base} にすると先行 PR の差分がこの PR に混入します。`gh pr create --base {parent_head}` で作成してください。この積み方は依存の有無によらず常時とります(ADR-0027)。"
                ));
            }
            self.record_chain_head(&head_branch);
            return None;
        }

        // --- 層(ii): セッション内チェーン状態 ---
        self.judge_session_chain(prs, &head_branch, &declared_base, f)
    }

    fn judge_session_chain(
        &self,
        prs: &str,
        head_branch: &str,
        declared_base: &str,
        f: &PrFlags,
    ) -> Option<String> {
        let ledger = self.ledger();
        if !ledger.path(self.sid()).is_file() {
            self.record_chain_head(head_branch);
            return None; // このセッションでの初回 PR -> 通す
        }

        let live: Vec<String> = ledger
            .records(self.sid())
            .into_iter()
            .filter(|b| has_head(prs, b))
            .collect();
        let Some(last) = live.last() else {
            self.record_chain_head(head_branch);
            return None; // 記録済みの PR が全て閉じている/実在しない -> 新チェーン扱い
        };

        if declared_base == last {
            self.record_chain_head(head_branch);
            return None;
        }

        if !f.has_body {
            return None; // 本文フラグ無し -> 判定不能で通す
        }
        if f.body_text.is_empty() {
            return None; // 本文が読めない(コマンド置換等) -> 通す
        }
        if has_reasoned_tag(&f.body_text, INDEPENDENT_TAG) {
            self.record_chain_head(head_branch);
            return None;
        }

        Some(format!(
            "このセッションでは既に PR({last})を作成しています。2 本目以降は直前の段に積むのが既定です(ADR-0027)。`git rebase --onto {last} ...` で積み替えて `gh pr create --base {last}` とするか、真に独立な PR なら本文に `Independent-PR: <理由>` を書いて明示的に抜けてください。"
        ))
    }

    /// bash の `decide_mcp_stack`: MCP GitHub の書き込み系 tool(現在未接続、
    /// 命名は未確認、#161)。create_pull/update_pull 系のみ対象。
    pub fn decide_mcp_stack(&self, call: &ToolCall, project: &Path) -> Option<String> {
        if !(call.tool.contains("create_pull") || call.tool.contains("update_pull")) {
            return None;
        }
        let ti = |k: &str| jq_r_path(&call.raw, &["tool_input", k]);
        let base = ti("base").ok()?.unwrap_or_default();
        if base.is_empty() {
            return None;
        }
        let body = ti("body").ok().flatten().unwrap_or_default();
        let head = ti("head").ok().flatten().unwrap_or_default();
        let repo = ti("repo").ok().flatten().unwrap_or_default();

        let mut f = PrFlags {
            base,
            head,
            repo: repo.clone(),
            has_base: true,
            has_body: true,
            body_text: body,
            ..PrFlags::default()
        };

        let nwo = if repo.is_empty() {
            owner_repo(project)?
        } else {
            repo
        };
        let prs = open_prs(&nwo)?;

        if call.tool.contains("create_pull") {
            self.judge_create(project, &nwo, &prs, &f)
        } else {
            // update_pull: PR 番号を判定できない場合は判定不能で通す。
            let num = call.tool_input_alt(&["pull_number", "pullNumber"])?;
            if num.is_empty() {
                return None;
            }
            f.target = num;
            judge_edit(project, &prs, &f)
        }
    }
}

/// open PR 一覧の中から、`target_sha` の祖先になっている PR のうち最も近い
/// ものを探す。`(番号, headRefName)`。`self_head` / `self_num` は除外する
/// 自分自身(空なら除外しない)。
fn find_nearest_ancestor_pr(
    project: &Path,
    target_sha: &str,
    prs: &str,
    self_head: &str,
    self_num: &str,
) -> Option<(String, String)> {
    let mut best: Option<(u64, String, String)> = None;
    for r in stream_rows(prs) {
        if r.num.is_empty() {
            continue;
        }
        if !self_head.is_empty() && r.head == self_head {
            continue;
        }
        if !self_num.is_empty() && r.num == self_num {
            continue;
        }
        if r.oid.is_empty() {
            continue;
        }
        if !git_ok(
            project,
            &["cat-file", "-e", &format!("{}^{{commit}}", r.oid)],
        ) {
            continue;
        }
        if r.oid == target_sha {
            continue;
        }
        if !git_ok(
            project,
            &["merge-base", "--is-ancestor", &r.oid, target_sha],
        ) {
            continue;
        }
        let Some(dist) = git_out(
            project,
            &["rev-list", "--count", &format!("{}..{}", r.oid, target_sha)],
        )
        .and_then(|s| s.trim().parse::<u64>().ok()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(d, _, _)| dist < *d) {
            best = Some((dist, r.num, r.head));
        }
    }
    best.map(|(_, n, h)| (n, h))
}

fn judge_edit(project: &Path, prs: &str, f: &PrFlags) -> Option<String> {
    let row = if !f.target.is_empty() {
        let t = f.target.as_str();
        first_row_where(prs, |e| {
            Ok(jq_tostring(jq_index(e, "number")?) == t
                || matches!(jq_index(e, "headRefName")?, Value::String(s) if s == t))
        })
    } else {
        let cur = git_out(project, &["branch", "--show-current"]).unwrap_or_default();
        if cur.is_empty() {
            return None;
        }
        first_row_where(prs, |e| {
            Ok(matches!(jq_index(e, "headRefName")?, Value::String(s) if *s == cur))
        })
    }?; // 対象 PR が解決できない -> 通す

    if row.oid.is_empty() {
        return None;
    }
    if !git_ok(
        project,
        &["cat-file", "-e", &format!("{}^{{commit}}", row.oid)],
    ) {
        return None;
    }
    let declared_base = f.base.as_str();
    if declared_base.is_empty() {
        return None;
    }

    let (parent_num, parent_head) = find_nearest_ancestor_pr(project, &row.oid, prs, "", &row.num)?;
    if declared_base != parent_head {
        let (num, head) = (&row.num, &row.head);
        return Some(format!(
            "PR #{num}({head})は open PR #{parent_num}({parent_head})のコミットを含んでいます。base を {declared_base} にすると先行 PR の差分が混入します。`gh pr edit {num} --base {parent_head}` としてください。この積み方は依存の有無によらず常時とります(ADR-0027)。"
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// hook 入出力
// ---------------------------------------------------------------------------

/// `git -C <dir> rev-parse --is-inside-work-tree` が成功するか(bash と同じく
/// 終了コードだけを見る)。
fn in_work_tree(dir: &Path) -> bool {
    git_ok(dir, &["rev-parse", "--is-inside-work-tree"])
}

/// hook の判定(bash の `main` / Codex adapter の `main_codex`)。deny なら理由。
///
/// - Claude: project は `input.cwd`(git work tree のとき)→ `CLAUDE_PROJECT_DIR`
///   の順(#538: `CLAUDE_PROJECT_DIR` はセッション全体で固定なので、別
///   リポジトリを scratchpad 等に clone して `--repo` 無しで `gh pr create`
///   すると元のプロジェクトのリモートへ誤解決していた)。`Bash` と
///   `mcp__github*` を見る。
/// - Codex: project は `input.cwd` だけ。MCP tool 名の命名規則が Codex 側で
///   未確認(#161)なので `Bash` だけを見る。
/// - Copilot: bash 版に adapter が無かったので何もしない。
pub fn check(call: &ToolCall) -> Option<String> {
    let cwd = || {
        jq_r_path(&call.raw, &["cwd"])
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let project = match call.agent {
        Agent::Copilot => return None,
        Agent::Codex => {
            if !call.is_bash() {
                return None;
            }
            cwd()
        }
        Agent::Claude => {
            let c = cwd();
            if !c.is_empty() && in_work_tree(Path::new(&c)) {
                c
            } else {
                std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default()
            }
        }
    };
    if project.is_empty() {
        return None;
    }
    let project = Path::new(&project);
    if !in_work_tree(project) {
        return None;
    }

    // `jq -r '.session_id // "unknown"'`(空文字は `${SESSION_ID:-unknown}` で
    // unknown になる)。
    let sid = jq_r_path(&call.raw, &["session_id"])
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".into());
    let guard = Guard::from_env(sid);

    if call.tool == "Bash" {
        let cmd = call.bash_command()?;
        guard.decide_stack(&cmd, project)
    } else if call.agent == Agent::Claude && call.tool.starts_with("mcp__github") {
        guard.decide_mcp_stack(call, project)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_flags_one_pass() {
        let f = parse_pr_tokens(
            &v(&[
                "gh",
                "pr",
                "edit",
                "--title",
                "-R",
                "2",
                "--base=main",
                "-H",
                "h",
                "--body",
                "b",
            ]),
            "",
        );
        // `--title` の値 `-R` は読み飛ばされ、`2` が対象になる
        assert_eq!(f.repo, "");
        assert_eq!(f.target, "2");
        assert!(f.has_base);
        assert_eq!(f.base, "main");
        assert_eq!(f.head, "h");
        assert!(f.has_body);
        assert_eq!(f.body_text, "b");
        let f = parse_pr_tokens(&v(&["gh", "pr", "create", "--base"]), "");
        assert!(f.has_base);
        assert_eq!(f.base, "");
    }

    #[test]
    fn heredoc_body_is_appended() {
        let f = parse_pr_tokens(
            &v(&["gh", "pr", "create", "--body", "$(cat <<'E'"]),
            "Independent-PR: x\n",
        );
        assert_eq!(f.body_text, "$(cat <<'E'\nIndependent-PR: x");
    }

    #[test]
    fn rows_like_jq_tsv() {
        let rows = stream_rows(
            r#"[{"number":1,"headRefName":"a","headRefOid":"o","baseRefName":"main"},{"number":2,"headRefName":"","headRefOid":"p","baseRefName":"m"},3,{"number":4}]"#,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].head, "a");
        // 空フィールドは潰れて詰まる(IFS 空白)
        assert_eq!(rows[1].head, "p");
        assert_eq!(rows[1].oid, "m");
        assert!(stream_rows("not json").is_empty());
        assert!(has_head(r#"[{"headRefName":"a"}]"#, "a"));
        assert!(!has_head(r#"[{"headRefName":"a"},1]"#, "a"));
    }

    #[test]
    fn edit_row_selection() {
        let j = r#"[{"number":1,"headRefName":"s1","headRefOid":"o1"},{"number":2,"headRefName":"s2","headRefOid":"o2"}]"#;
        let r = first_row_where(j, |e| Ok(jq_tostring(jq_index(e, "number")?) == "2")).unwrap();
        assert_eq!(r.head, "s2");
        assert!(first_row_where(j, |_| Ok(false)).is_none());
    }
}
