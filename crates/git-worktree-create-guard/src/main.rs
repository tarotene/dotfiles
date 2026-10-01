//! git-worktree-create-guard — PreToolUse(Bash)の stdin JSON を読み、直接の
//! worktree 追加を deny する。判定なし・縮退はすべて無出力 exit 0。

use hook_io::{Agent, HookInput, PermissionDecision};
use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    // 事前フィルタは `decide` がパース後の command に対して行う。生の JSON に
    // 対して見ると、タブが `\t` にエスケープされて `worktree<TAB>add` が
    // 素通りする(#637)。
    let Some(input) = HookInput::parse(&raw) else {
        return;
    };
    let Some(command) = input.bash_command() else {
        return;
    };
    // bash 版は `$(jq …)` で末尾改行が落ちる。
    let command = command.trim_end_matches('\n');
    if command.is_empty() {
        return;
    }
    if let Some(reason) = git_worktree_create_guard::decide(command) {
        PermissionDecision::deny(reason).emit(Agent::Claude);
    }
}
