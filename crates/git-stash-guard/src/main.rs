//! git-stash-guard — PreToolUse(Bash)の stdin JSON を読み、素の stash 呼び出し
//! を deny する。`--host claude|codex`(既定 claude)。出力は両 host とも
//! `hookSpecificOutput` 形で同一。判定なし・縮退はすべて無出力 exit 0。
//!
//! 入口(引数・標準入力・出力)は `agent_session::entry` が持つ。`--host` は、退役済みの登録
//! (`home/modules/agents.nix` の退役一覧)が使っていた綴りで、`--agent` に読み替えて同じ解析に通す。
//! 退役一覧を消すとき(#979)に、この読み替えも消す。

use agent_session::entry::{emit, parse_call, read_stdin, Spec};
use agent_session::{Agent, HookEvent, Reply};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "git-stash-guard",
    agents: &[Agent::Claude, Agent::Codex],
    event: Some(HookEvent::PreToolUse),
};

/// `--host X` と `--host=X` を `--agent X` と `--agent=X` に読み替える。
fn host_as_agent(argv: impl Iterator<Item = String>) -> Vec<String> {
    argv.map(|a| {
        if a == "--host" {
            "--agent".to_string()
        } else if let Some(v) = a.strip_prefix("--host=") {
            format!("--agent={v}")
        } else {
            a
        }
    })
    .collect()
}

fn main() -> ExitCode {
    let argv = host_as_agent(std::env::args().skip(1));
    let call = match parse_call(&SPEC, &argv) {
        Ok(call) => call,
        Err(message) => {
            // exit 2 は Claude/Codex で「ブロック」扱いになるので使わない。
            eprintln!("git-stash-guard: {message}");
            return ExitCode::from(1);
        }
    };
    let raw = read_stdin();
    let reply = Reply::from_outcome(git_stash_guard::hook(&raw));
    emit(&SPEC, &call, &raw, &reply)
}
