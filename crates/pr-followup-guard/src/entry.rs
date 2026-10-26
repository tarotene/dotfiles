// pr-followup-guard のエントリポイント。判定は lib.rs。
//
// Claude Code と Codex CLI から同じ 1 バイナリを呼ぶ(#391、pkexec-guard と
// 同じ型)。bash 版の「Claude 版エンジンを `source` する Codex adapter」
// (config/codex/hooks/pr-confirm-guard.sh)の代わりに、`--agent claude|codex`
// で切り替える。省略時は `claude`。Copilot 版は元から無い(依頼は Codex のみ)。
//
// 使い方:
//   hook として: stdin JSON(PreToolUse)
//   手動 e2e:   pr-followup-guard --check '<コマンド文字列>' [<project-dir>]
//               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//
// 縮退(bash 版と同じ): stdin が読めない・不正 JSON・project が決まらない・
// git 作業ツリーの外なら何も出さず exit 0。判定不能を deny に変えない。
//
// project の決め方(bash 版の差): Claude は `CLAUDE_PROJECT_DIR`(空なら
// `.cwd`)、Codex は `.cwd` だけ。project は git 作業ツリーの中かを見るだけで
// 判定には使わない(bash 版でも未使用の引数)。

use agent_session::hook::agent_from_args;
use agent_session::hook::deny_output;
use agent_session::Outcome;
use pr_followup_guard::{allow_from_env, decide};
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = agent_from_args(&args);

    if let Some(pos) = args.iter().position(|a| a == "--check") {
        let cmd = args.get(pos + 1).map(String::as_str).unwrap_or("");
        return match decide(cmd, allow_from_env()) {
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
    if let Some(Outcome::Deny(reason)) = pr_followup_guard::hook(agent, &buf) {
        print!("{}", deny_output(agent, &reason));
    }
    ExitCode::SUCCESS
}
