//! 統合テストの共通部品(#413)。
//!
//! 流れ(Stop、SessionStart、`--add`、`--retro-*`、`--trial`)は、正本の 4 つの bin
//! (session-wrapup、issue-inbox、session-retrospective、feedback-trial)をまたぐ。`gate()` と
//! `session_start()` は、引数が決まる前の見かけの入口(旧 `wrapup-stop-gate` と同じ使い方)で、
//! 実行の直前(`run_raw` か `routed`)に、最初の引数で正本の bin へ振り分ける。session-wrapup 以外の 3 つは
//! 別 package の bin で `CARGO_BIN_EXE_*` が無いので、同じ target のディレクトリに置かれたものを探し、
//! 無ければ一度だけ `cargo build` で作る(`cargo test --workspace` では、最初から揃っている)。

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Once;

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

/// 引数が決まる前の見かけの入口。`routed` が正本の bin に置き換える。
const GATE: &str = "<gate>";
const SESSION_START: &str = "<session-start>";

const RETRO_FLAGS: [&str; 3] = ["--retro-add", "--retro-close", "--retro-procedure"];
const INBOX_FLAGS: [&str; 8] = [
    "--inbox-path",
    "--migrate",
    "--stamp-feedback-session",
    "--add",
    "--check-dup",
    "--list-open",
    "--set-status",
    "--mark-filed",
];

fn session_wrapup_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_session-wrapup"))
}

/// 同じ target に置かれた、他の package の bin。無ければ一度だけ作る。
pub fn sibling(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    let dir = session_wrapup_bin().parent().unwrap().to_path_buf();
    let path = dir.join(name);
    if !path.is_file() {
        BUILD.call_once(|| {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut c = Command::new(cargo);
            c.args([
                "build",
                "-p",
                "issue-inbox",
                "-p",
                "session-retrospective",
                "-p",
                "feedback-trial",
            ]);
            match dir.file_name().unwrap().to_string_lossy().as_ref() {
                "debug" => {}
                "release" => {
                    c.arg("--release");
                }
                profile => {
                    c.args(["--profile", profile]);
                }
            }
            assert!(c.status().expect("cargo build").success(), "cargo build");
        });
    }
    assert!(path.is_file(), "{} が無い", path.display());
    path
}

/// 見かけの入口の Command を、最初の引数で正本の bin に置き換える(env・cwd・引数は引き継ぐ)。
pub fn routed(c: Command) -> Command {
    let program = c.get_program().to_string_lossy().into_owned();
    if program != GATE && program != SESSION_START {
        return c;
    }
    let mut args: Vec<String> = c
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let first = args.first().cloned().unwrap_or_default();
    let bin = if program == SESSION_START {
        args.insert(0, "start".into());
        session_wrapup_bin()
    } else if first == "--trial" {
        args.remove(0);
        sibling("feedback-trial")
    } else if RETRO_FLAGS.contains(&first.as_str()) {
        sibling("session-retrospective")
    } else if INBOX_FLAGS.contains(&first.as_str()) {
        sibling("issue-inbox")
    } else {
        args.insert(0, "stop".into());
        session_wrapup_bin()
    };
    let mut out = Command::new(bin);
    out.args(args);
    if let Some(dir) = c.get_current_dir() {
        out.current_dir(dir);
    }
    for (k, v) in c.get_envs() {
        match v {
            Some(v) => out.env(k, v),
            None => out.env_remove(k),
        };
    }
    out
}

/// gate の見かけの入口(`Command::new` に渡す)。
pub fn gate_path() -> PathBuf {
    PathBuf::from(GATE)
}

/// session-start の見かけの入口。
pub fn session_start_path() -> PathBuf {
    PathBuf::from(SESSION_START)
}

/// session-start の指示文が指す gate のパス(自分と同じディレクトリの issue-inbox)。
pub fn session_start_gate_path() -> PathBuf {
    session_wrapup_bin().parent().unwrap().join("issue-inbox")
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
            // 実機の vault に書かない: 宣言(環境変数・マーカー)は、テストが明示したときだけ。
            .env_remove("OBSIDIAN_VAULT")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("ATTRIBUTION_AGENT_NAME")
            .env_remove("ATTRIBUTION_AGENT_URL")
            .env_remove("WRAPUP_VERDICT_ESCALATE_BIN")
            .env_remove("WRAPUP_STUB_DUP")
            .env_remove("WRAPUP_STUB_FAIL");
        c
    }

    pub fn gate(&self) -> Command {
        self.base(Command::new(gate_path()))
    }

    pub fn session_start(&self) -> Command {
        self.base(Command::new(session_start_path()))
    }
}

/// 読み替えない、プロセスの出力そのもの。
pub fn run_raw(c: Command, stdin: &str) -> Run {
    let mut c = routed(c);
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap().into()
}

/// 旧い観測形で返す: Stop の block は、標準出力の JSON(`decision: "block"`、終了コード 0)で出るが、
/// このテストの assert の多くは、終了コード 2 と標準エラー(理由 + 改行)で書かれている。
/// 輸送そのものは `run_raw` と、`feedback_memory_check` の末尾が確かめる。
pub fn run(c: Command, stdin: &str) -> Run {
    let raw = run_raw(c, stdin);
    if raw.code != 0 {
        return raw;
    }
    let reason = serde_json::from_str::<serde_json::Value>(&raw.stdout)
        .ok()
        .filter(|v| v["decision"] == "block")
        .and_then(|v| v["reason"].as_str().map(str::to_string));
    match reason {
        Some(reason) => Run {
            code: 2,
            stdout: String::new(),
            stderr: format!("{}{reason}\n", raw.stderr),
        },
        None => raw,
    }
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

pub fn hook_input_sid(cwd: &Path, sid: &str) -> String {
    format!(
        r#"{{"cwd":"{}","session_id":"{sid}","stop_hook_active":false}}"#,
        cwd.display()
    )
}

/// stamp を 1 時間前に戻す(selftest の `touch -d '1 hour ago'`)。
pub fn backdate(p: &Path) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

/// Stop が指示を出す唯一の条件(#N の無い feedback memory)を用意する。セッション `sid` の
/// stamp を作って過去に戻し、その後に更新された `type: feedback` のメモを置く。メモのパスを返す。
pub fn arm_feedback(e: &Env, sid: &str) -> PathBuf {
    let r = run_args(e.gate(), &["--stamp-feedback-session", sid]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    backdate(&e.stamp_dir().join(format!("{sid}.stamp")));
    let mem = e.memory_dir().join("proj/memory");
    std::fs::create_dir_all(&mem).unwrap();
    let f = mem.join("unlinked.md");
    std::fs::write(
        &f,
        "---\nname: unlinked\nmetadata:\n  type: feedback\n---\n\nIssue 番号が無い。\n",
    )
    .unwrap();
    f
}
