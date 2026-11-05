//! calendar-handshake-reminder の統合テスト: 実バイナリを走らせ、SessionStart の出力を読む。

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn run(stdin: &[u8]) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_calendar-handshake-reminder"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn injects_the_procedure_with_the_vocabulary_tokens() {
    let (ok, out, err) = run(br#"{"hook_event_name":"SessionStart","source":"startup"}"#);
    assert!(ok);
    assert_eq!(err, "");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("`[blocked] `"), "{ctx}");
    assert!(ctx.contains("`[secured] `"), "{ctx}");
    assert!(ctx.contains("本人申告"), "{ctx}");
}

#[test]
fn still_injects_when_stdin_is_empty_or_garbage() {
    for input in [&b""[..], &b"not json"[..]] {
        let (ok, out, _) = run(input);
        assert!(ok);
        assert!(out.contains("additionalContext"), "{out}");
    }
}
