//! hook-router のエントリポイント。`hook-router pre|post`。
//!
//! JSON として読めない入力は素通しし、独立した品質検査を実行する。

use agent_session::Agent;
use hook_router::{
    applicable, execute_inputs_for, merge, render_for, tool_name, Event, Tools, Verdict,
};
use std::io::Write;
use std::time::Duration;

/// `hook-router wrap <command> [--timeout <秒>]`: Claude の形で宣言された hook の command を、
/// agy の hook として走らせる(`hook_router::antigravity::wrap`)。
fn wrap_main(args: &[String]) {
    let (command, limit) = match args {
        [command] => (command, 30),
        [command, flag, secs] if flag == "--timeout" => match secs.parse() {
            Ok(n) => (command, n),
            Err(_) => usage_wrap(),
        },
        _ => usage_wrap(),
    };
    let raw = agent_session::entry::read_stdin();
    let out = hook_router::antigravity::wrap(command, &raw, Duration::from_secs(limit));
    let _ = std::io::stdout().write_all(out.as_bytes());
}

fn usage_wrap() -> ! {
    eprintln!("usage: hook-router wrap <command> [--timeout <seconds>]");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("wrap") {
        wrap_main(&args[1..]);
        return;
    }
    let compat = args.len() == 2 && args[0] == "compat";
    let agent = match args.as_slice() {
        [_] => Agent::Claude,
        [_, _] if compat => Agent::Codex,
        [_, flag, name] if flag == "--agent" && name == "codex" => Agent::Codex,
        [_, flag, name] if flag == "--agent" && name == "antigravity" => Agent::Antigravity,
        _ => {
            eprintln!("usage: hook-router pre|post [--agent codex|antigravity] | wrap <command>");
            std::process::exit(1);
        }
    };
    let atuin = args[0] == "atuin" && agent == Agent::Codex;
    let event = match args[0].as_str() {
        "pre" => Event::Pre,
        "post" => Event::Post,
        "compat" if compat => Event::Pre,
        "atuin" if atuin => Event::Pre,
        _ => {
            eprintln!("usage: hook-router pre|post");
            std::process::exit(1);
        }
    };

    // 不正な UTF-8 は、jq と同じく U+FFFD に置き換えてから読む(`read_stdin` が持つ)。
    let raw = agent_session::entry::normalize_for(agent, agent_session::entry::read_stdin());
    if atuin {
        hook_router::codex::atuin(&raw);
        return;
    }
    if compat {
        if let Some(out) = hook_router::codex::compatibility(&args[1], &raw) {
            println!("{out}");
        }
        return;
    }
    let all: Vec<_> = applicable(event, "")
        .into_iter()
        .filter(|r| r.tools == Tools::All)
        .collect();
    let mut checks: Vec<_> = all.into_iter().map(|rule| (rule, raw.clone())).collect();
    let inputs = if agent == Agent::Codex {
        hook_router::codex::inputs(&raw)
    } else {
        vec![raw]
    };
    for raw in inputs {
        if let Some(tool) = tool_name(&raw) {
            let rules: Vec<_> = applicable(event, &tool)
                .into_iter()
                .filter(|r| r.tools != Tools::All)
                .collect();
            checks.extend(rules.into_iter().map(|rule| (rule, raw.clone())));
        }
    }
    let results = execute_inputs_for(agent, &checks);

    if let Some(path) = agent_session::gate_event::default_path() {
        for (node, verdict) in &results {
            if matches!(verdict, Verdict::Panicked | Verdict::TimedOut) {
                // Persisted event identities survive display/package renames.
                let recorded_node = if *node == "pr-followup-guard" {
                    "pr-confirm-guard"
                } else {
                    node
                };
                let _ = agent_session::gate_event::record(&path, recorded_node, "fail-open");
            }
        }
    }

    if let Some(rendered) = render_for(agent, event, &merge(&results)) {
        // 描画の文字列が、末尾の改行まで持つ(`Reply::render`)。
        let mut out = std::io::stdout();
        let _ = out.write_all(rendered.as_bytes());
        let _ = out.flush();
    }
    // 時間を越えた規則のスレッドを待たずに、プロセスごと終わる。
    std::process::exit(0);
}
