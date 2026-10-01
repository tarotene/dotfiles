//! `git prune-branches` — upstream が `[gone]` のローカルブランチを確認つきで
//! 削除する(`--auto` は `git audit-worktrees --evidence` の `branch` 行を
//! 確認なしで消す)。`scripts/git-prune-branches` の Rust 移植(ADR-0024)。
//!
//! なぜ `--merged=main` でなく `[gone]` + `branch -D` か: squash-merge では
//! PR の commit が main の祖先にならないので `--merged` も `-d` も常に
//! 拒否する。実際の `fetch --prune` が付ける `[gone]`(リモートの branch が
//! 本当に消えた)が安全網。他の worktree で checkout 中の branch は報告のみで
//! 触らない。
//!
//! 環境変数(bash 版と同一):
//!   GIT_PRUNE_BRANCHES_AUDIT_BIN          既定 "git-audit-worktrees"
//!   GIT_PRUNE_BRANCHES_LOG_DIR            既定 "${XDG_STATE_HOME:-$HOME/.local/state}/git-auto-prune"
//!   GIT_PRUNE_BRANCHES_TEST_PRE_ACT_HOOK  テスト専用: 再検証直前に実行するシェル断片
//!
//! 唯一の差: bash 版の `--selftest` は持たない(テストは tests/*.rs)。

use git_prune::{
    audit_bin, err, git_inherit, git_merged, git_stdout, has_exact_line, log_auto_prune, log_dir,
    out, read_answer, rows_of_kind, run_audit, run_pre_act_hook, split_us, Exit,
};

const AUDIT_ENV: &str = "GIT_PRUNE_BRANCHES_AUDIT_BIN";
const LOG_ENV: &str = "GIT_PRUNE_BRANCHES_LOG_DIR";
const HOOK_ENV: &str = "GIT_PRUNE_BRANCHES_TEST_PRE_ACT_HOOK";

const USAGE: &str = "usage: git prune-branches [--dry-run|--auto]

Without options: delete local branches (in the current repository) whose
upstream is [gone] — a real `fetch --prune` marks this once the PR's remote
branch is actually gone (merged, or removed on purpose), never from a local
ancestry heuristic (see this script's own header comment for why `[gone]` +
`branch -D`, not `--merged`). Lists them, confirms once (y/N), then
`git branch -D`s each. A branch checked out in another worktree is reported
separately and never touched — remove that worktree first
(`git prune-worktrees`), then re-run.

--dry-run  list [gone] branches (and worktree-blocked ones), delete nothing
--auto     unattended deletion, for a scheduled timer: consumes
           `git audit-worktrees --evidence`'s `branch` rows (C2/C3
           content-preservation classes, docs/worktree-lifecycle.md)
           across every repository the audit scans, instead of [gone]
           tracking state in the current repository alone. Skips the
           confirmation prompt, re-validates against a fresh `--evidence`
           scan right before deleting (TOCTOU), and appends every deletion
           to $XDG_STATE_HOME/git-auto-prune/log.tsv (the same file
           `git prune-worktrees --auto` writes to). Combine with --dry-run
           to preview.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(c) => c,
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> Exit {
    let (mut dry, mut auto) = (false, false);
    for a in args {
        match a.as_str() {
            "--dry-run" => dry = true,
            "--auto" => auto = true,
            "-h" | "--help" => {
                out(USAGE);
                return Ok(());
            }
            _ => {
                err(USAGE);
                return Err(2);
            }
        }
    }
    if auto {
        return auto_prune_branches(&audit_bin(AUDIT_ENV), dry);
    }
    run_gone(dry)
}

/// 現在の repo のローカル branch のうち upstream が `[gone]` のもの。
/// `(branch, 他の worktree で checkout 中か)`。
fn gone_branches() -> Result<Vec<(String, bool)>, i32> {
    let wl = git_stdout(&["worktree", "list", "--porcelain"])?;
    let checked_out: Vec<&str> = wl
        .split('\n')
        .filter_map(|l| l.strip_prefix("branch refs/heads/"))
        .collect();
    let refs = git_stdout(&[
        "for-each-ref",
        "--format=%(refname:short) %(upstream:track)",
        "refs/heads",
    ])?;
    let mut rows = Vec::new();
    for l in refs.split('\n') {
        let mut it = l.split_whitespace();
        let (first, second) = (it.next(), it.next());
        if let (Some(b), Some("[gone]")) = (first, second) {
            rows.push((b.to_string(), checked_out.contains(&b)));
        }
    }
    Ok(rows)
}

/// 既定の経路: `[gone]` tracking state(この repo のみ)。
fn run_gone(dry: bool) -> Exit {
    git_inherit(&["fetch", "--prune", "origin"])?;
    let rows = gone_branches()?;
    if rows.is_empty() {
        out("prune-branches: no [gone] branches.");
        return Ok(());
    }
    let in_use: Vec<&str> = rows.iter().filter(|r| r.1).map(|r| r.0.as_str()).collect();
    let to_delete: Vec<&str> = rows.iter().filter(|r| !r.1).map(|r| r.0.as_str()).collect();

    if !in_use.is_empty() {
        out("prune-branches: [gone] but checked out in a worktree — remove the worktree first:");
        for b in &in_use {
            out(&format!("  {b}"));
        }
    }
    if to_delete.is_empty() {
        out("prune-branches: nothing left to delete.");
        return Ok(());
    }

    out("prune-branches: will delete these [gone] branches:");
    for b in &to_delete {
        out(&format!("  {b}"));
    }

    if dry {
        out("(--dry-run: 削除はしていません)");
        return Ok(());
    }

    // 改行なしの EOF でも部分入力は ans に入る(`read ... || true`)。
    let (ans, _) = read_answer("Delete? [y/N] ");
    if !matches!(ans.as_str(), "y" | "Y" | "yes" | "YES") {
        out("prune-branches: aborted.");
        return Ok(());
    }

    for b in to_delete {
        git_inherit(&["branch", "-D", "--", b])?;
    }
    Ok(())
}

fn render_evidence_branches(rows: &[&str]) {
    let mut count = 0;
    for row in rows {
        let f = split_us(row, 7);
        let (repo, branch, sha, ev) = (&f[2], &f[4], &f[5], &f[6]);
        if branch.is_empty() {
            continue;
        }
        count += 1;
        out(&format!(
            "evidence branch: repo={repo} branch={branch} sha={sha} evidence={ev}"
        ));
    }
    if count != 0 {
        out(&format!("total: {count} evidence-backed branch(es)"));
    }
}

/// `--auto`: audit の `branch` 行(C2/C3)を確認なしで削除する。削除直前に
/// 新しい `--evidence` スキャンで行が完全一致するか再検証(TOCTOU)。
fn auto_prune_branches(bin: &str, dry: bool) -> Exit {
    let log_dir = log_dir(LOG_ENV)?;
    let listing_raw = run_audit(bin, "--evidence", false)?;
    let listing = rows_of_kind(&listing_raw, "branch");
    if listing.is_empty() {
        out("auto: 削除対象の branch はありません。");
        return Ok(());
    }
    render_evidence_branches(&listing);
    if dry {
        out("(--dry-run: 削除はしていません)");
        return Ok(());
    }

    run_pre_act_hook(HOOK_ENV)?;
    let fresh_raw = run_audit(bin, "--evidence", false)?;
    let fresh = rows_of_kind(&fresh_raw, "branch");

    let (mut removed, mut skipped) = (0u32, 0u32);
    for row in &listing {
        let f = split_us(row, 7);
        let (kind, common, repo, path, branch, sha, ev) =
            (&f[0], &f[1], &f[2], &f[3], &f[4], &f[5], &f[6]);
        if branch.is_empty() {
            continue;
        }
        let line = format!("{kind}\t{common}\t{repo}\t{path}\t{branch}\t{sha}\t{ev}");
        if !has_exact_line(&fresh, &line) {
            err(&format!("skip(状態が変化したため見送り): {branch}"));
            skipped += 1;
            continue;
        }
        let gd = format!("--git-dir={common}");
        let (ok, e) = git_merged(&[&gd, "branch", "-D", "--", branch]);
        if ok {
            out(&format!("removed({ev}): {branch}"));
            // 列: ts, branch, common, repo, path(branch 行では常に空), branch, sha, ev
            log_auto_prune(&log_dir, "branch", &[common, repo, "", branch, sha, ev])?;
            removed += 1;
        } else {
            err(&format!("skip(git が拒否): {branch}: {e}"));
            skipped += 1;
        }
    }

    out(&format!("auto: removed={removed} skipped={skipped}"));
    Ok(())
}
