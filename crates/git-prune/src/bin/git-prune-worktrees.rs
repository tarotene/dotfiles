//! `git prune-worktrees` — `git audit-worktrees` が報告する stale な
//! worktree(prunable / orphaned)を削除する。`scripts/git-prune-worktrees`
//! の Rust 移植(ADR-0024)。外部挙動(フラグ・メッセージ・終了コード・
//! log.tsv・環境変数)は bash 版と同一:
//!
//!   GIT_PRUNE_WORKTREES_AUDIT_BIN          既定 "git-audit-worktrees"
//!   GIT_PRUNE_WORKTREES_LOG_DIR            既定 "${XDG_STATE_HOME:-$HOME/.local/state}/git-auto-prune"
//!   GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK  テスト専用: 再検証直前に実行するシェル断片
//!
//! 唯一の差: bash 版の `--selftest` は持たない(テストは tests/*.rs)。

use git_prune::{
    audit_bin, err, git_inherit, git_merged, has_exact_line, lines_of, log_auto_prune, log_dir,
    out, read_answer, read_tab_ws, rows_of_kind, run_audit, run_pre_act_hook, sorted_unique,
    split_us, Exit,
};
use std::io::IsTerminal;
use std::process::{Command, Stdio};

const AUDIT_ENV: &str = "GIT_PRUNE_WORKTREES_AUDIT_BIN";
const LOG_ENV: &str = "GIT_PRUNE_WORKTREES_LOG_DIR";
const HOOK_ENV: &str = "GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK";

const USAGE: &str = "usage: git prune-worktrees [--dry-run|--yes|--force|--auto]

Lists every stale worktree finding from `git audit-worktrees` — both
prunable (registration only, checkout already gone) and orphaned (checkout
still present but abandoned) — confirms once, re-validates right before
acting, then removes each:
  prunable  git worktree prune --expire=now (metadata only; branch kept)
  orphaned  git worktree remove (no --force unless --force is given —
            except a worktree containing a submodule, which Git refuses
            unconditionally: after our own stricter clean check passes,
            that case retries with --force regardless)

Branches are left alone; run `git prune-branches` afterward.

--dry-run  list candidates only, remove nothing
--yes      skip the confirmation prompt (required for non-interactive use)
--force    also override Git's own delete refusal for orphaned checkouts
           (e.g. dirty content) — re-validation still applies and can skip

--auto     unattended deletion, for a scheduled timer: consumes
           `git audit-worktrees --evidence` (C1/C2/C3 content-preservation
           classes, docs/worktree-lifecycle.md) instead of the
           upstream-tracking prunable/orphaned classes above, and skips the
           confirmation prompt entirely (no --yes needed). Still re-
           validates right before acting. Plain `git worktree remove` only
           (no submodule --force retry) — a worktree Git refuses to remove
           is skipped and reported, not overridden. Every deletion is
           appended to $XDG_STATE_HOME/git-auto-prune/log.tsv (repo, path,
           sha, evidence class) so a mistaken class can be traced back and
           the content recovered (C2's sha is on origin's default branch
           already; C3's sha is recoverable via refs/pull/<N>/head even
           after local deletion). Combine with --dry-run to preview.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(c) => c,
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> Exit {
    let (mut dry, mut assume_yes, mut force, mut auto) = (false, false, false, false);
    for a in args {
        match a.as_str() {
            "--dry-run" => dry = true,
            "--yes" => assume_yes = true,
            "--force" => force = true,
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
    let bin = audit_bin(AUDIT_ENV);
    if auto {
        return auto_prune(&bin, dry);
    }
    interactive(&bin, dry, assume_yes, force)
}

/// `git -C <repo> worktree remove [--force] -- <path>`(出力は合流して返す)。
fn wt_remove(repo: &str, path: &str, force: bool) -> (bool, String) {
    if force {
        git_merged(&["-C", repo, "worktree", "remove", "--force", "--", path])
    } else {
        git_merged(&["-C", repo, "worktree", "remove", "--", path])
    }
}

/// worktree の index に gitlink(mode 160000)があるか。git の出力は最後まで
/// 読み切ってから判定する(bash 版の SIGPIPE 回避と同じ要件)。
fn has_submodule(path: &str) -> bool {
    let o = Command::new("git")
        .args(["-C", path, "ls-files", "-s"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match o {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .split('\n')
            .any(|l| l.split_whitespace().next() == Some("160000")),
        _ => false,
    }
}

/// submodule の作業ツリーまで含めて未コミット変更が無いか
/// (`--ignore-submodules=none`)。git がエラーを出した場合も出力が空でない
/// ので false(bash 版の `2>&1` 判定と同じ)。
fn strictly_clean(path: &str) -> bool {
    let o = Command::new("git")
        .args([
            "-C",
            path,
            "status",
            "--porcelain",
            "--ignore-submodules=none",
        ])
        .stdin(Stdio::null())
        .output();
    match o {
        Ok(o) => o.stdout.is_empty() && o.stderr.is_empty(),
        Err(_) => false,
    }
}

fn interactive(bin: &str, dry: bool, assume_yes: bool, force: bool) -> Exit {
    let findings = run_audit(bin, "--porcelain", true)?;
    let (prunable, orphaned) = classify(&findings);
    if prunable.is_empty() && orphaned.is_empty() {
        out("削除対象の worktree はありません。");
        return Ok(());
    }
    render(&prunable, &orphaned);

    if dry {
        out("(--dry-run: 削除はしていません)");
        return Ok(());
    }

    if !assume_yes {
        if !std::io::stdin().is_terminal() {
            err("非対話実行では --yes が必要です。");
            return Err(2);
        }
        let (ans, terminated) = read_answer("上記を削除しますか? [y/N] ");
        if !terminated {
            // 改行なしの EOF は `read` が非 0 → set -e
            return Err(1);
        }
        if ans != "y" && ans != "Y" {
            return Err(2);
        }
    }

    if !prunable.is_empty() {
        prune_prunable(&prunable)?;
    }

    let (mut removed, mut skipped) = (0u32, 0u32);
    if !orphaned.is_empty() {
        // 再検証: 一覧表示・確認の間に状態が変わっていないか、削除に取り掛かる
        // 直前でもう一度スキャンし直す(TOCTOU 対策)。
        run_pre_act_hook(HOOK_ENV)?;
        // 再スキャンの audit 失敗は bash 版でも無視される(入れ子の `$(...)`)。
        // 空 = 全件「状態が変化」で見送り。
        let fresh_raw = run_audit(bin, "--porcelain", true).unwrap_or_default();
        let fresh = classify(&fresh_raw).1.join("\n");

        for row in &orphaned {
            let f = read_tab_ws(row, 4);
            let (repo, path, branch) = (&f[0], &f[1], &f[2]);
            if path.is_empty() {
                continue;
            }
            // 部分文字列一致(grep -qF)。bash 版と同じ。
            if !fresh.contains(path.as_str()) {
                err(&format!("skip(状態が変化したため見送り): {path}"));
                skipped += 1;
                continue;
            }
            if force {
                // --force: git 自身の削除拒否のみ押し切る。
                let (ok, e) = wt_remove(repo, path, true);
                if ok {
                    out(&format!("removed(--force): {path} ({branch})"));
                    removed += 1;
                } else {
                    err(&format!("skip(git が拒否): {path}: {e}"));
                    skipped += 1;
                }
                continue;
            }
            let (ok, e) = wt_remove(repo, path, false);
            if ok {
                out(&format!("removed: {path} ({branch})"));
                removed += 1;
            } else if has_submodule(path) && strictly_clean(path) {
                // plain remove は submodule 入りを無条件に拒否する。strictly_clean が
                // --force で素通りする分を先に検査済み。
                let (ok2, e2) = wt_remove(repo, path, true);
                if ok2 {
                    out(&format!(
                        "removed(submodule のため --force): {path} ({branch})"
                    ));
                    removed += 1;
                } else {
                    err(&format!("skip(git が拒否): {path}: {e2}"));
                    skipped += 1;
                }
            } else {
                err(&format!("skip(git が拒否): {path}: {e}"));
                skipped += 1;
            }
        }
    }

    out(&format!("removed={removed} skipped={skipped}"));
    if removed != 0 {
        out("[gone] ブランチが残っていれば git prune-branches で確認・削除してください。");
    }
    Ok(())
}

/// porcelain TSV(common, repo, path, branch, class, reason)を
/// prunable 行(無加工)と orphaned 行("repo\tpath\tbranch\treason")に分ける。
fn classify(findings: &str) -> (Vec<String>, Vec<String>) {
    let (mut prunable, mut orphaned) = (Vec::new(), Vec::new());
    for l in lines_of(findings) {
        let f: Vec<&str> = l.split('\t').collect();
        let get = |i: usize| f.get(i).copied().unwrap_or("");
        match get(4) {
            "prunable" => prunable.push(l.to_string()),
            "orphaned" => orphaned.push(format!("{}\t{}\t{}\t{}", get(1), get(2), get(3), get(5))),
            _ => {}
        }
    }
    (prunable, orphaned)
}

fn detached(branch: &str) -> &str {
    if branch.is_empty() {
        "(detached)"
    } else {
        branch
    }
}

fn render(prunable: &[String], orphaned: &[String]) {
    let mut count = 0;
    for row in prunable {
        let f = split_us(row, 6);
        let (repo, path, branch, reason) = (&f[1], &f[2], &f[3], &f[5]);
        if path.is_empty() {
            continue;
        }
        count += 1;
        out(&format!(
            "prunable worktree: repo={repo} path={path} branch={} reason={reason}",
            detached(branch)
        ));
    }
    for row in orphaned {
        let f = split_us(row, 4);
        let (repo, path, branch, reason) = (&f[0], &f[1], &f[2], &f[3]);
        if path.is_empty() {
            continue;
        }
        count += 1;
        out(&format!(
            "orphaned worktree: repo={repo} path={path} branch={branch} reason={reason}"
        ));
    }
    if count != 0 {
        out(&format!("total: {count} stale worktree(s)"));
    }
}

/// common dir ごとに 1 回 `git worktree prune --verbose --expire=now`。
fn prune_prunable(prunable: &[String]) -> Exit {
    let commons = sorted_unique(
        prunable
            .iter()
            .map(|r| r.split('\t').next().unwrap_or("").to_string()),
    );
    for common in commons {
        prune_common(&common)?;
    }
    Ok(())
}

fn prune_common(common: &str) -> Exit {
    let gd = format!("--git-dir={common}");
    git_inherit(&[&gd, "worktree", "prune", "--verbose", "--expire=now"])
}

fn render_evidence(rows: &[&str]) {
    let mut count = 0;
    for row in rows {
        let f = split_us(row, 7);
        let (repo, path, branch, sha, ev) = (&f[2], &f[3], &f[4], &f[5], &f[6]);
        if path.is_empty() {
            continue;
        }
        count += 1;
        out(&format!(
            "evidence worktree: repo={repo} path={path} branch={} sha={sha} evidence={ev}",
            detached(branch)
        ));
    }
    if count != 0 {
        out(&format!("total: {count} evidence-backed worktree(s)"));
    }
}

fn auto_prune(bin: &str, dry: bool) -> Exit {
    let log_dir = log_dir(LOG_ENV)?;
    let listing_raw = run_audit(bin, "--evidence", false)?;
    let listing = rows_of_kind(&listing_raw, "worktree");
    if listing.is_empty() {
        out("auto: 削除対象の worktree はありません。");
        return Ok(());
    }
    render_evidence(&listing);
    if dry {
        out("(--dry-run: 削除はしていません)");
        return Ok(());
    }

    run_pre_act_hook(HOOK_ENV)?;
    let fresh_raw = run_audit(bin, "--evidence", false)?;
    let fresh = rows_of_kind(&fresh_raw, "worktree");

    let (mut removed, mut skipped) = (0u32, 0u32);

    // C1: 登録のみ。common dir 単位で 1 回(git が自分で prunability を再評価
    // するので per-row の再検証は不要)。
    let c1_commons = sorted_unique(listing.iter().filter_map(|r| {
        let f: Vec<&str> = r.split('\t').collect();
        (f.get(6) == Some(&"C1")).then(|| f.get(1).copied().unwrap_or("").to_string())
    }));
    for common in c1_commons {
        prune_common(&common)?;
    }
    for row in &listing {
        let f = split_us(row, 7);
        if !f[3].is_empty() && f[6] == "C1" {
            log_auto_prune(
                &log_dir,
                "worktree",
                &[&f[1], &f[2], &f[3], &f[4], &f[5], &f[6]],
            )?;
            removed += 1;
        }
    }

    // C2/C3: 生きている checkout。fresh スキャンとの完全一致で再検証。
    for row in &listing {
        let f = split_us(row, 7);
        let (kind, common, repo, path, branch, sha, ev) =
            (&f[0], &f[1], &f[2], &f[3], &f[4], &f[5], &f[6]);
        if path.is_empty() || ev == "C1" {
            continue;
        }
        let line = format!("{kind}\t{common}\t{repo}\t{path}\t{branch}\t{sha}\t{ev}");
        if !has_exact_line(&fresh, &line) {
            err(&format!("skip(状態が変化したため見送り): {path}"));
            skipped += 1;
            continue;
        }
        let (ok, e) = wt_remove(repo, path, false);
        if ok {
            out(&format!("removed({ev}): {path} ({})", detached(branch)));
            log_auto_prune(&log_dir, "worktree", &[common, repo, path, branch, sha, ev])?;
            removed += 1;
        } else {
            err(&format!("skip(git が拒否): {path}: {e}"));
            skipped += 1;
        }
    }

    out(&format!("auto: removed={removed} skipped={skipped}"));
    if removed != 0 {
        out("[gone] ブランチが残っていれば git prune-branches --auto で確認・削除してください。");
    }
    Ok(())
}
