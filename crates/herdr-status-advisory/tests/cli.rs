//! 実バイナリを、PATH の先頭に置いた偽の `herdr` で動かす。本物の herdr や server には触れない。

use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

fn fake_herdr(dir: &Path, script_body: &str) {
    let path = dir.join("herdr");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh\n{script_body}").unwrap();
    drop(f);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// PATH を `dir` だけにして実行する(本物の herdr を拾わない)。`/bin/sh` は絶対パスで呼ばれる。
fn run_hook(dir: &Path) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_herdr-status-advisory"));
    c.env("PATH", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = c.output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

fn emit(json: &str) -> String {
    // PATH には偽の herdr しか無いので、cat などの外部コマンドは使えない。printf は sh の組み込み。
    assert!(!json.contains('\''));
    format!("printf '%s\\n' '{json}'")
}

#[test]
fn healthy_server_is_silent() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(
        d.path(),
        &emit(
            r#"{"client":{"protocol":22},"server":{"running":true,"protocol":22,"compatible":true,"endpoint_compatible":true,"restart_needed":false,"server_binary_stale":false},"update":{"restart_needed":false,"server_binary_stale":false}}"#,
        ),
    );
    assert_eq!(run_hook(d.path()), (String::new(), 0));
}

#[test]
fn protocol_mismatch_prints_session_start_context() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(
        d.path(),
        &emit(
            r#"{"client":{"protocol":22},"server":{"running":true,"protocol":20,"compatible":false,"endpoint_compatible":true,"restart_needed":true},"update":{"restart_needed":true}}"#,
        ),
    );
    let (out, code) = run_hook(d.path());
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("client protocol 22"));
    assert!(ctx.contains("server protocol 20"));
    assert!(ctx.contains("herdr server stop"));
}

#[test]
fn herdr_missing_is_silent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(run_hook(d.path()), (String::new(), 0));
}

#[test]
fn herdr_status_failure_is_silent() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(d.path(), "exit 1");
    assert_eq!(run_hook(d.path()), (String::new(), 0));
}

#[test]
fn unparsable_output_is_silent() {
    let d = tempfile::tempdir().unwrap();
    fake_herdr(d.path(), "echo 'not json'");
    assert_eq!(run_hook(d.path()), (String::new(), 0));
}

#[test]
fn a_hung_herdr_status_is_cut_off() {
    let d = tempfile::tempdir().unwrap();
    // `sleep` は PATH に無いので、sh の組み込みだけで待つ。read は標準入力が /dev/null なら即返るので使えない。
    fake_herdr(d.path(), "while :; do :; done");
    let start = Instant::now();
    assert_eq!(run_hook(d.path()), (String::new(), 0));
    assert!(start.elapsed().as_secs() < 15, "タイムアウトで打ち切るはず");
}
