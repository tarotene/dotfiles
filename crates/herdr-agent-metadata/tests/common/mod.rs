//! 3 エージェント共通の統合テスト基盤。
//!
//! テスト内の `UnixListener` を herdr の socket に見立て、hook が送った
//! JSON 行を受け取って比較する。`id` の時刻・乱数部と `seq` は非決定なので
//! [`normalize`] で置き換えてから比べる(それ以外はバイト一致)。
//!
//! 対象実装は `HERDR_AGENT_METADATA_ORACLE` で切り替える:
//! - `bash`: 移植元の `config/<agent>/hooks/herdr-<agent>-metadata.sh` を `sh` で起動
//! - `rust`: `herdr-agent-metadata --agent <agent>`
//!
//! 未設定時は [`DEFAULT_ORACLE`]。

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const DEFAULT_ORACLE: &str = "bash";

pub const PANE: &str = "pane-1";

/// Python の `json.dumps`(ensure_ascii)で表した各ラベル。
pub const PLAN: &str = r#""\u25c7 plan""#;
pub const DEFAULT: &str = r#""\u25c6 default""#;
pub const ACCEPT: &str = r#""\u2713 accept""#;
pub const BYPASS: &str = r#""\u25b2 bypass""#;
pub const NULL: &str = "null";

pub struct Harness {
    pub tmp: tempfile::TempDir,
    pub socket: PathBuf,
    rx: mpsc::Receiver<String>,
}

pub struct Run {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    /// 正規化済みの受信行。
    pub lines: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oracle() -> String {
    std::env::var("HERDR_AGENT_METADATA_ORACLE").unwrap_or_else(|_| DEFAULT_ORACLE.to_string())
}

impl Harness {
    pub fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("ham")
            .tempdir_in("/tmp")
            .unwrap();
        for d in ["home", "run", "config/herdr", "tmp"] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        let socket = tmp.path().join("h.sock");
        Self::with_socket(tmp, socket)
    }

    fn with_socket(tmp: tempfile::TempDir, socket: PathBuf) -> Self {
        let listener = UnixListener::bind(&socket).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut line = String::new();
                let _ = BufReader::new(&conn).read_line(&mut line);
                let _ = conn.write_all(b"{\"result\": {}}\n");
                let _ = tx.send(line);
            }
        });
        Harness { tmp, socket, rx }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    /// ペインの状態ファイル(claude のみ使う)。
    pub fn mode_state(&self) -> PathBuf {
        self.path(&format!("run/herdr-claude-mode.{PANE}"))
    }

    /// `name` ディレクトリに branch `branch` の空 repo を作り、パスを返す。
    pub fn git_repo(&self, name: &str, branch: &str) -> PathBuf {
        let dir = self.path(name);
        let st = Command::new("git")
            .args(["init", "-q", "-b", branch])
            .arg(&dir)
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(st.success());
        dir
    }

    pub fn write_marks(&self, body: &str) {
        std::fs::write(self.path("config/herdr/oshi-marks.tsv"), body).unwrap();
    }

    pub fn command(&self, agent: &str, args: &[&str]) -> Command {
        let mut c = if oracle() == "bash" {
            let rel = match agent {
                "claude" => "config/claude/hooks/herdr-claude-metadata.sh",
                "codex" => "config/codex/hooks/herdr-codex-metadata.sh",
                "copilot" => "config/copilot/hooks/herdr-copilot-metadata.sh",
                other => panic!("unknown agent {other}"),
            };
            let mut c = Command::new("sh");
            c.arg(repo_root().join(rel));
            c
        } else {
            let mut c = Command::new(env!("CARGO_BIN_EXE_herdr-agent-metadata"));
            c.args(["--agent", agent]);
            c
        };
        c.args(args);
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("TMPDIR", self.path("tmp"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HERDR_ENV", "1")
            .env("HERDR_SOCKET_PATH", &self.socket)
            .env("HERDR_PANE_ID", PANE);
        c
    }

    pub fn run(&self, agent: &str, args: &[&str], stdin: &str) -> Run {
        self.run_cmd(self.command(agent, args), stdin)
    }

    pub fn run_cmd(&self, mut c: Command, stdin: &str) -> Run {
        c.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        // hook は応答を待ってから終わるので、終了時点で行は届いている。
        let mut lines = Vec::new();
        while let Ok(l) = self.rx.recv_timeout(Duration::from_millis(100)) {
            lines.push(normalize(l.trim_end_matches('\n')));
        }
        Run {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            lines,
        }
    }
}

/// `"id": "<src>:<ms>:<rand6>"` と `"seq": <ns>` の非決定部を置き換える。
/// 形が崩れていれば panic する(形そのものも契約の一部)。
pub fn normalize(line: &str) -> String {
    let mut s = line.to_string();
    let key = r#""id": ""#;
    let i = s.find(key).expect("id key") + key.len();
    let j = i + s[i..].find('"').expect("id end");
    let parts: Vec<&str> = s[i..j].split(':').collect();
    assert_eq!(parts.len(), 3, "id shape: {line}");
    assert!(parts[1].chars().all(|c| c.is_ascii_digit()) && !parts[1].is_empty());
    assert!(parts[2].len() == 6 && parts[2].chars().all(|c| c.is_ascii_digit()));
    let src = parts[0].to_string();
    s.replace_range(i..j, &format!("{src}:<ms>:<rand>"));
    let key = r#""seq": "#;
    let i = s.find(key).expect("seq key") + key.len();
    let n = s[i..].chars().take_while(|c| c.is_ascii_digit()).count();
    assert!(n > 0, "seq digits: {line}");
    s.replace_range(i..i + n, "<seq>");
    s
}

/// python 版の `json.dumps(request)` を組み立てる。`tokens` の値は JSON リテラル。
pub fn expected(source: &str, tokens: &[(&str, &str)], ttl: bool) -> String {
    let toks: Vec<String> = tokens
        .iter()
        .map(|(k, v)| format!("\"{k}\": {v}"))
        .collect();
    let ttl = if ttl { ", \"ttl_ms\": 14400000" } else { "" };
    format!(
        "{{\"id\": \"{source}:<ms>:<rand>\", \"method\": \"pane.report_metadata\", \"params\": {{\"pane_id\": \"{PANE}\", \"source\": \"{source}\", \"seq\": <seq>, \"tokens\": {{{}}}{ttl}}}}}",
        toks.join(", ")
    )
}

/// JSON 文字列リテラル(ASCII の範囲だけで使う)。
pub fn s(v: &str) -> String {
    format!("\"{v}\"")
}

pub fn assert_quiet(r: &Run) {
    assert_eq!(r.status, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
}
