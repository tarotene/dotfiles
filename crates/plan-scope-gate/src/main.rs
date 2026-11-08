//! エントリポイント。詳細は lib.rs / docs/agents/requirements-inventory.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e:   plan-scope-gate --check <plan.md> <issue-ref>
//!               (issue-ref は `owner/repo#N`。`#N` と番号だけの `N` は cwd の
//!               git remote から owner/repo を補う。それ以外の書式は案内して落とす)
//!   提出前の自走(経路Bのみ、gh 不使用):
//!               plan-scope-gate --check-plan <plan.md>
//!
//! スキップ手段: `touch ~/.claude/plan-scope-gate/skip` または
//! `SKIP_PLAN_SCOPE_GATE=1`。

use agent_session::entry::{emit, read_stdin, Call, Spec};
use agent_session::{Agent, Reply};
use plan_scope_gate::{
    judge_inventory, judge_issue, normalize_issue_ref, resolve_owner_repo, trim_newlines,
    try_fetch_children, EXAMPLE_BLOCK,
};
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

fn read_plan_file(path: &str) -> Option<String> {
    let p = Path::new(path);
    if path.is_empty() || !p.is_file() {
        return None;
    }
    let s = std::fs::read(p).ok()?;
    Some(trim_newlines(String::from_utf8_lossy(&s).into_owned()))
}

fn cmd_check(plan_file: &str, r: &str) -> ExitCode {
    let Some(plan) = read_plan_file(plan_file) else {
        eprintln!("plan file not found: {plan_file}");
        return ExitCode::from(1);
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let r = match normalize_issue_ref(r, || resolve_owner_repo(&cwd)) {
        Ok(r) => r,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(1);
        }
    };
    let children = match try_fetch_children(&r) {
        Ok(c) => c,
        Err(why) => {
            eprintln!("failed to fetch children for {r}: {why}");
            return ExitCode::from(1);
        }
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "children: {}", children.compact());
    let _ = write!(out, "{}", judge_issue(&r, &children, &plan).render());
    ExitCode::SUCCESS
}

/// 経路B(インベントリ内整合性)だけをプラン単体で検査する自走モード。
fn cmd_check_plan(plan_file: &str) -> ExitCode {
    let Some(plan) = read_plan_file(plan_file) else {
        eprintln!("plan file not found: {plan_file}");
        return ExitCode::from(1);
    };
    let problems = judge_inventory(&plan);
    let mut out = std::io::stdout().lock();
    if problems.is_empty() {
        let _ = writeln!(out, "OK: 要求インベントリの節内整合性の検査を通過しました(経路Aの Issue 照合は --check <plan.md> <issue-ref> で別途確認してください)。");
        return ExitCode::SUCCESS;
    }
    for p in &problems {
        let _ = writeln!(out, "{p}");
    }
    let _ = writeln!(out, "\n{EXAMPLE_BLOCK}");
    ExitCode::from(1)
}

/// ExitPlanMode の gate は、PreToolUse と PermissionRequest の両方で同じ bin を使う。出力のイベントは
/// 標準入力の `hook_event_name` で決まる(PermissionRequest 以外は PreToolUse)。`--agent` は読まない(Claude 固定)。
const SPEC: Spec = Spec {
    name: "plan-scope-gate",
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
        &Reply::from_outcome(plan_scope_gate::hook(&raw)),
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match args.first().map(String::as_str) {
        Some("--check") => cmd_check(arg(1), arg(2)),
        Some("--check-plan") => cmd_check_plan(arg(1)),
        _ => hook(),
    }
}
