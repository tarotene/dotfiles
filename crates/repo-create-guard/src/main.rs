//! repo-create-guard のエントリポイント。判定は lib.rs。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。
//!
//! 使い方: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から stdin JSON
//! で呼ばれる。Bash 以外(mcp__github* は現在未接続)は判定しない。
//!
//! 縮退(bash 版と同じ): `REPO_CREATE_GUARD_BYPASS` が空でなければ何もせず
//! exit 0(バイパスは hook 入出力層の責務)。不正 JSON・
//! Bash 以外・コマンドが空でも exit 0。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "repo-create-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| repo_create_guard::hook(raw))
}
