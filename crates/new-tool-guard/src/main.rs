//! new-tool-guard — 3 つの顔を持つ 1 バイナリ。詳細は lib.rs /
//! docs/adr/543-existing-means-and-deterministic-promotion.md。
//!
//! - 無引数(hook モード): PreToolUse(Write)の stdin JSON を読み、新しい
//!   道具・単位を ledger 未登録のまま作ろうとしていれば deny する。
//! - `classify <path>`: ディスク上の `<path>` を読み、[`new_tool_guard::
//!   is_new_tool_unit`] の判定を単一正本として公開する(pr-gate.sh の
//!   `judge_prior` から呼ばれる、ADR-0024 の「hook と呼び出し元が同じ
//!   判定エンジンを共有する」型)。終了コード 0=該当・1=非該当。
//! - `register <既存手段: ...>`: 現在の worktree(git toplevel)の session
//!   ledger に 1 行追記する。

use hook_io::{Agent, PermissionDecision, SessionLedger};
use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("classify") => cmd_classify(args.next()),
        Some("register") => cmd_register(args.next()),
        Some(other) => {
            eprintln!("new-tool-guard: unknown subcommand: {other}(classify|register)");
            std::process::exit(2);
        }
        None => cmd_hook(),
    }
}

fn cmd_classify(path: Option<String>) {
    let Some(path) = path else {
        eprintln!("usage: new-tool-guard classify <path>");
        std::process::exit(2);
    };
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    if new_tool_guard::is_new_tool_unit(&path, &content) {
        println!("yes");
        std::process::exit(0);
    }
    println!("no");
    std::process::exit(1);
}

fn cmd_register(line: Option<String>) {
    let Some(line) = line else {
        eprintln!("usage: new-tool-guard register '既存手段: <path> — 採用|拡張|自前 — ...'");
        std::process::exit(2);
    };
    if let Err(reason) = new_tool_guard::is_valid_kizon_line(&line) {
        eprintln!("new-tool-guard register: 書式が不正です — {reason}");
        std::process::exit(1);
    }
    let Some(ledger) = default_ledger() else {
        eprintln!("new-tool-guard register: $HOME が解決できません");
        std::process::exit(1);
    };
    let Some(key) = ledger_key(&std::env::current_dir().unwrap_or_default()) else {
        eprintln!("new-tool-guard register: git worktree の外です(git toplevel を解決できません)");
        std::process::exit(1);
    };
    match ledger.append(&key, &line) {
        Ok(()) => println!("登録しました: {line}"),
        Err(e) => {
            eprintln!("new-tool-guard register: 書き込みに失敗しました: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_hook() {
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    if input.hook_event_name != "PreToolUse" || input.tool_name != "Write" {
        return;
    }
    if skipped() {
        record_gate_event("skip");
        return;
    }
    let Some(file_path) = input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    // 既存ファイルへの Write(上書き)は対象外(ADR-543 D1: 新規ファイル
    // 作成の瞬間だけを見る)。
    if Path::new(file_path).exists() {
        return;
    }
    let content = input
        .tool_input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !new_tool_guard::is_new_tool_unit(file_path, content) {
        return;
    }
    let Some(ledger) = default_ledger() else {
        return;
    };
    let cwd = input.project_dir().unwrap_or_default();
    let Some(key) = ledger_key(&cwd) else {
        return;
    };
    let records = ledger.records(&key);
    if new_tool_guard::ledger_has_record(&records, file_path) {
        return;
    }
    record_gate_event("deny");
    PermissionDecision::deny(new_tool_guard::deny_message(file_path)).emit(Agent::Claude);
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
fn default_ledger() -> Option<SessionLedger> {
    Some(SessionLedger::new(base_dir()?.join("state"), "ledger"))
}

fn base_dir() -> Option<PathBuf> {
    match std::env::var_os("NEW_TOOL_GUARD_DIR") {
        Some(d) if !d.is_empty() => Some(d.into()),
        _ => Some(Path::new(&std::env::var_os("HOME")?).join(".claude/new-tool-guard")),
    }
}

/// `SKIP_NEW_TOOL_GUARD=1` か `<base>/skip` の存在(既存 gate と同じ形)。
fn skipped() -> bool {
    std::env::var_os("SKIP_NEW_TOOL_GUARD").is_some_and(|v| v == "1")
        || base_dir().is_some_and(|d| d.join("skip").exists())
}

/// ledger キー: session_id ではなく git toplevel(ADR-543 D7 の差分)。
fn ledger_key(dir: &Path) -> Option<String> {
    hook_io::git::toplevel(dir)?.to_str().map(str::to_string)
}
