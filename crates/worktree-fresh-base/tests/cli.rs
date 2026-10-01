//! bash 版 `worktree-fresh-base.sh --selftest`(6 シナリオ)を、実バイナリ・
//! 実 git repo(upstream + clone の worktree)で再現する。

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// ホストの user/global git 設定から隔離する(bash 版 selftest と同じ #56/#59
/// の教訓)。
fn isolate(c: &mut Command) -> &mut Command {
    c.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("WORKTREE_FRESH_BASE_FETCH_TTL", "600")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    isolate(&mut c)
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
        .args(args);
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn empty_commit(dir: &Path, msg: &str) {
    git(dir, &["commit", "--allow-empty", "-q", "-m", msg]);
}

/// upstream 側で 1 コミット進めてから worktree を切る(behind>0 の初期状態)。
fn new_repo_pair(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let upstream = root.join(format!("{name}-upstream"));
    let worktree = root.join(format!("{name}-worktree"));
    let mut c = Command::new("git");
    isolate(&mut c)
        .args(["init", "-q", "-b", "main"])
        .arg(&upstream);
    assert!(c.status().unwrap().success());
    empty_commit(&upstream, "base");
    let mut c = Command::new("git");
    isolate(&mut c)
        .args(["clone", "-q"])
        .arg(&upstream)
        .arg(&worktree);
    assert!(c.status().unwrap().success());
    git(
        &worktree,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(&worktree, &["checkout", "-q", "-b", "work"]);
    empty_commit(&upstream, "ahead1");
    (upstream, worktree)
}

fn run_hook(stdin: &str, project_env: Option<&Path>) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_worktree-fresh-base"));
    isolate(&mut c)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = project_env {
        c.env("CLAUDE_PROJECT_DIR", p);
    }
    let mut child = c.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap(),
    )
}

fn hook_input(wt: &Path) -> String {
    format!(r#"{{"cwd":"{}"}}"#, wt.display())
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

/// bash 版と同じく `CLAUDE_PROJECT_DIR` を設定して呼ぶ。
fn run_env(wt: &Path) -> String {
    let (out, code) = run_hook(&hook_input(wt), Some(wt));
    assert_eq!(code, 0);
    out
}

#[test]
fn ff_succeeds_when_pristine_and_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let (up, wt) = new_repo_pair(tmp.path(), "pristine");
    let before = head(&wt);
    let out = run_env(&wt);
    let after = head(&wt);
    assert_eq!(after, head(&up), "FF 成功: HEAD が upstream に一致");
    assert!(
        out.contains("fast-forward"),
        "additionalContext が出る: {out}"
    );
    assert_ne!(before, after, "HEAD が動いた");
    let v: Value = serde_json::from_str(&out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "SessionStart");
    let ctx = h["additionalContext"].as_str().unwrap();
    let short_before = &before[..7];
    assert!(ctx.starts_with(
        "[worktree-fresh-base] pristine worktree を origin/main へ 1 コミット fast-forward しました("
    ));
    assert!(ctx.contains(short_before));
    assert!(ctx.ends_with("この更新前の状態を見ている場合があります。"));
}

#[test]
fn dirty_worktree_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "dirty");
    std::fs::write(wt.join("untracked.txt"), "x\n").unwrap();
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "dirty: HEAD 不変");
    assert_eq!(out, "", "dirty: 出力なし");
}

#[test]
fn ahead_of_origin_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "ahead");
    empty_commit(&wt, "own-commit");
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "ahead>0: HEAD 不変");
    assert_eq!(out, "", "ahead>0: 出力なし");
}

#[test]
fn already_current_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "current");
    git(&wt, &["fetch", "--quiet", "origin", "main"]);
    git(&wt, &["merge", "--ff-only", "--quiet", "origin/main"]);
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "behind==0: HEAD 不変");
    assert_eq!(out, "", "behind==0: 出力なし");
}

#[test]
fn detached_head_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "detached");
    git(&wt, &["checkout", "-q", "--detach", "HEAD"]);
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "detached: HEAD 不変");
    assert_eq!(out, "", "detached: 出力なし");
}

#[test]
fn unset_origin_head_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "noorigin");
    git(&wt, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "origin/HEAD 未設定: HEAD 不変");
    assert_eq!(out, "", "origin/HEAD 未設定: 出力なし");
}

// --- bash 版 selftest に無い配線の回帰(追加)---------------------------------

#[test]
fn project_dir_falls_back_to_stdin_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let (up, wt) = new_repo_pair(tmp.path(), "cwdonly");
    let (out, code) = run_hook(&hook_input(&wt), None);
    assert_eq!(code, 0);
    assert!(out.contains("fast-forward"));
    assert_eq!(head(&wt), head(&up));
}

#[test]
fn degenerate_inputs_are_silent_exit_0() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "degen");
    let before = head(&wt);
    for stdin in ["", "not json", r#"{"cwd":"/nonexistent-dir-xyz"}"#, "{}"] {
        let (out, code) = run_hook(stdin, None);
        assert_eq!((out.as_str(), code), ("", 0), "stdin: {stdin}");
    }
    // 不正 JSON でも CLAUDE_PROJECT_DIR があればそれを使う(bash 版は jq を
    // 呼ばない)。
    let (out, _) = run_hook("not json", Some(&wt));
    assert!(out.contains("fast-forward"));
    assert_ne!(head(&wt), before);
}

#[test]
fn fresh_fetch_head_within_ttl_skips_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "ttl");
    // FETCH_HEAD を作って(新鮮)、origin/main の追跡 ref は古いまま:
    // fetch を省くので behind は 0 のまま何も起きない。
    std::fs::write(wt.join(".git/FETCH_HEAD"), "").unwrap();
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before);
    assert_eq!(out, "");
}
