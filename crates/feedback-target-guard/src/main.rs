//! feedback-target-guard のエントリポイント。判定は lib.rs。
//!
//! 使い方: settings.json の PreToolUse(matcher: Bash)から stdin JSON で
//! 呼ばれる。Bash 以外は判定しない。
//!
//! 縮退(bash 版と同じ): stdin が読めない・不正 JSON・Bash 以外・コマンドが
//! 空なら何も出さず exit 0。

use feedback_target_guard::{decide, Home};
use guard_core::hook::{deny_output, ToolCall};
use hook_io::Agent;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    let Some(call) = ToolCall::parse(Agent::Claude, &buf) else {
        return ExitCode::SUCCESS;
    };
    let Some(cmd) = call.bash_command() else {
        return ExitCode::SUCCESS;
    };
    if let Some(reason) = decide(Home::from_env().as_ref(), &cmd) {
        print!("{}", deny_output(Agent::Claude, &reason));
    }
    ExitCode::SUCCESS
}
