//! decision-colocation-guard のエントリポイント。判定は
//! `decision_colocation::guard`(bash 版 decision-colocation-guard.sh の移植、
//! ADR-396、docs/agents/decision-colocation.md)。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse、matcher: "Bash|mcp__.*")
//!   手動 e2e:   decision-colocation-guard --check '<コマンド文字列>' (省略可: `<project-dir>`)
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): 不正 JSON・
//! project が git 作業ツリーでない場合は何も出さず exit 0。
//! Codex / Copilot 向けの adapter は意図的に作らない — CI required check が
//! 全エージェント共通の backstop になるため(ADR-396 の非目標)。

use std::path::PathBuf;
use std::process::ExitCode;

use agent_session::cli::{self, Verdict};
use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use decision_colocation::guard::Guard;

const SPEC: Spec = Spec {
    name: "decision-colocation-guard",
    agents: &[Agent::Claude],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };

    if let Some(rest) = cli::manual(&call.rest) {
        let verdict = match cli::args(rest, 1, 1) {
            Ok(a) => {
                let project = match a.get(1).filter(|p| !p.is_empty()) {
                    Some(p) => PathBuf::from(p),
                    None => std::env::current_dir().unwrap_or_default(),
                };
                Verdict::from(Guard::with_env(|g| g.decide(a[0], &project)))
            }
            Err(usage) => usage,
        };
        return cli::finish(
            "decision-colocation-guard",
            "decision-colocation-guard --check <command> [<project>]",
            verdict,
        );
    }

    let raw = read_stdin();
    let reply = Reply::from_outcome(decision_colocation::guard::hook(&raw));
    emit(&SPEC, &call, &raw, &reply)
}
