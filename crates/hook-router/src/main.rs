//! hook-router のエントリポイント。`hook-router pre|post`。
//!
//! 縮退: `agent-permissions` は入力変換前の全呼び出しを検査する。それ以外の検査は
//! JSON として読めない・`tool_name` が無い入力を素通す。素通りにした規則は `gate-events.jsonl` に
//! `fail-open` で記録する(記録は best-effort)。

use hook_io::Agent;
use hook_router::{
    applicable, execute_for, execute_inputs_for, merge, render, tool_name, Event, Tools, Verdict,
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
        // Still run the permissions check: a deployed policy must not be bypassed
        // by an unreadable payload. Other guards retain their fail-open behavior.
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
    if permission {
        let checks: Vec<_> = applicable(Event::Pre, "")
            .into_iter()
            .filter(|r| r.tools == Tools::All)
            .collect();
        let checked = merge(&execute_for(agent, &checks, &raw));
        if !checked.deny.is_empty() {
            // OpenAI Codex rust-v0.164.0, hooks/src/schema.rs:190-227
            // (retrieved 2026-10-26): PermissionRequest uses decision.behavior,
            // unlike PreToolUse's permissionDecision.
            // https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/hooks/src/schema.rs#L190-L227
            let reason = checked.deny.join("\n\n");
            println!(
                "{}",
                serde_json::json!({"hookSpecificOutput": {
                    "hookEventName": "PermissionRequest", "decision": {
                        "behavior": "deny", "message": reason
                    }
                }})
            );
            std::process::exit(0);
        }
        if !checked.ask.is_empty() {
            // No hook allow: the native approval prompt must run.
            std::process::exit(0);
        }
        if let Some(out) = hook_router::codex::permission(&raw) {
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

    if let Some(path) = hook_io::gate_event::default_path() {
        for (node, verdict) in &results {
            if matches!(verdict, Verdict::Panicked | Verdict::TimedOut) {
                let decision = if *node == "agent-permissions" {
                    "deny"
                } else {
                    "fail-open"
                };
                let _ = hook_io::gate_event::record(&path, node, decision);
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
