//! daily-desk-reminder の統合テスト: 偽のハブと印のディレクトリを tempdir に作り、実バイナリを走らせる。

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

fn fake_hub(root: &Path, with_desk: bool) {
    let perf = root.join("state/performances");
    std::fs::create_dir_all(&perf).unwrap();
    std::fs::write(perf.join("README.md"), "x").unwrap();
    if with_desk {
        let daily = root.join("state/reviews/daily");
        std::fs::create_dir_all(&daily).unwrap();
        std::fs::write(daily.join("desk.md"), "x").unwrap();
    }
}

fn run(hub: Option<&Path>, state: &Path) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_daily-desk-reminder"));
    cmd.env_clear()
        .env("DESK_STATE_DIR", state)
        .env("HOME", state)
        .stdin(Stdio::null());
    if let Some(h) = hub {
        cmd.env("PERFORMANCE_HUB", h);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success());
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn injects_until_the_mark_exists() {
    let dir = tempfile::tempdir().unwrap();
    let hub = dir.path().join("hub");
    let state = dir.path().join("state");
    fake_hub(&hub, true);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("2000-01-01"), "").unwrap();

    let (out, _) = run(Some(&hub), &state);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("前回の机: 2000-01-01"), "{ctx}");
    assert!(ctx.contains("`daily-desk`"));

    // 本日の日付を、注入文から取り出して印を作ると、次は無音になる。
    let today = ctx
        .split("JST ")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap()
        .to_string();
    std::fs::write(state.join(&today), "").unwrap();
    let (out, _) = run(Some(&hub), &state);
    assert_eq!(out, "");
}

#[test]
fn silent_without_a_hub() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let (out, err) = run(None, &state);
    assert_eq!(out, "");
    assert_eq!(err, "");
}

#[test]
fn warns_once_when_the_procedure_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let hub = dir.path().join("hub");
    fake_hub(&hub, false);
    let (out, err) = run(Some(&hub), &dir.path().join("state"));
    assert_eq!(out, "");
    assert!(err.contains("desk.md"), "{err}");
}
