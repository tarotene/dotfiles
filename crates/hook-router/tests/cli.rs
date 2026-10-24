//! 実バイナリでの end-to-end。実際の検査(`git-stash-guard` など)が、ルータ経由で動く。

use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_hook-router");

/// 台帳・記録・設定は、テスト用の一時ディレクトリに向ける(本物の HOME を汚さない)。
fn run(args: &[&str], stdin: &str) -> (Output, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(BIN)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    (child.wait_with_output().unwrap(), home)
}

fn bash(event: &str, command: &str) -> String {
    serde_json::json!({
        "hook_event_name": event,
        "session_id": "s1",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": "/tmp",
    })
    .to_string()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout が JSON でない: {e}: {:?}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

#[test]
fn a_bare_git_stash_is_denied_through_the_router() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "git stash"));
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("git stash"), "{reason}");
}

#[test]
fn a_harmless_command_produces_no_output() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "ls -la"));
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn worktree_add_is_denied_by_the_other_guard_in_the_same_run() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "git worktree add ../x"));
    let v = json(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .contains("worktree"));
}

#[test]
fn two_guards_denying_at_once_both_reasons_survive() {
    // `git stash` は git-stash-guard、`git worktree add` は worktree-create-guard が拒否する。
    let (out, _h) = run(
        &["pre"],
        &bash("PreToolUse", "git stash && git worktree add ../x"),
    );
    let v = json(&out);
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(
        reason.contains("git stash") && reason.contains("worktree"),
        "理由がどちらも残る: {reason}"
    );
    assert!(reason.contains("\n\n"), "理由は空行で区切られる");
}

#[test]
fn a_tool_without_applicable_rules_is_a_silent_pass() {
    let raw = serde_json::json!({
        "hook_event_name": "PreToolUse", "session_id": "s", "tool_name": "WebFetch",
        "tool_input": {"url": "https://example.com"}, "cwd": "/tmp"
    })
    .to_string();
    let (out, _h) = run(&["pre"], &raw);
    assert!(out.status.success() && out.stdout.is_empty());
}

#[test]
fn unreadable_input_is_a_silent_pass() {
    for input in ["", "not json", "{}", r#"{"tool_name":""}"#] {
        let (out, _h) = run(&["pre"], input);
        assert!(out.status.success(), "{input:?}");
        assert!(out.stdout.is_empty(), "{input:?}");
    }
}

#[test]
fn post_event_runs_the_recorders_and_stays_silent_when_there_is_nothing_to_say() {
    let (out, _h) = run(&["post"], &bash("PostToolUse", "ls"));
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let (out, _h) = run(&["stop"], "{}");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: hook-router pre|post"));
    let (out, _h) = run(&[], "{}");
    assert_eq!(out.status.code(), Some(1));
}
