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

use agent_session::Agent;
use agent_session::Outcome;
use agent_session::PermissionDecision;
use new_tool_guard::hook::{default_ledger, ledger_key};
use std::io::Read;

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
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(Outcome::Deny(reason)) = new_tool_guard::hook::hook(&raw) {
        PermissionDecision::deny(reason).emit(Agent::Claude);
    }
}
