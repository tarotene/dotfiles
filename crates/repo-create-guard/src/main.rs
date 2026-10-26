//! repo-create-guard のエントリポイント。判定は lib.rs。
//!
//! 使い方: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から stdin JSON
//! で呼ばれる。Bash 以外(mcp__github* は現在未接続)は判定しない。
//!
//! 縮退(bash 版と同じ): `REPO_CREATE_GUARD_BYPASS` が空でなければ何もせず
//! exit 0(バイパスは hook 入出力層の責務)。stdin が読めない・不正 JSON・
//! Bash 以外・コマンドが空でも exit 0。

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
    if let Some(Outcome::Deny(reason)) = repo_create_guard::hook(&buf) {
        print!("{}", deny_output(Agent::Claude, &reason));
    }
    ExitCode::SUCCESS
}
