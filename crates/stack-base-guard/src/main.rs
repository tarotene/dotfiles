//! stack-base-guard のエントリポイント。判定は lib.rs。
//!
//! Claude Code と Codex CLI から同じ 1 バイナリを呼ぶ(#391、attribution-guard
//! と同じ型)。bash 版の「Claude 版エンジンを `source` する Codex adapter」
//! (config/codex/hooks/stack-base-guard.sh)の代わりに `--agent <claude|codex>`
//! で入力の読み方を切り替える。省略時は `claude`。state ディレクトリ
//! (~/.claude/stack-base-guard/state)は Claude と Codex で共有する —
//! session_id で区切られるため衝突しない。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   stack-base-guard --check '<コマンド文字列>' [<project-dir>]
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`。
//!               セッション ID は環境変数 `SESSION_ID`(無ければ unknown)。
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): stdin が読めない・
//! 不正 JSON・git/gh が無い・判定できない場合は何も出さず exit 0。

use guard_core::hook::{agent_from_args, deny_output, ToolCall};
use stack_base_guard::{check, Guard};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = agent_from_args(&args);

    if let Some(pos) = args.iter().position(|a| a == "--check") {
        let cmd = args.get(pos + 1).map(String::as_str).unwrap_or("");
        let project = args
            .get(pos + 2)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let guard = Guard::from_env(std::env::var("SESSION_ID").unwrap_or_default());
        return match guard.decide_stack(cmd, &project) {
            Some(reason) => {
                println!("deny: {reason}");
                ExitCode::from(1)
            }
            None => {
                println!("pass");
                ExitCode::SUCCESS
            }
        };
    }

    // skip は stdin を読む前に見る(bash 版 main と同じ順)。session_id は
    // skip 判定に関係しない。
    if Guard::from_env(String::new()).skipped() {
        return ExitCode::SUCCESS;
    }

    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    let Some(call) = ToolCall::parse(agent, &buf) else {
        return ExitCode::SUCCESS;
    };
    if let Some(reason) = check(&call) {
        print!("{}", deny_output(agent, &reason));
    }
    ExitCode::SUCCESS
}
