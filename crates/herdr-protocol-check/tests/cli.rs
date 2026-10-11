//! 実バイナリ越しに、PATH 上の偽 `herdr`(`status --json` の出力を差し替える)で
//! 4 ケースを検証する: 互換 / 不一致 / restart_needed / herdr 不在。

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// PATH を `dir` だけにして hook を実行する。`herdr` が無ければ `dir` は空でよい。
fn run_with_path(dir: &std::path::Path) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_herdr-protocol-check"))
        .env("PATH", dir)
        .output()
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap(),
    )
}

fn fake_herdr(dir: &std::path::Path, json: &str) {
    let p = dir.join("herdr");
    std::fs::write(&p, format!("#!/bin/sh\nprintf '%s\\n' '{json}'\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn status(compatible: bool, restart: bool) -> String {
    format!(
        r#"{{"client":{{"protocol":22}},"server":{{"running":true,"protocol":21,"compatible":{compatible},"restart_needed":{restart},"server_binary_stale":false}}}}"#
    )
}

#[test]
fn compatible_is_silent() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(d.path(), &status(true, false));
    assert_eq!(run_with_path(d.path()), (String::new(), 0));
}

#[test]
fn mismatch_emits_additional_context() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(d.path(), &status(false, false));
    let (out, code) = run_with_path(d.path());
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "SessionStart");
    let ctx = h["additionalContext"].as_str().unwrap();
    assert!(ctx.contains("client protocol 22") && ctx.contains("server protocol 21"));
}

#[test]
fn restart_needed_emits_additional_context() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(d.path(), &status(true, true));
    let (out, code) = run_with_path(d.path());
    assert_eq!(code, 0);
    assert!(out.contains("additionalContext"));
}

#[test]
fn herdr_absent_is_silent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(run_with_path(d.path()), (String::new(), 0));
}
