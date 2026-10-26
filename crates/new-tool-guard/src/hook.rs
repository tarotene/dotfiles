//! hook モード(無引数)の本体と、その補助(台帳・skip・gate イベント)。
//!
//! `main` の `register` / `classify` も同じ台帳・ledger キーを使うので、`pub` にしてある。

use hook_io::{HookInput, Outcome, SessionLedger};
use std::path::{Path, PathBuf};

/// hook 本体(`main` が標準入力を読んだあとの処理)。`main` と、ルータ(`crates/hook-router`)が
/// 同じ関数を呼ぶ。入力が読めない・不正 JSON などは `None`(判定しない)。
///
/// 記録役(PostToolUse の ExitPlanMode)と判定役(PreToolUse の Write)を 1 つで兼ねる
/// (`gh-edit-allow` と同じ型)。deny のときだけ `Some(Outcome::Deny(…))`。
pub fn hook(raw: &str) -> Option<Outcome> {
    let input = HookInput::parse(raw)?;
    // 記録役: ExitPlanMode の承認後に、計画の `既存手段:` 行を台帳へ転記する。
    if input.hook_event_name == "PostToolUse" && input.tool_name == "ExitPlanMode" {
        if !skipped() {
            record_plan_lines(&input);
        }
        return None;
    }
    // 判定役: PreToolUse(Write)。
    if input.hook_event_name != "PreToolUse" || input.tool_name != "Write" {
        return None;
    }
    if skipped() {
        record_gate_event("skip");
        return None;
    }
    let file_path = input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    // 既存ファイルへの Write(上書き)は対象外(ADR-543 D1: 新規ファイル
    // 作成の瞬間だけを見る)。
    if Path::new(file_path).exists() {
        return None;
    }
    let content = input
        .tool_input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !crate::is_new_tool_unit(file_path, content) {
        return None;
    }
    let ledger = default_ledger()?;
    // キーは `CLAUDE_PROJECT_DIR`(並列 worktree のサブエージェントでは親の
    // リポジトリを指しうる)ではなく、書き込み先ファイルが属する worktree の
    // toplevel にする。`register` は cwd の toplevel をキーにするので、
    // 書き込み先と同じ worktree から register していれば必ず同じキーになる
    // (#661)。
    let fallback = input.project_dir().unwrap_or_default();
    let root = write_target_toplevel(file_path, &fallback)?;
    let key = root.to_str()?;
    let records = ledger.records(key);
    if crate::ledger_has_record(&records, &root, file_path) {
        return None;
    }
    record_gate_event("deny");
    let shown = crate::normalize_path(&root, file_path);
    Some(Outcome::Deny(crate::deny_message(&shown)))
}

/// 計画の `既存手段:` 行を、現在の worktree(git toplevel)の台帳へ書く。
/// Claude は承認後、Codex は Plan 検査通過後に呼ぶ。記録は検討の証跡で、承認を表さない。
/// 台帳は計画の派生で、正本は計画。手書きの `register` は、計画の無い作業の経路として残る。
/// 計画が取れない・節が無い・git の外・台帳が書けない場合は、何もしない(記録は
/// best-effort で、承認そのものに影響させない)。同じ行がすでにあれば足さない。
pub fn record_plan_lines(input: &HookInput) {
    let Some(plan) = hook_io::plan::plan_text(input) else {
        return;
    };
    let lines = crate::kizon_lines_in_plan(&plan);
    if lines.is_empty() {
        return;
    }
    let Some(ledger) = default_ledger() else {
        return;
    };
    let Some(root) = input.project_dir().and_then(|d| hook_io::git::toplevel(&d)) else {
        return;
    };
    let Some(key) = root.to_str() else {
        return;
    };
    let existing = ledger.records(key);
    for line in lines {
        if existing.iter().any(|r| r.trim() == line) {
            continue;
        }
        let _ = ledger.append(key, &line);
    }
}

/// 新規ファイル `file_path` が属する worktree の git toplevel。ファイルも
/// 親ディレクトリもまだ無いことがあるので、実在する最も近い祖先で解決する。
/// 見つからなければ `fallback`(hook 入力の project dir)で解決する。
fn write_target_toplevel(file_path: &str, fallback: &Path) -> Option<PathBuf> {
    let mut dir = Path::new(file_path).parent();
    while let Some(d) = dir {
        if d.is_dir() {
            return hook_io::git::toplevel(d).or_else(|| hook_io::git::toplevel(fallback));
        }
        dir = d.parent();
    }
    hook_io::git::toplevel(fallback)
}

/// ADR-543 段3: 降格候補検出(段4 の promotion-detect)の入力になる
/// deny/skip イベントを記録する。記録の成否は判定に影響させない
/// (best-effort — `hook_io::gate_event::record` のエラーは黙って握り潰す)。
fn record_gate_event(decision: &str) {
    if let Some(path) = hook_io::gate_event::default_path() {
        let _ = hook_io::gate_event::record(&path, "new-tool-guard", decision);
    }
}

/// `${NEW_TOOL_GUARD_DIR:-$HOME/.claude/new-tool-guard}/state/<key>.ledger`
/// (`gh-edit-allow` と同じ形)。
pub fn default_ledger() -> Option<SessionLedger> {
    Some(SessionLedger::new(base_dir()?.join("state"), "ledger"))
}

fn base_dir() -> Option<PathBuf> {
    match std::env::var_os("NEW_TOOL_GUARD_DIR") {
        Some(d) if !d.is_empty() => Some(d.into()),
        _ => Some(Path::new(&std::env::var_os("HOME")?).join(".claude/new-tool-guard")),
    }
}

/// `SKIP_NEW_TOOL_GUARD=1` か `<base>/skip` の存在(既存 gate と同じ形)。
pub fn skipped() -> bool {
    std::env::var_os("SKIP_NEW_TOOL_GUARD").is_some_and(|v| v == "1")
        || base_dir().is_some_and(|d| d.join("skip").exists())
}

/// ledger キー: session_id ではなく git toplevel(ADR-543 D7 の差分)。
pub fn ledger_key(dir: &Path) -> Option<String> {
    hook_io::git::toplevel(dir)?.to_str().map(str::to_string)
}
