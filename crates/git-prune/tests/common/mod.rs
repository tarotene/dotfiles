//! 結合テスト共有ヘルパ。実バイナリ(`CARGO_BIN_EXE_*`)を本物の一時 git repo と
//! スタブ実行ファイル(audit など)に対して走らせる。bash 版 `--selftest` と
//! 同じ構成(スタブ audit、実 git)。実物の git-audit-worktrees は使わない。
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub fn write_exec(path: &Path, script: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).unwrap();
}

/// 失敗してもよい git 呼び出し(終了成否を返す)。ホストの ~/.gitconfig の
/// hooksPath 等に影響されないよう `-c core.hooksPath=/dev/null`。
pub fn git_status(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

pub fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

pub fn git_out(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim_end().to_string()
}

/// bash 版 selftest の repo セットアップ。`main_branch` なら `init -b main`。
pub fn new_repo(dir: &Path, main_branch: bool) {
    fs::create_dir_all(dir).unwrap();
    if main_branch {
        git(dir, &["init", "-qb", "main"]);
    } else {
        git(dir, &["init", "-q"]);
    }
    git(dir, &["config", "core.hooksPath", "/dev/null"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "user.name", "test"]);
    git(dir, &["config", "user.email", "test@example.invalid"]);
    git(dir, &["commit", "--allow-empty", "-qm", "initial"]);
}

pub fn common_dir(repo: &Path) -> String {
    git_out(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
}

pub fn branch_exists(repo: &Path, name: &str) -> bool {
    git_status(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
}

pub fn worktree_registered(common: &str, path: &Path) -> bool {
    let o = Command::new("git")
        .arg(format!("--git-dir={common}"))
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).contains(&format!("worktree {}", path.display()))
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// bash 版が `2>&1` で見ているテキスト相当(stdout の後に stderr)。
    pub fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub fn finish(o: Output) -> Run {
    Run {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// `bin args...` を `cwd` で、stdin に `stdin`(None なら /dev/null)を与えて
/// 走らせる。GIT_PRUNE_* は呼び出し側が `env` で明示する。
pub fn run(bin: &str, args: &[&str], cwd: &Path, env: &[(&str, &str)], stdin: Option<&str>) -> Run {
    use std::io::Write;
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .env_remove("GIT_PRUNE_WORKTREES_AUDIT_BIN")
        .env_remove("GIT_PRUNE_BRANCHES_AUDIT_BIN")
        .env_remove("GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK")
        .env_remove("GIT_PRUNE_BRANCHES_TEST_PRE_ACT_HOOK")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(s) = stdin {
        let mut si = child.stdin.take().unwrap();
        si.write_all(s.as_bytes()).ok();
    }
    finish(child.wait_with_output().unwrap())
}

pub fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}

pub fn pb(p: &Path, rel: &str) -> PathBuf {
    p.join(rel)
}
