//! PreToolUse(matcher: `mcp__.*`)から stdin JSON で呼ばれる。判定の本体は lib.rs。
//! 入口(引数・標準入力・出力)は `agent_session::entry` が持つ。
//!
//! 縮退: tool_name 無し・不正 JSON は黙って exit 0。`EXTERNAL_SEND_GUARD_ALLOW=1`
//! なら即 pass(この名前は理由文に書かない — 当事者が自分で bypass できてしまう)。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "external-send-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| external_send_guard::hook(raw))
}
