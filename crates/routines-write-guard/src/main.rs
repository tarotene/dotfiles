//! hook エントリポイント。詳細は lib.rs / docs/claude/claude-routines.md。
//!
//! PreToolUse 専用。判定できない入力では何も出力せず exit 0 する
//! (判定できないことを deny に変えない — rulesets-write-guard と同じ
//! fail-open 方針)。入口(引数・標準入力・出力)は `agent_session::entry` が持つ。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "routines-write-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| routines_write_guard::hook(raw))
}
