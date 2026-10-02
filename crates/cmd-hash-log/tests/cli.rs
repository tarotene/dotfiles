//! main.rs のワイヤリング(stdin 読み・ファイル書き込み・git toplevel 解決)を
//! 実バイナリ越しに検査する。純粋なハッシュ・パス解決ロジックは lib.rs /
//! hook_io::cmd_hash の単体テストで検査済みなので、ここでは配線に絞る。

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("-c")
        .arg("core.hooksPath=")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn init_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(
        d.path(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    d
}

fn run(repo: &Path, log_path: &Path, stdin_json: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_cmd-hash-log"))
        .current_dir(repo)
        .env("CMD_HASH_LOG_PATH", log_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_json.as_bytes())
        .ok();
    child.wait_with_output().unwrap()
}

#[test]
fn appends_one_line_without_command_body() {
    let repo = init_repo();
    let log = repo.path().join("state").join("cmd-hashes.jsonl");
    let stdin = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "echo secret-token-xyz"},
        "session_id": "s1",
        "cwd": repo.path().to_str().unwrap(),
    })
    .to_string();

    let out = run(repo.path(), &log, &stdin);
    assert!(out.stdout.is_empty());

    let content = fs::read_to_string(&log).unwrap();
    assert_eq!(content.lines().count(), 1);
    assert!(!content.contains("secret-token-xyz"), "{content}");
    assert!(content.contains("\"session_id\":\"s1\""), "{content}");
    assert!(content.contains(repo.path().to_str().unwrap()), "{content}");
}

#[test]
fn ignores_non_post_tool_use_events() {
    let repo = init_repo();
    let log = repo.path().join("state").join("cmd-hashes.jsonl");
    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "echo x"},
        "cwd": repo.path().to_str().unwrap(),
    })
    .to_string();

    let out = run(repo.path(), &log, &stdin);
    assert!(out.stdout.is_empty());
    assert!(!log.exists());
}

#[test]
fn ignores_non_bash_tools() {
    let repo = init_repo();
    let log = repo.path().join("state").join("cmd-hashes.jsonl");
    let stdin = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Read",
        "tool_input": {},
        "cwd": repo.path().to_str().unwrap(),
    })
    .to_string();

    let out = run(repo.path(), &log, &stdin);
    assert!(out.stdout.is_empty());
    assert!(!log.exists());
}

#[test]
fn appends_across_multiple_invocations() {
    let repo = init_repo();
    let log = repo.path().join("state").join("cmd-hashes.jsonl");
    for cmd in ["echo a", "echo b"] {
        let stdin = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
            "cwd": repo.path().to_str().unwrap(),
        })
        .to_string();
        run(repo.path(), &log, &stdin);
    }
    let content = fs::read_to_string(&log).unwrap();
    assert_eq!(content.lines().count(), 2);
}
