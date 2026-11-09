//! stack-base-guard のエントリポイント。判定は lib.rs。
//!
//! Claude Code と Codex CLI から同じ 1 バイナリを呼ぶ(#391、attribution-guard
//! と同じ型)。bash 版の「Claude 版エンジンを `source` する Codex adapter」
//! (config/codex/hooks/stack-base-guard.sh)の代わりに `--agent <claude|codex>`
//! で入力の読み方を切り替える。省略時は `claude`。state ディレクトリ
//! (~/.claude/stack-base-guard/state)は Claude と Codex で共有する —
//! session_id で区切られるため衝突しない。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。`--agent copilot` は受けるが、判定は何もしない
//! (`lib.rs` の `check` は Copilot を None にする)。
//!
//! 使い方:
//!   hook として: stdin JSON(PreToolUse)
//!   手動 e2e:   stack-base-guard --check '<コマンド文字列>' [<project-dir>]
//!               deny なら `deny: <理由>` を出して exit 1、通すなら `pass`。
//!               セッション ID は環境変数 `SESSION_ID`(無ければ unknown)。
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): 不正 JSON・git/gh が無い・
//! 判定できない場合は何も出さず exit 0。

use agent_session::cli::{self, Verdict};
use agent_session::entry::{emit, enter, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use stack_base_guard::Guard;
use std::path::PathBuf;
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "stack-base-guard",
    agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
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
                let project = a
                    .get(1)
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_default();
                let guard = Guard::from_env(std::env::var("SESSION_ID").unwrap_or_default());
                Verdict::from(guard.decide_stack(a[0], &project))
            }
            Err(usage) => usage,
        };
        return cli::finish(
            "stack-base-guard",
            "stack-base-guard [--agent claude|codex|copilot] --check <command> [<project>]  (環境変数 SESSION_ID)",
            verdict,
        );
    }

    // skip は stdin を読んだあとの `hook` の中で見る(stdin は常に読む)。
    let raw = read_stdin();
    let reply = Reply::from_outcome(stack_base_guard::hook(call.agent, &raw));
    emit(&SPEC, &call, &raw, &reply)
}
