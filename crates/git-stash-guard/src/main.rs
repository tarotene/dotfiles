//! git-stash-guard — PreToolUse(Bash)の stdin JSON を読み、素の stash 呼び出し
//! を deny する。`--host claude|codex`(既定 claude)。出力は両 host とも
//! `hookSpecificOutput` 形で同一。判定なし・縮退はすべて無出力 exit 0。

use agent_session::Agent;
use agent_session::Outcome;
use agent_session::PermissionDecision;
use std::io::Read;

fn main() {
    let mut agent = Agent::Claude;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let value = if a == "--host" {
            args.next()
        } else {
            a.strip_prefix("--host=").map(str::to_string)
        };
        if let Some(v) = value {
            match v.parse::<Agent>() {
                Ok(Agent::Copilot) | Err(_) => {
                    // exit 2 は Claude/Codex で「ブロック」扱いになるので使わない。
                    eprintln!("git-stash-guard: --host は claude|codex のみ: {v}");
                    std::process::exit(1);
                }
                Ok(h) => agent = h,
            }
        }
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(Outcome::Deny(reason)) = git_stash_guard::hook(&raw) {
        PermissionDecision::deny(reason).emit(agent);
    }
}
