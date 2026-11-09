//! attribution-guard のエントリポイント。判定は lib.rs。
//!
//! 3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から同じ 1 バイナリを
//! 呼ぶ(#391、pkexec-guard と同じ型)。bash 版の「Claude 版エンジンを
//! `source` する per-agent adapter」(config/{codex,copilot}/hooks/
//! attribution-guard.sh)の代わりに、`--agent <claude|codex|copilot>` で
//! 入力の形・フッター文言・出力の形を切り替える。省略時は `claude`。
//! 入口(引数・標準入力・出力)は `agent_session::entry` が持つ。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   attribution-guard [--agent <a>] --check '<コマンド文字列>'
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): 不正 JSON なら何も出さず exit 0。
//! 判定できない場合は断定に変えず素通す。

use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use attribution_guard::Attribution;
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "attribution-guard",
    agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };

    if let Some(pos) = call.rest.iter().position(|a| a == "--check") {
        let cmd = call.rest.get(pos + 1).map(String::as_str).unwrap_or("");
        return match Attribution::for_agent(call.agent).decide(cmd) {
            Some(reason) => {
                println!("deny: {reason}");
                ExitCode::from(1)
            }
            None => {
                println!("pass");
                ExitCode::SUCCESS
            }
        };
    }

    let raw = read_stdin();
    let reply = Reply::from_outcome(attribution_guard::hook(call.agent, &raw));
    emit(&SPEC, &call, &raw, &reply)
}
