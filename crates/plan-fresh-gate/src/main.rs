//! エントリポイント。詳細は lib.rs / docs/claude/plan-fresh-gate.md。
//!
//! hook として PreToolUse(matcher: ExitPlanMode)から stdin JSON で呼ばれる。
//! スキップ手段: `touch ~/.claude/plan-fresh-gate/skip` または
//! `SKIP_PLAN_FRESH_GATE=1`。

use hook_io::{jqfmt, HookInput, SessionLedger};
use plan_fresh_gate::{git, git_ok, intersect_files, MAX_DENY_DISPLAY};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

/// fetch の打ち切り(旧版の `timeout 15`)。
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

fn gate_dir() -> PathBuf {
    std::env::var_os("CLAUDE_PLAN_FRESH_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/plan-fresh-gate")
        })
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn pass_through(msg: &str) -> ExitCode {
    print!("{}", jqfmt::system_message(msg));
    ExitCode::SUCCESS
}

fn ensure_private_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

fn run() -> ExitCode {
    if !hook_io::proc::command_exists("git") {
        return ExitCode::SUCCESS;
    }
    let dir = gate_dir();
    if dir.join("skip").exists() || std::env::var("SKIP_PLAN_FRESH_GATE").as_deref() == Ok("1") {
        return ExitCode::SUCCESS;
    }

    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
    // 不正 JSON は bash 版(jq 失敗時の既定値)と同じく PreToolUse / unknown に倒す。
    let input = HookInput::parse(&buf).unwrap_or_else(|| HookInput {
        session_id: "unknown".into(),
        ..Default::default()
    });
    let Some(project) = input.project_dir() else {
        return ExitCode::SUCCESS;
    };
    let p = project.as_path();
    if !git_ok(p, &["rev-parse", "--is-inside-work-tree"]) {
        return ExitCode::SUCCESS;
    }

    let branch = git(p, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return ExitCode::SUCCESS; // detached HEAD / rebase 中は触らない
    }
    let Some(base) = hook_io::git::default_branch(p).filter(|b| !b.is_empty()) else {
        return ExitCode::SUCCESS;
    };
    let is_base_branch = branch == base;

    // fetch は TTL なしで常に実行する(ExitPlanMode はセッションに稀なイベント)。
    let mut fetch = Command::new("git");
    fetch
        .arg("-C")
        .arg(p)
        .args(["fetch", "--quiet", "origin", &base]);
    let _ = hook_io::proc::output_with_timeout(&mut fetch, FETCH_TIMEOUT, None);
    let origin_base = format!("origin/{base}");
    if !git_ok(p, &["rev-parse", "--verify", "-q", &origin_base]) {
        return ExitCode::SUCCESS;
    }
    let Some(to_sha) = git(p, &["rev-parse", &origin_base]) else {
        return ExitCode::SUCCESS;
    };

    ensure_private_dir(&dir);
    let ledger = SessionLedger::new(&dir, "denied_sha");
    let sfile = ledger.path(&input.session_id);
    let mut from_sha = String::new();
    if sfile.is_file() {
        from_sha = ledger
            .records(&input.session_id)
            .last()
            .cloned()
            .unwrap_or_default();
        if !from_sha.is_empty() && !git_ok(p, &["merge-base", "--is-ancestor", &from_sha, &to_sha])
        {
            from_sha.clear(); // 記録 SHA が origin/<base> の祖先でなくなっている → 破棄
        }
    }
    if from_sha.is_empty() {
        let Some(mb) = git(p, &["merge-base", "HEAD", &origin_base]) else {
            return ExitCode::SUCCESS;
        };
        from_sha = mb;
    }

    if from_sha == to_sha {
        let _ = std::fs::remove_file(&sfile);
        return ExitCode::SUCCESS; // 追いつき済み(または前回の deny が確認済み)
    }

    let Some(changed) = git(p, &["diff", "--name-only", &from_sha, &to_sha, "--"]) else {
        return ExitCode::SUCCESS;
    };
    if changed.is_empty() {
        return ExitCode::SUCCESS; // 実質的な差分なし
    }

    // --- 移動(pristine のときだけ) ---
    let mut did_ff = false;
    let (mut before, mut after) = (String::new(), String::new());
    if !is_base_branch
        && git(p, &["status", "--porcelain"])
            .unwrap_or_default()
            .is_empty()
    {
        let range = format!("HEAD...{origin_base}");
        let ab = git(p, &["rev-list", "--left-right", "--count", &range]).unwrap_or_default();
        let ahead = ab.split('\t').next().unwrap_or("");
        let behind = ab.rsplit('\t').next().unwrap_or("");
        let behind_n = (!behind.is_empty() && behind.bytes().all(|b| b.is_ascii_digit()))
            .then(|| behind.parse::<u64>().ok())
            .flatten();
        if ahead == "0" && behind_n.is_some_and(|n| n > 0) {
            before = git(p, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
            if git_ok(p, &["merge", "--ff-only", "--quiet", &origin_base]) {
                after = git(p, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
                did_ff = !before.is_empty() && !after.is_empty() && before != after;
            }
        }
    }

    // --- プラン本文(tool_input.plan → planFilePath → 最新 ~/.claude/plans/*.md)
    let plan = hook_io::plan::plan_text(&input)
        .map(|mut s| {
            while s.ends_with('\n') {
                s.pop();
            }
            s
        })
        .unwrap_or_default();
    if plan.is_empty() {
        return pass_through(&format!(
            "[plan-fresh-gate] origin/{base} が進行していますが({}..{})、プラン本文を取得できず交差判定をスキップしました。",
            short(&from_sha),
            short(&to_sha)
        ));
    }

    let intersecting = intersect_files(&changed, &plan);
    if intersecting.is_empty() {
        let note = if did_ff {
            format!("[plan-fresh-gate] origin/{base} へ {before} -> {after} まで fast-forward しました。プラン参照ファイルとの交差はありません。")
        } else {
            format!(
                "[plan-fresh-gate] origin/{base} が進行しています({}..{})。プラン参照ファイルとの交差はありません。",
                short(&from_sha),
                short(&to_sha)
            )
        };
        return pass_through(&note);
    }

    // --- 交差あり: deny ---
    let mut lines: Vec<String> = vec![
        format!("プラン作成後に origin/{base} が進行し、以下のプラン参照ファイルが変更されました。再読してプランが依然成立するか確認し、必要なら修正のうえ再度 ExitPlanMode してください。"),
        String::new(),
    ];
    let total = intersecting.len();
    let shown: Vec<&String> = intersecting.iter().take(MAX_DENY_DISPLAY).collect();
    lines.extend(shown.iter().map(|f| format!("  - {f}")));
    if total > MAX_DENY_DISPLAY {
        lines.push(format!("  ...他 {} 件", total - MAX_DENY_DISPLAY));
    }
    lines.push(String::new());
    lines.push("diffstat:".into());
    // bash 版はパス一覧を引用符なしで展開していた(`-- $(printf '%s\n' "$shown")`)
    // ため、空白を含むパスは語分割されていた。同じ分割を再現する。
    let mut stat_args: Vec<&str> = vec!["diff", "--stat", &from_sha, &to_sha, "--"];
    stat_args.extend(
        shown
            .iter()
            .flat_map(|f| f.split([' ', '\t', '\n']))
            .filter(|w| !w.is_empty()),
    );
    lines.push(git(p, &stat_args).unwrap_or_default());
    lines.push(String::new());
    if did_ff {
        lines.push(format!(
            "worktree は origin/{base} へ fast-forward 済みです({before} -> {after})。"
        ));
    } else {
        lines.push(format!("worktree は動かしていません(作業中のコミットがある、または main 直上のため)。origin/{base} 側の内容は `git show origin/{base}:<path>` または `git diff HEAD...origin/{base} -- <path>` で確認してください。rebase は人間に依頼してください。"));
    }

    // 台帳は「最後に deny した SHA」だけを持てばよいので、置き換えてから追記する
    // (bash 版の改行なし上書きファイルに追記して 1 行が壊れるのも避ける)。
    let _ = std::fs::remove_file(&sfile);
    let _ = ledger.append(&input.session_id, &to_sha);

    print!(
        "{}",
        jqfmt::deny_for_event(&input.hook_event_name, &lines.join("\n"))
    );
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    run()
}
