//! herdr-protocol-check — SessionStart hook(matcher: `startup|resume`)。
//! `herdr status --json` を短い timeout で実行し、protocol 不一致・再起動要・
//! サーババイナリ陳腐化のときだけ `additionalContext` を stdout に出す。
//! 全失敗経路は fail-open(無出力 exit 0)。設計: docs/claude/herdr-protocol-check.md。

use herdr_protocol_check::decide;
use serde_json::json;
use std::process::Command;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);

fn run() -> Option<()> {
    if !hook_io::proc::command_exists("herdr") {
        return None;
    }
    let (status, out) = hook_io::proc::output_with_timeout(
        Command::new("herdr").args(["status", "--json"]),
        TIMEOUT,
        None,
    )?;
    if !status.success() {
        return None;
    }
    let ctx = decide(&String::from_utf8_lossy(&out))?;
    println!(
        "{}",
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": ctx}})
    );
    Some(())
}

fn main() {
    let _ = run();
}
