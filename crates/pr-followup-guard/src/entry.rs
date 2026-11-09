// pr-followup-guard のエントリポイント。判定は lib.rs。
//
// Claude Code と Codex CLI から同じ 1 バイナリを呼ぶ(#391、pkexec-guard と
// 同じ型)。bash 版の「Claude 版エンジンを `source` する Codex adapter」
// (config/codex/hooks/pr-confirm-guard.sh)の代わりに、`--agent claude|codex`
// で切り替える。省略時は `claude`。Copilot 版は元から無い(依頼は Codex のみ)が、
// `--agent copilot` は受ける(判定は何もしない)。入口(引数・標準入力・出力)は
// `agent_session::entry` が持つ。
//
// 使い方:
//   hook として: stdin JSON(PreToolUse)
//   手動 e2e:   pr-followup-guard --check '<コマンド文字列>' [<project-dir>]
//               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//
// 縮退(bash 版と同じ): 不正 JSON・project が決まらない・
// git 作業ツリーの外なら何も出さず exit 0。判定不能を deny に変えない。
//
// project の決め方(bash 版の差): Claude は `CLAUDE_PROJECT_DIR`(空なら
// `.cwd`)、Codex は `.cwd` だけ。project は git 作業ツリーの中かを見るだけで
// 判定には使わない(bash 版でも未使用の引数)。

use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use pr_followup_guard::{allow_from_env, decide};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "pr-followup-guard",
    agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };

    if let Some(pos) = call.rest.iter().position(|a| a == "--check") {
        let cmd = call.rest.get(pos + 1).map(String::as_str).unwrap_or("");
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

    let raw = read_stdin();
    let reply = Reply::from_outcome(pr_followup_guard::hook(call.agent, &raw));
    emit(&SPEC, &call, &raw, &reply)
}
