use std::io::Write;
use std::process::{Command, Stdio};

fn run(mode: &str, input: &str) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_session-wrapup"));
    command
        .arg(mode)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("WRAPUP_STATE_DIR", home.path().join("state"))
        .env("CLAUDE_PROJECT_DIR", home.path())
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input == "not JSON" {
        command.env_remove("CLAUDE_PROJECT_DIR");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn canonical_start_routes_inbox_operations_to_the_inbox_cli() {
    let output = run("start", r#"{"session_id":"s"}"#);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("/issue-inbox' --add"));
    assert!(!text.contains("/wrapup-stop-gate' --add"));
}

#[test]
fn malformed_input_reports_the_parse_failure_once() {
    let output = run("stop", "not JSON");
    assert_eq!(output.status.code(), Some(5));
    let text = String::from_utf8(output.stderr).unwrap();
    assert_eq!(text.matches("hook input is not valid JSON").count(), 1);
}

#[test]
fn stop_with_no_pending_work_is_silent() {
    let output = run("stop", "{}");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
