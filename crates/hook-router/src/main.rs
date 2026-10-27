//! hook-router のエントリポイント。`hook-router pre|post`。
//!
//! JSON として読めない入力は素通しし、独立した品質検査を実行する。

use agent_session::Agent;
use hook_router::{
    applicable, execute_inputs_for, merge, render, tool_name, Event, Tools, Verdict,
};
use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let compat = args.len() == 2 && args[0] == "compat";
    let agent = match args.as_slice() {
        [_] => Agent::Claude,
        [_, _] if compat => Agent::Codex,
        [_, flag, name] if flag == "--agent" && name == "codex" => Agent::Codex,
        _ => {
            eprintln!("usage: hook-router pre|post [--agent codex]");
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

    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        bytes.clear();
    }
    // jq と同じく、不正な UTF-8 は U+FFFD に置き換えてから読む。
    let raw = String::from_utf8_lossy(&bytes).into_owned();
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

    if let Some(line) = render(event, &merge(&results)) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
    // 時間を越えた規則のスレッドを待たずに、プロセスごと終わる。
    std::process::exit(0);
}
