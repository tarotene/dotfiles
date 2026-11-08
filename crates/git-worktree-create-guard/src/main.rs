//! git-worktree-create-guard — PreToolUse(Bash)の stdin JSON を読み、直接の
//! worktree 追加を deny する。判定なし・縮退はすべて無出力 exit 0。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "git-worktree-create-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, |_, raw| git_worktree_create_guard::hook(raw))
}
