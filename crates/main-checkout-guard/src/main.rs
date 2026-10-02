//! main-checkout-guard — `pre`(PreToolUse)と `stop`(Stop)の 2 つのサブコマンド。
//! `--agent claude|codex`(既定 claude)。判定なし・縮退はすべて無出力 exit 0。

use hook_io::{Agent, HookInput, PermissionDecision};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;

fn main() {
    let mut agent = Agent::Claude;
    let mut sub = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let value = if a == "--agent" {
            args.next()
        } else {
            a.strip_prefix("--agent=").map(str::to_string)
        };
        match value {
            Some(v) => match v.parse::<Agent>() {
                Ok(Agent::Copilot) | Err(_) => {
                    // exit 2 は Claude/Codex で「ブロック」扱いになるので使わない。
                    eprintln!("main-checkout-guard: --agent は claude|codex のみ: {v}");
                    std::process::exit(1);
                }
                Ok(ag) => agent = ag,
            },
            None => sub = Some(a),
        }
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    let Some(input) = HookInput::parse(&raw) else {
        return;
    };
    match sub.as_deref() {
        Some("pre") => pre(&input, agent),
        Some("stop") => stop(&input, &raw),
        _ => {
            eprintln!("usage: main-checkout-guard [--agent claude|codex] pre|stop");
            std::process::exit(1);
        }
    }
}

fn pre(input: &HookInput, agent: Agent) {
    let Some(cwd) = input.cwd.clone().or_else(|| std::env::current_dir().ok()) else {
        return;
    };
    let analysis = main_checkout_guard::analyze(&input.tool_name, &input.tool_input, &cwd);

    for p in &analysis.mutations {
        if let Some(top) = main_checkout_guard::main_checkout_of(p) {
            PermissionDecision::deny(main_checkout_guard::deny_reason(&top)).emit(agent);
            return;
        }
    }

    let Some(ledger) = main_checkout_guard::ledger() else {
        return;
    };
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut notices = Vec::new();
    for p in &analysis.touches {
        let Some(top) = main_checkout_guard::main_checkout_of(p) else {
            continue;
        };
        if seen.contains(&top) {
            continue;
        }
        if let Some(n) = main_checkout_guard::record_touch(&ledger, &input.session_id, &top) {
            notices.push(n);
        }
        seen.push(top);
    }
    // PreToolUse の additionalContext は Claude Code のみ。Codex には出さない。
    if !notices.is_empty() && agent == Agent::Claude {
        println!(
            "{}",
            json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": notices.join("\n"),
            }})
        );
    }
}

fn stop(input: &HookInput, raw: &str) {
    // block の直後の再呼び出しでは黙る(台帳の更新が使えない場合の無限 block 対策)。
    let active = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("stop_hook_active").and_then(Value::as_bool))
        .unwrap_or(false);
    if active {
        return;
    }
    let Some(ledger) = main_checkout_guard::ledger() else {
        return;
    };
    let problems = main_checkout_guard::check_drift(&ledger, &input.session_id);
    if !problems.is_empty() {
        println!(
            "{}",
            json!({"decision": "block", "reason": main_checkout_guard::drift_reason(&problems)})
        );
    }
}
