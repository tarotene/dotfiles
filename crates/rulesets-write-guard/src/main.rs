//! hook エントリポイント。詳細は lib.rs / docs/agents/rulesets-write-guard.md。
//!
//! PreToolUse 専用(gh-edit-allow と違い記録役を持たない)。判定できない
//! 入力では何も出力せず exit 0 する(判定できないことを deny に変えない)。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "rulesets-write-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| rulesets_write_guard::hook(raw))
}
