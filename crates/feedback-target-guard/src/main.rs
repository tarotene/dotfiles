//! feedback-target-guard のエントリポイント。判定は lib.rs。
//!
//! 使い方: settings.json の PreToolUse(matcher: Bash)から stdin JSON で
//! 呼ばれる。Bash 以外は判定しない。
//!
//! 縮退(bash 版と同じ): stdin が読めない・不正 JSON・Bash 以外・コマンドが
//! 空なら何も出さず exit 0。

use agent_session::hook::deny_output;
use agent_session::Agent;
use agent_session::Outcome;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    if let Some(Outcome::Deny(reason)) = feedback_target_guard::hook(&buf) {
        print!("{}", deny_output(Agent::Claude, &reason));
    }
    ExitCode::SUCCESS
}
