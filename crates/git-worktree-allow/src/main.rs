//! git-worktree-allow — PreToolUse(Bash、`if: "Bash(git -C *)"`)の stdin JSON
//! を読み、条件を満たす `git -C <herdr worktree> …` に allow を返す。非該当は
//! 無出力 exit 0(通常の permission フローへフォールスルー)。
//! 環境変数: `HERDR_WORKTREES_DIR`(既定 `$HOME/.herdr/worktrees`)。

use hook_io::{Agent, PermissionDecision};

fn main() {
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    let Some(cmd) = input.bash_command() else {
        return;
    };
    // bash 版は `$(jq …)` で末尾改行が落ちる。
    let cmd = cmd.trim_end_matches('\n');
    if cmd.is_empty() {
        return;
    }
    let Some(root) = git_worktree_allow::worktrees_root() else {
        return;
    };
    if let Some(reason) = git_worktree_allow::decide(cmd, &root) {
        PermissionDecision::allow(reason).emit(Agent::Claude);
    }
}
