//! 統合テストの共通部品(#413)。
//!
//! 対象バイナリは既定で cargo の bin。`WRAPUP_STOP_GATE_ORACLE` /
//! `WRAPUP_SESSION_START_ORACLE` に bash 版スクリプトのパスを入れると
//! `bash <path>` に差し替わる(移植手順の段 2: 同じケースを bash に向けて
//! 先に緑にする、docs/rust-migration.md)。

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// 1 ケース分の隔離環境(実 $HOME・実 state を汚さない)。
pub struct Env {
    pub tmp: tempfile::TempDir,
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Run {
    fn from(o: Output) -> Self {
        Run {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
}

fn oracle(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(|p| std::fs::canonicalize(p).expect("oracle path"))
}

/// bash 版に向けているか。
pub fn is_oracle() -> bool {
    oracle("WRAPUP_STOP_GATE_ORACLE").is_some()
}

/// gate 自身のパス(指示文に出る `self_path`)。
pub fn gate_path() -> PathBuf {
    oracle("WRAPUP_STOP_GATE_ORACLE")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wrapup-stop-gate")))
}

/// session-start のパス。
pub fn session_start_path() -> PathBuf {
    oracle("WRAPUP_SESSION_START_ORACLE")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wrapup-session-start")))
}

/// session-start の指示文が指す gate のパス(自分と同じディレクトリ)。
pub fn session_start_gate_path() -> PathBuf {
    let ss = session_start_path();
    let name = if oracle("WRAPUP_SESSION_START_ORACLE").is_some() {
        "wrapup-stop-gate.sh"
    } else {
        "wrapup-stop-gate"
    };
    ss.parent().unwrap().join(name)
}

fn command_for(path: &Path, is_bash: bool) -> Command {
    if is_bash {
        let mut c = Command::new("bash");
        c.arg(path);
        c
    } else {
        Command::new(path)
    }
}

pub const STUB_GH: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >>"$WRAPUP_GH_ARGS_LOG" 2>/dev/null || true
if [[ "${WRAPUP_STUB_FAIL:-0}" == "1" ]]; then
  echo 'GraphQL: API rate limit already exceeded' >&2
  exit 1
fi
if [[ "${WRAPUP_STUB_DUP:-0}" == "1" ]]; then
  echo '[{"number":42,"title":"dup title"}]'
else
  echo '[]'
fi
"#;

pub fn write_exec(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// remote 付き(または無し)の git repo を作る。
pub fn repo(dir: &Path, remote: Option<&str>) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q"]);
    if let Some(r) = remote {
        git(dir, &["remote", "add", "origin", r]);
    }
    dir.to_path_buf()
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let e = Env { tmp };
        std::fs::create_dir_all(e.home()).unwrap();
        write_exec(&e.bin_dir().join("gh"), STUB_GH);
        std::fs::write(e.gh_log(), "").unwrap();
        e
    }

    pub fn path(&self) -> &Path {
        self.tmp.path()
    }
    pub fn home(&self) -> PathBuf {
        self.path().join("home")
    }
    pub fn state(&self) -> PathBuf {
        self.path().join("state")
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.path().join("bin")
    }
    pub fn gh_log(&self) -> PathBuf {
        self.path().join("gh-args.log")
    }
    pub fn stamp_dir(&self) -> PathBuf {
        self.path().join("feedback-stamp")
    }
    pub fn memory_dir(&self) -> PathBuf {
        self.path().join("feedback-memory")
    }
    pub fn stub_path(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir().display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// 期待値計算用: bash の `path_slug`。
    pub fn path_slug(p: &Path) -> String {
        p.to_string_lossy().replace(['/', '.'], "-")
    }
    pub fn inbox_for_slug(&self, slug: &str) -> PathBuf {
        self.state()
            .join("claude/wrapup")
            .join(format!("{slug}.jsonl"))
    }
    pub fn legacy_inbox(&self, project: &Path) -> PathBuf {
        self.inbox_for_slug(&Self::path_slug(project))
    }

    fn base(&self, mut c: Command) -> Command {
        c.env("HOME", self.home())
            .env("WRAPUP_STATE_DIR", self.state())
            .env("WRAPUP_FEEDBACK_STAMP_DIR", self.stamp_dir())
            .env("WRAPUP_FEEDBACK_MEMORY_DIR", self.memory_dir())
            .env("WRAPUP_GH_ARGS_LOG", self.gh_log())
            .env("PATH", self.stub_path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("XDG_STATE_HOME")
            .env_remove("ATTRIBUTION_AGENT_NAME")
            .env_remove("ATTRIBUTION_AGENT_URL")
            .env_remove("WRAPUP_VERDICT_ESCALATE_BIN")
            .env_remove("WRAPUP_STUB_DUP")
            .env_remove("WRAPUP_STUB_FAIL");
        c
    }

    pub fn gate(&self) -> Command {
        self.base(command_for(&gate_path(), is_oracle()))
    }

    pub fn session_start(&self) -> Command {
        self.base(command_for(
            &session_start_path(),
            oracle("WRAPUP_SESSION_START_ORACLE").is_some(),
        ))
    }
}

pub fn run(mut c: Command, stdin: &str) -> Run {
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap().into()
}

pub fn run_args(mut c: Command, args: &[&str]) -> Run {
    c.args(args);
    run(c, "")
}

pub fn lines(p: &Path) -> usize {
    std::fs::read(p)
        .map(|b| b.iter().filter(|&&c| c == b'\n').count())
        .unwrap_or(0)
}

pub fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

pub fn hook_input(cwd: &Path) -> String {
    format!(r#"{{"cwd":"{}","stop_hook_active":false}}"#, cwd.display())
}
