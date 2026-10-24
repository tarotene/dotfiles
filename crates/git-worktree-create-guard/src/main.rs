//! git-worktree-create-guard — PreToolUse(Bash)の stdin JSON を読み、直接の
//! worktree 追加を deny する。判定なし・縮退はすべて無出力 exit 0。

use hook_io::{Agent, Outcome, PermissionDecision};
use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(Outcome::Deny(reason)) = git_worktree_create_guard::hook(&raw) {
        PermissionDecision::deny(reason).emit(Agent::Claude);
    }
}
