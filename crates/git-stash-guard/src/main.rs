//! git-stash-guard — PreToolUse(Bash)の stdin JSON を読み、素の stash 呼び出し
//! を deny する。`--host claude|codex`(既定 claude)。出力は両 host とも
//! `hookSpecificOutput` 形で同一。判定なし・縮退はすべて無出力 exit 0。

use hook_io::{Agent, HookInput, PermissionDecision};
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
    // 事前フィルタは `decide` がパース後の command に対して行う。生の JSON に
    // 対して見ると、タブが `\t` にエスケープされて単語境界が崩れ、
    // `git<TAB>stash` が素通りする(#637)。
    let Some(input) = HookInput::parse(&raw) else {
        return;
    };
    let Some(cmd) = input.bash_command() else {
        return;
    };
    // bash 版は `$(jq …)` で末尾改行が落ちる。
    let cmd = cmd.trim_end_matches('\n');
    if cmd.is_empty() {
        return;
    }
    if let Some(reason) = git_stash_guard::decide(cmd) {
        PermissionDecision::deny(reason).emit(agent);
    }
}
