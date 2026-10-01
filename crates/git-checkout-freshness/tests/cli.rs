//! 旧 scripts/git-checkout-freshness の `--selftest` の全ケースを、本物の
//! バイナリ + 本物の一時 git repo で検査する。
//!
//! bash ケース → Rust テストの対応:
//!   "FF 成功" (exit 0 / HEAD 一致 / ログに fast-forwarded) -> ff_succeeds_when_clean_on_default_and_behind
//!   "dirty な checkout は無視" (exit 0 / HEAD 不変)          -> dirty_checkout_is_skipped
//!   "feature branch 上は無視" (HEAD 不変 / 理由が出る)       -> feature_branch_is_skipped
//!   "ahead>0 は無視" (HEAD 不変)                            -> ahead_commits_are_skipped
//!   "behind==0 は成功扱い" (exit 0 / HEAD 不変)             -> already_current_is_ok_and_unchanged
//!   "複数パス: 壊れていても継続"                            -> multiple_paths_continue_after_a_bad_one
//! 追加(bash の selftest には無い、本体の分岐):
//!   usage / detached HEAD / origin/HEAD 未設定 / 非 git dir / fetch 失敗

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn run(paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-checkout-freshness"))
        .args(paths)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// upstream を 1 コミット進めた状態の (upstream, checkout)。checkout は
/// default branch のまま behind>0(bash の new_repo_pair)。
fn new_repo_pair(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let up = root.join(format!("{name}-upstream"));
    let co = root.join(format!("{name}-checkout"));
    std::fs::create_dir_all(&up).unwrap();
    git(&up, &["init", "-q", "-b", "main"]);
    git(&up, &["commit", "--allow-empty", "-q", "-m", "base"]);
    git(
        root,
        &["clone", "-q", up.to_str().unwrap(), co.to_str().unwrap()],
    );
    git(
        &co,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(&up, &["commit", "--allow-empty", "-q", "-m", "ahead1"]);
    (up, co)
}

#[test]
fn ff_succeeds_when_clean_on_default_and_behind() {
    let t = tempfile::tempdir().unwrap();
    let (up, co) = new_repo_pair(t.path(), "fresh");
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        git(&co, &["rev-parse", "HEAD"]),
        git(&up, &["rev-parse", "HEAD"])
    );
    let err = stderr(&out);
    assert!(err.contains("fast-forwarded"), "{err}");
    assert!(err.contains("(1 commit(s))"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn dirty_checkout_is_skipped() {
    let t = tempfile::tempdir().unwrap();
    let (_up, co) = new_repo_pair(t.path(), "dirty");
    std::fs::write(co.join("untracked.txt"), "x\n").unwrap();
    let before = git(&co, &["rev-parse", "HEAD"]);
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(git(&co, &["rev-parse", "HEAD"]), before);
    assert!(stderr(&out).contains("working tree not clean"));
}

#[test]
fn feature_branch_is_skipped() {
    let t = tempfile::tempdir().unwrap();
    let (_up, co) = new_repo_pair(t.path(), "onbranch");
    git(&co, &["checkout", "-q", "-b", "work"]);
    let before = git(&co, &["rev-parse", "HEAD"]);
    let out = run(&[&co]);
    assert_eq!(git(&co, &["rev-parse", "HEAD"]), before);
    assert!(stderr(&out).contains("not the default branch"));
    assert!(stderr(&out).contains("on 'work', not the default branch 'main'"));
}

#[test]
fn ahead_commits_are_skipped() {
    let t = tempfile::tempdir().unwrap();
    let (_up, co) = new_repo_pair(t.path(), "ahead");
    git(&co, &["commit", "--allow-empty", "-q", "-m", "own-commit"]);
    let before = git(&co, &["rev-parse", "HEAD"]);
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(git(&co, &["rev-parse", "HEAD"]), before);
    assert!(stderr(&out).contains("1 local commit(s) not on origin/main"));
}

#[test]
fn already_current_is_ok_and_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let (_up, co) = new_repo_pair(t.path(), "current");
    git(&co, &["fetch", "--quiet", "origin", "main"]);
    git(&co, &["merge", "--ff-only", "--quiet", "origin/main"]);
    let before = git(&co, &["rev-parse", "HEAD"]);
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(git(&co, &["rev-parse", "HEAD"]), before);
    assert!(stderr(&out).contains("already current with origin/main"));
}

#[test]
fn multiple_paths_continue_after_a_bad_one() {
    let t = tempfile::tempdir().unwrap();
    let (up, good) = new_repo_pair(t.path(), "multigood");
    let missing = t.path().join("does-not-exist");
    let out = run(&[&missing, &good]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        git(&good, &["rev-parse", "HEAD"]),
        git(&up, &["rev-parse", "HEAD"])
    );
    assert!(stderr(&out).contains("SKIP"));
    assert!(stderr(&out).contains("no such directory"));
}

#[test]
fn no_args_is_usage_error_64() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(64));
    assert_eq!(
        stderr(&out),
        "usage: git-checkout-freshness <path> [<path> ...]\n"
    );
}

#[test]
fn detached_head_origin_head_unset_and_non_git_are_skipped() {
    let t = tempfile::tempdir().unwrap();
    let (_up, co) = new_repo_pair(t.path(), "detached");
    git(&co, &["checkout", "-q", "--detach"]);
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stderr(&out).contains("detached HEAD or mid-rebase"));

    let (_up, co) = new_repo_pair(t.path(), "nohead");
    git(
        &co,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    let out = run(&[&co]);
    assert!(stderr(&out).contains("origin/HEAD not set"));

    let plain = t.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_git-checkout-freshness"))
        .arg(&plain)
        .env("GIT_CEILING_DIRECTORIES", t.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(stderr(&out).contains("not a git work tree"));
}

#[test]
fn fetch_failure_is_skipped_with_exit_0() {
    let t = tempfile::tempdir().unwrap();
    let (up, co) = new_repo_pair(t.path(), "nofetch");
    std::fs::rename(&up, t.path().join("moved-away")).unwrap();
    let before = git(&co, &["rev-parse", "HEAD"]);
    let out = run(&[&co]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(git(&co, &["rev-parse", "HEAD"]), before);
    assert!(stderr(&out).contains("fetch failed or timed out"));
}
