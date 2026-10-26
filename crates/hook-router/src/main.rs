//! hook-router のエントリポイント。`hook-router pre|post`。
//!
//! 縮退: 標準入力が読めない・JSON として読めない・`tool_name` が無いなら、何も出さず exit 0
//! (判定できない入力は、断定に変えず素通す)。素通りにした規則は `gate-events.jsonl` に
//! `fail-open` で記録する(記録は best-effort)。

use hook_io::Agent;
use hook_router::{applicable, execute_for, merge, render, tool_name, Event, Verdict};
use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let compat = args.len() == 2 && args[0] == "compat";
    let agent = match args.as_slice() {
        [_] => Agent::Claude,
        [_, _] if compat => Agent::Codex,
        [_, flag, name] if flag == "--agent" && name == "codex" => Agent::Codex,
        _ => {
            eprintln!("usage: hook-router pre|post|permission [--agent codex]");
            std::process::exit(1);
        }
    };
    let atuin = args[0] == "atuin" && agent == Agent::Codex;
    let permission = args[0] == "permission" && agent == Agent::Codex;
    let event = match args[0].as_str() {
        "pre" => Event::Pre,
        "post" => Event::Post,
        "permission" if permission => Event::Pre,
        "compat" if compat => Event::Pre,
        "atuin" if atuin => Event::Pre,
        _ => {
            eprintln!("usage: hook-router pre|post");
            std::process::exit(1);
        }
    };

    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        return;
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
    if permission {
        if let Some(out) = hook_router::codex::permission(&raw) {
            println!("{out}");
        }
        return;
    }
    let inputs = if agent == Agent::Codex {
        hook_router::codex::inputs(&raw)
    } else {
        vec![raw]
    };
    let mut results = Vec::new();
    for raw in inputs {
        if let Some(tool) = tool_name(&raw) {
            results.extend(execute_for(agent, &applicable(event, &tool), &raw));
        }
    }

    if let Some(path) = hook_io::gate_event::default_path() {
        for (node, verdict) in &results {
            if matches!(verdict, Verdict::Panicked | Verdict::TimedOut) {
                let _ = hook_io::gate_event::record(&path, node, "fail-open");
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
