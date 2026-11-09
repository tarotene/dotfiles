//! `herdr-agent-metadata [--agent claude|codex|copilot] [report|clear]`
//!
//! - `--agent` 省略時は claude。
//! - Copilot だけは hook input に event 名が乗らないので、位置引数の action
//!   (`report` / `clear`)で動作を選ぶ。他のエージェントは位置引数を無視する。
//!
//! stdin は常に読み切る(書き手を SIGPIPE で落とさないため、bash 版の `cat`
//! と同じ)。終了コードは常に 0。

use agent_session::Agent;

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
                    // hook として登録された bin: 終了コード 2 は「止める」の意味なので、使い方の誤りは 1。
                    eprintln!("herdr-agent-metadata: {e}");
                    std::process::exit(1);
                }
                None => {
                    eprintln!("herdr-agent-metadata: --agent needs a value");
                    std::process::exit(1);
                }
            }
        } else if !seen_action {
            action = a;
            seen_action = true;
        }
    }
    herdr_agent_metadata::run(agent, &action, &agent_session::entry::read_stdin());
}
