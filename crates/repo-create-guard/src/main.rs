//! repo-create-guard のエントリポイント。判定は lib.rs。
//!
//! 使い方: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から stdin JSON
//! で呼ばれる。Bash 以外(mcp__github* は現在未接続)は判定しない。
//!
//! 縮退(bash 版と同じ): `REPO_CREATE_GUARD_BYPASS` が空でなければ何もせず
//! exit 0(バイパスは hook 入出力層の責務)。stdin が読めない・不正 JSON・
//! Bash 以外・コマンドが空でも exit 0。

use guard_core::hook::{deny_output, ToolCall};
use hook_io::Agent;
use repo_create_guard::decide;
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::var_os("REPO_CREATE_GUARD_BYPASS").is_some_and(|v| !v.is_empty()) {
        return ExitCode::SUCCESS;
    }
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
    if let Some(reason) = decide(&cmd) {
        print!("{}", deny_output(Agent::Claude, &reason));
    }
    ExitCode::SUCCESS
}
