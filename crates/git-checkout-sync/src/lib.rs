//! git-checkout-sync — herdr が worktree を fork する元の *親* checkout を
//! origin/<base> へ fast-forward し続ける(旧 scripts/git-checkout-freshness、
//! 設計根拠は docs/agents/git-checkout-sync.md)。
//!
//! 安全条件(すべて AND、path ごとに判定。fetch は最後の方): 存在する
//! git work tree / 現在 branch が非空 / 現在 branch が default branch /
//! `status --porcelain` が空 / ahead == 0 / behind > 0。満たしたときだけ
//! `merge --ff-only --quiet origin/<base>`(reset --hard は使わない)。
//!
//! 縮退: 各 path は独立に fail-open。skip 理由は stderr に出すだけで、
//! usage エラー以外では常に exit 0。

use git_repository::ok as git_ok;
use git_repository::out as git_out;
use std::path::Path;
#[cfg(test)]
use std::process::{Command, Stdio};
use std::time::Duration;

/// fetch の打ち切り秒数(旧版の `timeout 15`)。
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

pub use process_control::status_with_group_timeout as run_with_timeout;

fn skip(project: &str, why: &str) -> bool {
    eprintln!("SKIP {project}: {why}");
    false
}

/// 1 repo を処理する。true = 更新済み or 何もすることなし、false = skip/失敗(致命ではない)。
pub fn process_one(project: &str) -> bool {
    process_one_with(project, FETCH_TIMEOUT)
}

pub fn process_one_with(project: &str, fetch_timeout: Duration) -> bool {
    if project.is_empty() || !Path::new(project).is_dir() {
        return skip(project, "no such directory");
    }
    if !git_ok(project, &["rev-parse", "--is-inside-work-tree"]) {
        return skip(project, "not a git work tree");
    }

    let branch = git_out(project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return skip(project, "detached HEAD or mid-rebase");
    }

    let Some(base) = git_repository::default_branch(Path::new(project)) else {
        return skip(project, "origin/HEAD not set");
    };
    if branch != base {
        return skip(
            project,
            &format!("on '{branch}', not the default branch '{base}'"),
        );
    }

    if !git_out(project, &["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty()
    {
        return skip(project, "working tree not clean");
    }

    if !git_repository::fetch_inherited_stdout(Path::new(project), &base, fetch_timeout) {
        return skip(project, "fetch failed or timed out");
    }
    let origin_ref = format!("origin/{base}");
    if !git_ok(project, &["rev-parse", "--verify", "-q", &origin_ref]) {
        return skip(
            project,
            &format!("origin/{base} not resolvable after fetch"),
        );
    }

    let range = format!("HEAD...{origin_ref}");
    let Some(ab) = git_out(project, &["rev-list", "--left-right", "--count", &range]) else {
        return skip(project, "rev-list failed");
    };
    let ahead = ab.split('\t').next().unwrap_or("");
    let behind = ab.rsplit('\t').next().unwrap_or("");
    if ahead != "0" {
        return skip(
            project,
            &format!("{ahead} local commit(s) not on origin/{base}"),
        );
    }
    let behind_n = behind
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| behind.parse::<u64>().ok())
        .flatten()
        .unwrap_or(0);
    if behind.is_empty() || behind_n == 0 {
        eprintln!("OK   {project}: already current with origin/{base}");
        return true;
    }

    let before = git_out(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    if !git_ok(project, &["merge", "--ff-only", "--quiet", &origin_ref]) {
        return skip(
            project,
            "fast-forward merge failed (history moved under us?)",
        );
    }
    let after = git_out(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    eprintln!("OK   {project}: fast-forwarded {before} -> {after} ({behind} commit(s))");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn timeout_kills_slow_command() {
        let t = Instant::now();
        let ok = run_with_timeout(
            Command::new("sleep")
                .arg("10")
                .stdin(Stdio::null())
                .stdout(Stdio::null()),
            Duration::from_millis(300),
        );
        assert!(!ok);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn fast_success_and_failure() {
        assert!(run_with_timeout(
            &mut Command::new("true"),
            Duration::from_secs(5)
        ));
        assert!(!run_with_timeout(
            &mut Command::new("false"),
            Duration::from_secs(5)
        ));
        assert!(!run_with_timeout(
            &mut Command::new("/nonexistent/cmd"),
            Duration::from_secs(5)
        ));
    }
}
