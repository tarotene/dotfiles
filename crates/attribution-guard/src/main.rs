//! attribution-guard のエントリポイント。判定は lib.rs。
//!
//! 3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から同じ 1 バイナリを
//! 呼ぶ(#391、pkexec-guard と同じ型)。bash 版の「Claude 版エンジンを
//! `source` する per-agent adapter」(config/{codex,copilot}/hooks/
//! attribution-guard.sh)の代わりに、`--agent <claude|codex|copilot>` で
//! 入力の形・フッター文言・出力の形を切り替える。省略時は `claude`。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   attribution-guard [--agent <a>] --check '<コマンド文字列>'
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): stdin が読めない・
//! 不正 JSON なら何も出さず exit 0。判定できない場合は断定に変えず素通す。

use attribution_guard::{check, Attribution};
use guard_core::hook::{agent_from_args, deny_output, ToolCall};
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = agent_from_args(&args);

    if let Some(pos) = args.iter().position(|a| a == "--check") {
        let cmd = args.get(pos + 1).map(String::as_str).unwrap_or("");
        return match Attribution::for_agent(agent).decide(cmd) {
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
    if let Some(reason) = check(&call) {
        print!("{}", deny_output(agent, &reason));
    }
    ExitCode::SUCCESS
}
