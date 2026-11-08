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
    let mut c = Command::new(env!("CARGO_BIN_EXE_worktree-base-sync"));
    isolate(&mut c)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = project_env {
        c.env("CLAUDE_PROJECT_DIR", p);
    }
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).ok();
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
        "[worktree-base-sync] pristine worktree を origin/main へ 1 コミット fast-forward しました("
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
fn a_recent_stamp_within_ttl_skips_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "ttl");
    // このフック自身が最近 fetch した印(新鮮)がある。origin/main の追跡 ref は古いまま:
    // fetch を省くので behind は 0 のまま何も起きない。
    std::fs::write(wt.join(".git/worktree-base-sync.fetched"), "").unwrap();
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before);
    assert_eq!(out, "");
}

#[test]
fn a_fresh_fetch_head_alone_does_not_skip_fetch() {
    // #1344: FETCH_HEAD は他の fetch(別のブランチの `git fetch origin <branch>` など)でも更新される。
    // それだけで base の fetch を省くと、追跡 ref が古いまま、古い土台から切った worktree に気づけない。
    let tmp = tempfile::tempdir().unwrap();
    let (up, wt) = new_repo_pair(tmp.path(), "fetchhead");
    std::fs::write(wt.join(".git/FETCH_HEAD"), "").unwrap();
    let out = run_env(&wt);
    assert_eq!(head(&wt), head(&up), "fetch して FF した");
    assert!(out.contains("fast-forward"), "{out}");
    // 成功した fetch は印を残し、次の起動では省く。
    assert!(wt.join(".git/worktree-base-sync.fetched").exists());
}

#[test]
fn a_failed_fetch_says_so_when_the_worktree_is_behind_the_local_tracking_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "nofetch");
    // 手元の origin/main を進めてから(HEAD は古い土台のまま)、fetch できない URL にする。
    git(&wt, &["fetch", "-q", "origin", "main"]);
    git(
        &wt,
        &["remote", "set-url", "origin", "/nonexistent-upstream-xyz"],
    );
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before, "fetch できないので動かさない");
    let v: Value = serde_json::from_str(&out).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        ctx.starts_with("[worktree-base-sync] origin/main を fetch できませんでした"),
        "{ctx}"
    );
    assert!(ctx.contains("1 コミット遅れています"), "{ctx}");
    assert!(
        !wt.join(".git/worktree-base-sync.fetched").exists(),
        "失敗は印を残さない"
    );
}

#[test]
fn a_failed_fetch_is_silent_when_nothing_is_known_to_be_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, wt) = new_repo_pair(tmp.path(), "nofetch-quiet");
    // 手元の追跡 ref も HEAD と同じ(遅れの根拠が無い)。fetch できなくても黙る。
    git(
        &wt,
        &["remote", "set-url", "origin", "/nonexistent-upstream-xyz"],
    );
    let before = head(&wt);
    let out = run_env(&wt);
    assert_eq!(head(&wt), before);
    assert_eq!(out, "");
}
