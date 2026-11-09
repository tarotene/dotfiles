//! pr-gate の入口。`pr-gate session-start` / `pr-gate stop` を settings.json の
//! SessionStart / Stop から stdin JSON 付きで呼ぶ。それ以外の引数は何もせず
//! exit 0(bash 版の dispatch と同じ)。設計は lib.rs と docs/agents/pr-gate.md。

use agent_session::entry::{run_stop, StopSpec};
use agent_session::Agent;
use std::process::ExitCode;

const STOP_SPEC: StopSpec = StopSpec {
    name: "pr-gate",
    agents: &[Agent::Claude, Agent::Codex],
    reentry: pr_gate::stop::REENTRY,
};

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("session-start") => {
            let input = agent_session::entry::read_stdin();
            ExitCode::from(u8::try_from(pr_gate::session_start::run(&input)).unwrap_or(1))
        }
        Some("stop") => run_stop(&STOP_SPEC, |input| Ok(pr_gate::stop::run(input.raw()))),
        _ => ExitCode::SUCCESS,
    }
}
