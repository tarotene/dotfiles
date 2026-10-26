//! main-checkout-guard — `pre`(PreToolUse)と `stop`(Stop)の 2 つのサブコマンド。
//! `--agent claude|codex`(既定 claude)。判定なし・縮退はすべて無出力 exit 0。

use hook_io::{Agent, HookInput, Outcome, PermissionDecision};
use serde_json::{json, Value};
use std::io::Read;

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
    match main_checkout_guard::pre_hook(agent, input) {
        Some(Outcome::Deny(reason)) => PermissionDecision::deny(reason).emit(agent),
        Some(Outcome::Ask(reason)) => PermissionDecision::ask(reason).emit(agent),
        Some(Outcome::Allow(reason)) => PermissionDecision::allow(reason).emit(agent),
        Some(Outcome::Context(text)) => println!(
            "{}",
            json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": text,
            }})
        ),
        None => {}
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
