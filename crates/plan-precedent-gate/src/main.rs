//! エントリポイント。詳細は lib.rs / docs/agents/precedent-grounding.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e:   plan-precedent-gate --check <plan.md>(`pass` / `deny: …` / 使い方の誤り。終了コード 0・1・2)
//!
//! スキップ手段: `touch ~/.claude/plan-precedent-gate/skip` または
//! `SKIP_PLAN_PRECEDENT_GATE=1`。

use agent_session::cli::{self, Verdict};
use agent_session::entry::{emit, read_stdin, Call, Spec};
use agent_session::{Agent, Reply};
use plan_precedent_gate::{example_block, judge_precedent, today, trim_newlines};
use std::process::ExitCode;

fn read_plan_file(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if path.is_empty() || !p.is_file() {
        return None;
    }
    let s = std::fs::read(p).ok()?;
    Some(trim_newlines(String::from_utf8_lossy(&s).into_owned()))
}

/// 手動モード(`--check <plan.md>`)。通す = `pass`、指摘あり = `deny: <指摘 + 例文>`、読めない = 使い方の誤り。
fn check_plan(plan_file: &str) -> Verdict {
    let Some(plan) = read_plan_file(plan_file) else {
        return Verdict::Usage(format!("plan file not found: {plan_file}"));
    };
    let problems = judge_precedent(&plan);
    if problems.is_empty() {
        return Verdict::Pass;
    }
    Verdict::Deny(format!(
        "{}\n\n{}",
        problems.join("\n"),
        example_block(&today())
    ))
}

fn cmd_check(args: &[String]) -> ExitCode {
    let verdict = match cli::args(args, 1, 0) {
        Ok(a) => check_plan(a[0]),
        Err(usage) => usage,
    };
    cli::finish(
        "plan-precedent-gate",
        "plan-precedent-gate --check <plan.md>",
        verdict,
    )
}

/// ExitPlanMode の gate は、PreToolUse と PermissionRequest の両方で同じ bin を使う。出力のイベントは
/// 標準入力の `hook_event_name` で決まる(PermissionRequest 以外は PreToolUse)。`--agent` は読まない(Claude 固定)。
const SPEC: Spec = Spec {
    name: "plan-precedent-gate",
    agents: &[Agent::Claude],
    event: None,
};

fn hook() -> ExitCode {
    let raw = read_stdin();
    // 判定は lib の `hook`(ルータと同じ関数)。出力の書式は `Reply::render` が持つ。
    let call = Call {
        agent: Agent::Claude,
        rest: Vec::new(),
    };
    emit(
        &SPEC,
        &call,
        &raw,
        &Reply::from_outcome(plan_precedent_gate::hook(&raw)),
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::manual(&args) {
        Some(rest) => cmd_check(rest),
        None => hook(),
    }
}
