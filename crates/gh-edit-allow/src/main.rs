//! hook エントリポイント。詳細は lib.rs / docs/agents/gh-edit-allow.md。
//!
//! どの失敗経路でも何も出力せず exit 0 する(判定できないことを deny に変えない)。
//! 入口(引数・標準入力・出力)は `agent_session::entry` が持つ。PostToolUse の記録役は何も出力しない。
//! PreToolUse の判定役は allow を返す検査なので、ルータには載せない。

use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, HookInput, Outcome, Reply};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "gh-edit-allow",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return ExitCode::SUCCESS;
    };
    if gh_edit_allow::skipped() {
        return ExitCode::SUCCESS;
    }
    let Some(ledger) = gh_edit_allow::default_ledger() else {
        return ExitCode::SUCCESS;
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
                return emit(
                    &SPEC,
                    &call,
                    &raw,
                    &Reply::from_outcome(Some(Outcome::from(d))),
                );
            }
        }
        _ => {}
    }
    ExitCode::SUCCESS
}
