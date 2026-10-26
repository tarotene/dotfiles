//! hook エントリポイント。詳細は lib.rs / docs/agents/gh-edit-allow.md。
//!
//! どの失敗経路でも何も出力せず exit 0 する(判定できないことを deny に変えない)。

use agent_session::Agent;

fn main() {
    let Some(input) = agent_session::input::read_stdin() else {
        return;
    };
    if gh_edit_allow::skipped() {
        return;
    }
    let Some(ledger) = gh_edit_allow::default_ledger() else {
        return;
    };
    match input.hook_event_name.as_str() {
        "PostToolUse" => {
            gh_edit_allow::record(&input, &ledger);
        }
        "PreToolUse" => {
            let origin = input
                .project_dir()
                .and_then(|d| git_repository::origin_nwo(&d));
            if let Some(d) = gh_edit_allow::check(&input, &ledger, origin.as_deref()) {
                d.emit(Agent::Claude);
            }
        }
        _ => {}
    }
}
