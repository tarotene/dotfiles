//! エントリポイント。詳細は lib.rs / docs/agents/precedent-grounding.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e:   plan-precedent-gate --check <plan.md>
//!
//! スキップ手段: `touch ~/.claude/plan-precedent-gate/skip` または
//! `SKIP_PLAN_PRECEDENT_GATE=1`。

use agent_session::Outcome;
use jq_json_compat::jqfmt;
use plan_precedent_gate::{example_block, judge_precedent, today, trim_newlines};
use std::io::Write;
use std::process::ExitCode;

fn read_plan_file(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if path.is_empty() || !p.is_file() {
        return None;
    }
    let s = std::fs::read(p).ok()?;
    Some(trim_newlines(String::from_utf8_lossy(&s).into_owned()))
}

fn cmd_check(plan_file: &str) -> ExitCode {
    let Some(plan) = read_plan_file(plan_file) else {
        eprintln!("plan file not found: {plan_file}");
        return ExitCode::from(1);
    };
    let problems = judge_precedent(&plan);
    let mut out = std::io::stdout().lock();
    if problems.is_empty() {
        let _ = writeln!(out, "OK: 先行例との対比の検査を通過しました。");
        return ExitCode::SUCCESS;
    }
    for p in &problems {
        let _ = writeln!(out, "{p}");
    }
    let _ = writeln!(out, "\n{}", example_block(&today()));
    ExitCode::from(1)
}

fn hook() -> ExitCode {
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
    // 判定は lib の `hook`(ルータと同じ関数)。ここは出力の書式だけを持つ。
    if let Some(Outcome::Deny(reason)) = plan_precedent_gate::hook(&buf) {
        let event = agent_session::HookInput::parse(&buf)
            .unwrap_or_default()
            .hook_event_name;
        print!("{}", jqfmt::deny_for_event(&event, &reason));
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--check") => cmd_check(args.get(1).map(String::as_str).unwrap_or("")),
        _ => hook(),
    }
}
