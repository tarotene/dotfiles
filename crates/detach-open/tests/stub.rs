//! PATH 上のスタブ(`setsid` / `gio`)で、どの argv で切り離し起動するかを固定する。
//! 実際に `setsid` を起動するテストにはしない(CI にビューアが無い)。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn stub(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let log = dir.join("argv.log");
    let _ = fs::remove_file(&log);
    let st = Command::new(env!("CARGO_BIN_EXE_detach-open"))
        .args(args)
        .env("PATH", dir)
        .env("ARGV_LOG", &log)
        .status()
        .unwrap();
    (st.code(), fs::read_to_string(&log).unwrap_or_default())
}

const RECORD: &str = r#"for a in "$@"; do printf '%s\n' "$a"; done > "$ARGV_LOG""#;

#[test]
fn uses_gio_when_available() {
    let d = tempfile::tempdir().unwrap();
    stub(d.path(), "setsid", RECORD);
    stub(d.path(), "gio", "exit 0");
    let (code, argv) = run(d.path(), &["https://example.com", "a b"]);
    assert_eq!(code, Some(0));
    assert_eq!(argv, "-f\ngio\nopen\nhttps://example.com\na b\n");
}

#[test]
fn falls_back_to_absolute_xdg_open() {
    let d = tempfile::tempdir().unwrap();
    stub(d.path(), "setsid", RECORD);
    let (code, argv) = run(d.path(), &["file.pdf"]);
    assert_eq!(code, Some(0));
    assert_eq!(argv, "-f\n/usr/bin/xdg-open\nfile.pdf\n");
}

#[test]
fn stdout_and_stderr_go_to_dev_null() {
    let d = tempfile::tempdir().unwrap();
    stub(d.path(), "setsid", "echo out; echo err >&2");
    let out = Command::new(env!("CARGO_BIN_EXE_detach-open"))
        .env("PATH", d.path())
        .output()
        .unwrap();
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn missing_setsid_exits_127() {
    let d = tempfile::tempdir().unwrap();
    let (code, _) = run(d.path(), &["x"]);
    assert_eq!(code, Some(127));
}
