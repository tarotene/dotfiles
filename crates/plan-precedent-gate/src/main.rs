//! エントリポイント。詳細は lib.rs / docs/claude/precedent-grounding.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e:   plan-precedent-gate --check <plan.md>
//!
//! スキップ手段: `touch ~/.claude/plan-precedent-gate/skip` または
//! `SKIP_PLAN_PRECEDENT_GATE=1`。

use hook_io::jqfmt;
use plan_precedent_gate::{deny_message, example_block, judge_precedent};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn gate_dir() -> PathBuf {
    std::env::var_os("CLAUDE_PLAN_PRECEDENT_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/plan-precedent-gate")
        })
}

fn read_plan_file(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if path.is_empty() || !p.is_file() {
        return None;
    }
    let s = std::fs::read(p).ok()?;
    Some(trim_newlines(String::from_utf8_lossy(&s).into_owned()))
}

/// `$(cat …)` と同じく末尾の改行を落とす。
fn trim_newlines(mut s: String) -> String {
    while s.ends_with('\n') {
        s.pop();
    }
    s
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
    let _ = writeln!(out, "\n{}", example_block(&hook_io::proc::date("%Y-%m-%d")));
    ExitCode::from(1)
}

fn hook() -> ExitCode {
    if gate_dir().join("skip").exists()
        || std::env::var("SKIP_PLAN_PRECEDENT_GATE").as_deref() == Ok("1")
    {
        return ExitCode::SUCCESS;
    }
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
    // 不正 JSON でも bash 版(jq が空を返す)と同じく最新プランへ縮退する。
    let input = hook_io::HookInput::parse(&buf).unwrap_or_default();
    input.enter_cwd();
    let Some(plan) = hook_io::plan::plan_text(&input) else {
        return ExitCode::SUCCESS;
    };
    let problems = judge_precedent(&trim_newlines(plan));
    if problems.is_empty() {
        return ExitCode::SUCCESS;
    }
    let msg = deny_message(&problems, &hook_io::proc::date("%Y-%m-%d"));
    print!("{}", jqfmt::deny_for_event(&input.hook_event_name, &msg));
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--check") => cmd_check(args.get(1).map(String::as_str).unwrap_or("")),
        _ => hook(),
    }
}
