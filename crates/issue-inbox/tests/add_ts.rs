//! `issue-inbox --add` の `ts` の扱い(外から見える振る舞い)。
//!
//! vault を宣言しない機体(HOME を存在しない場所にする)で実際のバイナリを起動し、inbox の jsonl に
//! 残った行を読む。`ts` が無い行には機械が今の時刻(JST の ISO 8601)を足し、渡された `ts` は
//! そのまま残す。

use std::fs;
use std::process::{Command, Output};

fn add(dir: &std::path::Path, json: &str) -> (Output, String) {
    let inbox = dir.join("inbox.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_issue-inbox"))
        .args(["--add", inbox.to_str().unwrap(), json])
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

fn field(line: &str, key: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    v[key].as_str().unwrap_or("").to_string()
}

/// `YYYY-MM-DDTHH:MM:SS+09:00` の形か。
fn is_jst_iso8601(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 25
        && s.ends_with("+09:00")
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[..4]
            .iter()
            .chain(&b[5..7])
            .chain(&b[8..10])
            .all(u8::is_ascii_digit)
        && b[11..13]
            .iter()
            .chain(&b[14..16])
            .chain(&b[17..19])
            .all(u8::is_ascii_digit)
}

#[test]
fn a_line_without_ts_gets_the_current_time_and_keeps_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let (out, text) = add(tmp.path(), r#"{"title":"T","detail":"d"}"#);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = text.lines().next().unwrap();
    assert!(is_jst_iso8601(&field(line, "ts")), "{line}");
    assert_eq!(field(line, "title"), "T");
    assert_eq!(field(line, "detail"), "d");
}

#[test]
fn a_given_ts_is_kept_as_is() {
    let tmp = tempfile::tempdir().unwrap();
    let (out, text) = add(
        tmp.path(),
        r#"{"ts":"2026-10-01T09:00:00+09:00","title":"T","detail":"d"}"#,
    );
    assert!(out.status.success());
    let line = text.lines().next().unwrap();
    assert_eq!(field(line, "ts"), "2026-10-01T09:00:00+09:00");
}

#[test]
fn an_empty_ts_is_replaced_and_invalid_json_still_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let (out, text) = add(tmp.path(), r#"{"ts":"","title":"T"}"#);
    assert!(out.status.success());
    assert!(is_jst_iso8601(&field(text.lines().next().unwrap(), "ts")));

    let tmp2 = tempfile::tempdir().unwrap();
    let (bad, text2) = add(tmp2.path(), "not json");
    assert_eq!(bad.status.code(), Some(64));
    assert!(text2.is_empty());
}
