//! `herdr-agent-metadata [--agent claude|codex|copilot] [report|clear]`
//!
//! - `--agent` 省略時は claude。
//! - Copilot だけは hook input に event 名が乗らないので、位置引数の action
//!   (`report` / `clear`)で動作を選ぶ。他のエージェントは位置引数を無視する。
//!
//! stdin は常に読み切る(書き手を SIGPIPE で落とさないため、bash 版の `cat`
//! と同じ)。終了コードは常に 0。

use hook_io::Agent;
use std::io::Read;

fn main() {
    let mut agent = Agent::Claude;
    let mut action = String::new();
    let mut args = std::env::args().skip(1);
    let mut seen_action = false;
    while let Some(a) = args.next() {
        if a == "--agent" {
            match args.next().map(|v| v.parse::<Agent>()) {
                Some(Ok(v)) => agent = v,
                Some(Err(e)) => {
                    eprintln!("herdr-agent-metadata: {e}");
                    std::process::exit(64);
                }
                None => {
                    eprintln!("herdr-agent-metadata: --agent needs a value");
                    std::process::exit(64);
                }
            }
        } else if !seen_action {
            action = a;
            seen_action = true;
        }
    }
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    herdr_agent_metadata::run(agent, &action, &String::from_utf8_lossy(&buf));
}
