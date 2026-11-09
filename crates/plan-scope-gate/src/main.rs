//! エントリポイント。詳細は lib.rs / docs/agents/requirements-inventory.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e / 提出前の自走:
//!               plan-scope-gate --check <plan.md> [<issue-ref>]
//!               (節内整合性は常に検査する。issue-ref を渡すと、参照 Issue の子項目との照合も行う。
//!               issue-ref は `owner/repo#N`。`#N` と番号だけの `N` は cwd の git remote から
//!               owner/repo を補う。出力は `pass` / `deny: …` / 使い方の誤り。終了コード 0・1・2)
//!
//! スキップ手段: `touch ~/.claude/plan-scope-gate/skip` または
//! `SKIP_PLAN_SCOPE_GATE=1`。

use agent_session::cli::{self, Verdict};
use agent_session::entry::{emit, read_stdin, Call, Spec};
use agent_session::{Agent, Reply};
use plan_scope_gate::{
    judge_inventory, judge_issue, missing_lines, normalize_issue_ref, resolve_owner_repo,
    trim_newlines, try_fetch_children, IssueJudgement, EXAMPLE_BLOCK,
};
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

/// 手動モード(`--check <plan.md> [<issue-ref>]`)。経路B(節内整合性)は常に検査し、経路A(参照 Issue の
/// 子項目との照合)は `issue-ref` を渡したときだけ検査する。通す = `pass`、指摘あり = `deny: <指摘 + 例文>`、
/// 読めない・取得できない = 使い方の誤り。子項目の取得結果と、処分の状態は、情報として標準エラーに出す。
fn check_plan(plan_file: &str, issue_ref: Option<&str>) -> Verdict {
    let Some(plan) = read_plan_file(plan_file) else {
        return Verdict::Usage(format!("plan file not found: {plan_file}"));
    };
    let mut problems = judge_inventory(&plan);
    if let Some(r) = issue_ref {
        let cwd = std::env::current_dir().unwrap_or_default();
        let r = match normalize_issue_ref(r, || resolve_owner_repo(&cwd)) {
            Ok(r) => r,
            Err(why) => return Verdict::Usage(why),
        };
        let children = match try_fetch_children(&r) {
            Ok(c) => c,
            Err(why) => return Verdict::Usage(format!("failed to fetch children for {r}: {why}")),
        };
        eprintln!("children: {}", children.compact());
        let judgement = judge_issue(&r, &children, &plan);
        eprint!("{}", judgement.render());
        if let IssueJudgement::Missing(missing) = &judgement {
            problems.extend(missing_lines(&r, missing));
        }
    }
    if problems.is_empty() {
        return Verdict::Pass;
    }
    Verdict::Deny(format!("{}\n\n{EXAMPLE_BLOCK}", problems.join("\n")))
}

fn cmd_check(args: &[String]) -> ExitCode {
    let verdict = match cli::args(args, 1, 1) {
        Ok(a) => check_plan(a[0], a.get(1).copied()),
        Err(usage) => usage,
    };
    cli::finish(
        "plan-scope-gate",
        "plan-scope-gate --check <plan.md> [<issue-ref>]",
        verdict,
    )
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
    match cli::manual(&args) {
        Some(rest) => cmd_check(rest),
        None => hook(),
    }
}
