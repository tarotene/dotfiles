//! エントリポイント。詳細は lib.rs / docs/claude/scope-inventory.md。
//!
//! 使い方:
//!   hook として: PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる
//!   手動 e2e:   plan-scope-gate --check <plan.md> <issue-ref>
//!               (issue-ref は `#N` または `owner/repo#N`。bare の場合は cwd の
//!               git remote から owner/repo を解決する)
//!   提出前の自走(経路Bのみ、gh 不使用):
//!               plan-scope-gate --check-plan <plan.md>
//!
//! スキップ手段: `touch ~/.claude/plan-scope-gate/skip` または
//! `SKIP_PLAN_SCOPE_GATE=1`。

use hook_io::jqfmt;
use plan_scope_gate::{
    deny_message, extract_issue_refs, extract_user_text, fetch_children, judge_inventory,
    judge_issue, missing_lines, resolve_owner_repo, trim_newlines, IssueJudgement, EXAMPLE_BLOCK,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn gate_dir() -> PathBuf {
    std::env::var_os("CLAUDE_PLAN_SCOPE_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/plan-scope-gate")
        })
}

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
    let mut r = r.to_string();
    if !r.contains('/') {
        let cwd = std::env::current_dir().unwrap_or_default();
        let owner_repo = resolve_owner_repo(&cwd);
        if owner_repo.is_empty() {
            eprintln!("cwd is not inside a GitHub-remote repo; pass owner/repo#N explicitly");
            return ExitCode::from(1);
        }
        r = format!("{owner_repo}{r}");
    }
    let Some(children) = fetch_children(&r) else {
        eprintln!("failed to fetch children for {r} (gh 不在・未認証・ネットワーク不通のいずれか)");
        return ExitCode::from(1);
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

fn hook() -> ExitCode {
    // ADR-0005: バイナリの存在でゲートする(gh 不在のマシンでは経路Bも含め沈黙)。
    if !hook_io::proc::command_exists("gh") {
        return ExitCode::SUCCESS;
    }
    if gate_dir().join("skip").exists()
        || std::env::var("SKIP_PLAN_SCOPE_GATE").as_deref() == Ok("1")
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
    let plan = trim_newlines(plan);
    let cwd = std::env::current_dir().unwrap_or_default();

    let mut deny_lines: Vec<String> = Vec::new();

    // --- 経路A: Issue 起点の実カバレッジ ---
    let transcript = serde_json::from_str::<serde_json::Value>(&buf)
        .ok()
        .and_then(|v| v.get("transcript_path")?.as_str().map(str::to_string))
        .unwrap_or_default();
    if !transcript.is_empty() && Path::new(&transcript).is_file() {
        let raw = std::fs::read(&transcript).unwrap_or_default();
        let user_text = trim_newlines(extract_user_text(&String::from_utf8_lossy(&raw)));
        let owner_repo = resolve_owner_repo(&cwd);
        for r in extract_issue_refs(&user_text, &owner_repo) {
            let Some(children) = fetch_children(&r) else {
                continue;
            };
            if let IssueJudgement::Missing(items) = judge_issue(&r, &children, &plan) {
                deny_lines.extend(missing_lines(&r, &items));
            }
        }
    }

    // --- 経路B: インベントリ内整合性 ---
    deny_lines.extend(judge_inventory(&plan));

    if deny_lines.is_empty() {
        return ExitCode::SUCCESS;
    }
    print!(
        "{}",
        jqfmt::deny_for_event(&input.hook_event_name, &deny_message(&deny_lines))
    );
    ExitCode::SUCCESS
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
