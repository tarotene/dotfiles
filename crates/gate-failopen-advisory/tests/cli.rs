//! 実バイナリを、一時の `XDG_STATE_HOME` に置いた gate-events.jsonl で動かす。
//! 実機のログには触れない。

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

fn write_events(state_home: &Path, events: &[(&str, &str)]) {
    let path = state_home.join("claude/gate-events.jsonl");
    for (gate, decision) in events {
        hook_io::gate_event::record(&path, gate, decision).unwrap();
    }
}

/// 戻り値は (stdout, 終了コード)。`threshold` が `Some` なら環境変数で渡す。
fn run_hook(state_home: &Path, threshold: Option<&str>) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gate-failopen-advisory"));
    c.env("XDG_STATE_HOME", state_home)
        .env("HOME", state_home)
        .env_remove("GATE_FAILOPEN_THRESHOLD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(t) = threshold {
        c.env("GATE_FAILOPEN_THRESHOLD", t);
    }
    let out = c.output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

fn context(stdout: &str) -> String {
    let v: Value = serde_json::from_str(stdout).expect("stdout is JSON");
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string()
}

const GATE: &str = "copilot-plan-review";

#[test]
fn no_log_is_silent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

#[test]
fn below_threshold_is_silent() {
    let d = tempfile::tempdir().unwrap();
    write_events(d.path(), &[(GATE, "fail-open"), (GATE, "fail-open")]);
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

/// 受け入れ条件: 連続 fail-open が閾値(既定 3)に届いたとき、次の SessionStart に警告が出る。
#[test]
fn at_threshold_warns() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
        ],
    );
    let (out, code) = run_hook(d.path(), None);
    assert_eq!(code, 0);
    let ctx = context(&out);
    assert!(ctx.starts_with("[gate-failopen-advisory]"), "{ctx}");
    assert!(ctx.contains(GATE), "{ctx}");
    assert!(ctx.contains("3 回連続"), "{ctx}");
    assert!(ctx.contains("docs/claude/copilot-plan-review.md"), "{ctx}");
}

/// 受け入れ条件: レビューが 1 回成立すると、連続回数がリセットされる。
#[test]
fn a_pass_resets_the_count() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "pass"),
        ],
    );
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
    // pass のあとに再び閾値まで溜まれば、また鳴る。
    write_events(
        d.path(),
        &[
            (GATE, "fail-open"),
            (GATE, "fail-open"),
            (GATE, "fail-open"),
        ],
    );
    assert!(run_hook(d.path(), None).0.contains("3 回連続"));
}

#[test]
fn other_gates_do_not_count() {
    let d = tempfile::tempdir().unwrap();
    write_events(
        d.path(),
        &[
            ("other-gate", "fail-open"),
            ("other-gate", "fail-open"),
            ("other-gate", "fail-open"),
            ("new-tool-guard", "deny"),
        ],
    );
    assert_eq!(run_hook(d.path(), None), (String::new(), 0));
}

#[test]
fn threshold_is_overridable_and_garbage_falls_back_to_default() {
    let d = tempfile::tempdir().unwrap();
    write_events(d.path(), &[(GATE, "fail-open"), (GATE, "fail-open")]);
    assert!(
        run_hook(d.path(), Some("2")).0.contains("2 回連続"),
        "閾値 2 なら 2 回で鳴る"
    );
    assert_eq!(
        run_hook(d.path(), Some("abc")),
        (String::new(), 0),
        "数値でなければ既定の 3(2 回では鳴らない)"
    );
    assert_eq!(
        run_hook(d.path(), Some("0")),
        (String::new(), 0),
        "0 は無効として既定の 3 に戻す(常に鳴らない)"
    );
}
