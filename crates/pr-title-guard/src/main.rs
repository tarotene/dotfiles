//! pr-title-guard のエントリポイント。判定は lib.rs。
//!
//! 3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から同じ 1 バイナリを
//! 呼ぶ(#391、attribution-guard と同じ型)。bash 版の「Claude 版エンジンを
//! `source` する per-agent adapter」(config/{codex,copilot}/hooks/
//! pr-title-guard.sh)の代わりに、`--agent <claude|codex|copilot>` で入力の
//! 形と出力の形を切り替える。省略時は `claude`。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   pr-title-guard --check '<コマンド文字列>' [<project-dir>]
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退: 不正 JSON なら何も出さず exit 0(fail-open)。

use agent_session::cli::{self, Verdict};
use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::escape::PR_TITLE_GUARD;
use agent_session::{Agent, HookEvent, Reply};
use pr_title_guard::decide;
use std::path::PathBuf;
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "pr-title-guard",
    agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };

    if let Some(rest) = cli::manual(&call.rest) {
        let allow = PR_TITLE_GUARD.active(None, None).is_some();
        let verdict = match cli::args(rest, 1, 1) {
            Ok(a) => {
                // bash の `proj="${3:-$PWD}"`(空文字列も PWD に倒す)。
                let project = match a.get(1).filter(|s| !s.is_empty()) {
                    Some(p) => PathBuf::from(p),
                    None => std::env::current_dir().unwrap_or_default(),
                };
                Verdict::from(decide(a[0], &project, allow))
            }
            Err(usage) => usage,
        };
        return cli::finish(
            "pr-title-guard",
            "pr-title-guard [--agent claude|codex|copilot] --check <command> [<project>]",
            verdict,
        );
    }

    let raw = read_stdin();
    let reply = Reply::from_outcome(pr_title_guard::hook(call.agent, &raw));
    emit(&SPEC, &call, &raw, &reply)
}
