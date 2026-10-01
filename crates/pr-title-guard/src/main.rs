//! pr-title-guard のエントリポイント。判定は lib.rs。
//!
//! 3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から同じ 1 バイナリを
//! 呼ぶ(#391、attribution-guard と同じ型)。bash 版の「Claude 版エンジンを
//! `source` する per-agent adapter」(config/{codex,copilot}/hooks/
//! pr-title-guard.sh)の代わりに、`--agent <claude|codex|copilot>` で入力の
//! 形と出力の形を切り替える。省略時は `claude`。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   pr-title-guard --check '<コマンド文字列>' [<project-dir>]
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退: stdin が読めない・不正 JSON なら何も出さず exit 0(fail-open)。

use guard_core::hook::{agent_from_args, deny_output, ToolCall};
use pr_title_guard::{check, decide};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = agent_from_args(&args);
    let allow = std::env::var("PR_TITLE_GUARD_ALLOW").is_ok_and(|v| v == "1");

    if let Some(pos) = args.iter().position(|a| a == "--check") {
        let cmd = args.get(pos + 1).map(String::as_str).unwrap_or("");
        // bash の `proj="${3:-$PWD}"`(空文字列も PWD に倒す)。
        let project = match args.get(pos + 2).filter(|s| !s.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir().unwrap_or_default(),
        };
        return match decide(cmd, &project, allow) {
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

    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    let Some(call) = ToolCall::parse(agent, &buf) else {
        return ExitCode::SUCCESS;
    };
    let env_project = std::env::var("CLAUDE_PROJECT_DIR").ok();
    if let Some(reason) = check(&call, env_project.as_deref(), allow) {
        print!("{}", deny_output(agent, &reason));
    }
    ExitCode::SUCCESS
}
