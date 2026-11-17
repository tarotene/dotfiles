//! `issue-inbox --add` は、`<inbox> <json>` の後ろの余分な引数(`--repo` など)を、黙って捨てずに止める。
//!
//! 行き先のリポジトリは JSON の `"repo"` で決まる。`--add … --repo owner/repo` が無視されると、cwd のリポジトリのノートに
//! 追記されても、エラーも警告も出ず、読み戻して初めて気づく(2026-11-16 に実際に起きた)。
//! vault を宣言しない機体(HOME を存在しない場所にする)で実際のバイナリを起動し、jsonl に何も書かれないことも確かめる。

use std::fs;
use std::process::{Command, Output};

fn add_with(dir: &std::path::Path, extra: &[&str]) -> (Output, String) {
    let inbox = dir.join("inbox.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_issue-inbox"))
        .args([
            "--add",
            inbox.to_str().unwrap(),
            r#"{"title":"t","detail":"d"}"#,
        ])
        .args(extra)
        .env("HOME", "/nonexistent-home")
        .env("CLAUDE_PROJECT_DIR", dir)
        .env_remove("OBSIDIAN_VAULT")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    let text = fs::read_to_string(&inbox).unwrap_or_default();
    (out, text)
}

#[test]
fn an_extra_repo_flag_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let (out, text) = add_with(dir.path(), &["--repo", "owner/other"]);
    assert_eq!(out.status.code(), Some(64));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("余分な引数"), "{err}");
    assert!(err.contains("--repo owner/other"), "{err}");
    assert!(
        err.contains("\"repo\""),
        "行き先を JSON に書く案内が無い: {err}"
    );
    assert_eq!(text, "", "止めたのに、追記された");
}

#[test]
fn without_extra_arguments_the_line_is_added() {
    let dir = tempfile::tempdir().unwrap();
    let (out, text) = add_with(dir.path(), &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("\"title\":\"t\""), "{text}");
}
