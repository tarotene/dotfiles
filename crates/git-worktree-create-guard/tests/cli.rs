//! bash 版 `git-worktree-create-guard --selftest`(全ケース)を実バイナリ越しに
//! 再現する。末尾 2 件の `main` 経由ケースは bash 版でも JSON 入出力を見て
//! いたもの。

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn run_raw(stdin: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_git-worktree-create-guard"))
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
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap(),
    )
}

fn deny_reason(cmd: &str) -> Option<String> {
    let (out, code) =
        run_raw(&json!({"tool_name":"Bash","tool_input":{"command":cmd}}).to_string());
    assert_eq!(code, 0);
    if out.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    Some(h["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn expect_deny(cmd: &str) {
    let r = deny_reason(cmd).unwrap_or_else(|| panic!("deny 期待: {cmd}"));
    assert!(!r.is_empty());
}
fn expect_pass(cmd: &str) {
    assert_eq!(deny_reason(cmd), None, "pass 期待: {cmd}");
}

#[test]
fn deny_forms() {
    expect_deny("git worktree add /tmp/wt");
    expect_deny("git -C /repo worktree add /tmp/wt topic");
    expect_deny("cd /repo && git worktree add /tmp/wt");
    expect_deny("git status\ngit worktree add /tmp/wt");
    expect_deny("git --git-dir=/repo/.git worktree add /tmp/wt");
}

#[test]
fn pass_forms() {
    expect_pass("git worktree list");
    expect_pass("git worktree remove /tmp/wt");
    expect_pass("git worktree prune --dry-run");
    expect_pass("echo \"use git worktree add carefully\"");
    expect_pass("rg \"git worktree add\" docs/");
}

#[test]
fn main_path_json_in_json_out() {
    // bash 版 selftest 末尾の 2 ケース(main 経由)。
    let (out, code) = run_raw(
        &json!({"tool_name":"Bash","tool_input":{"command":"git worktree add /tmp/wt"}})
            .to_string(),
    );
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "直接の git worktree add は一時ディレクトリ削除後に stale 登録を残すため拒否しました。herdr worktree create --cwd <repo> --branch <name> を使ってください。"
    );
    let (out, code) = run_raw(
        &json!({"tool_name":"Bash","tool_input":{"command":"git worktree list"}}).to_string(),
    );
    assert_eq!((out.as_str(), code), ("", 0));
}

#[test]
fn global_option_skipping_variants() {
    expect_deny("git -c core.x=1 worktree add /tmp/wt");
    expect_deny("git --no-pager -C /r worktree add /tmp/wt");
    expect_deny("git --work-tree /w --namespace=n worktree add /tmp/wt");
    expect_pass("git -C /r status");
}

#[test]
fn degenerate_inputs_are_silent_exit_0() {
    for stdin in [
        "not json worktree add",
        "",
        r#"{"tool_name":"Write","tool_input":{"command":"git worktree add /x"}}"#,
        r#"{"tool_name":"Bash","tool_input":{"command":""}}"#,
    ] {
        let (out, code) = run_raw(stdin);
        assert_eq!((out.as_str(), code), ("", 0), "stdin: {stdin}");
    }
}

/// タブは JSON 上で `\t` にエスケープされる。生テキストで絞り込むと
/// `worktree<TAB>add` が素通りする(#637)。
#[test]
fn deny_tab_separated() {
    expect_deny("git\tworktree\tadd /tmp/wt");
    expect_deny("git worktree\tadd /tmp/wt");
    expect_deny("git\t-C\t/r\tworktree\tadd\t/tmp/wt");
}
