//! feedback-target-guard のエントリポイント。判定は lib.rs。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。
//!
//! 使い方: settings.json の PreToolUse(matcher: Bash)から stdin JSON で
//! 呼ばれる。Bash 以外は判定しない。
//!
//! 縮退(bash 版と同じ): 不正 JSON・Bash 以外・コマンドが
//! 空なら何も出さず exit 0。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "feedback-target-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| feedback_target_guard::hook(raw))
}
