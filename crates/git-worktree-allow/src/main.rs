//! git-worktree-allow — PreToolUse(Bash、`if: "Bash(git -C *)"`)の stdin JSON
//! を読み、条件を満たす `git -C <herdr worktree> …` に allow を返す。非該当は
//! 無出力 exit 0(通常の permission フローへフォールスルー)。
//! 環境変数: `HERDR_WORKTREES_DIR`(既定 `$HOME/.herdr/worktrees`)。
//! 入口(引数・標準入力・出力)は `agent_session::entry` が持つ。allow を返す検査なので、ルータには載せない
//! (`if` の絞り込みが許可の範囲を決める。`docs/agents/hook-router.md`)。

use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, HookInput, Outcome, Reply};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "git-worktree-allow",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return ExitCode::SUCCESS;
    };
    let Some(cmd) = input.bash_command() else {
        return ExitCode::SUCCESS;
    };
    // bash 版は `$(jq …)` で末尾改行が落ちる。
    let cmd = cmd.trim_end_matches('\n');
    if cmd.is_empty() {
        return ExitCode::SUCCESS;
    }
    let Some(root) = git_worktree_allow::worktrees_root() else {
        return ExitCode::SUCCESS;
    };
    match git_worktree_allow::decide(cmd, &root) {
        Some(reason) => emit(
            &SPEC,
            &call,
            &raw,
            &Reply::from_outcome(Some(Outcome::Allow(reason))),
        ),
        None => ExitCode::SUCCESS,
    }
}
