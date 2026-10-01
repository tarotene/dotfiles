//! bash 版 `git-stash-guard.sh --selftest`(全ケース)と Codex adapter の
//! `--selftest` を、実バイナリ越しに stdin JSON で再現する。
//!
//! bash の `expect_deny`/`expect_pass` は関数 `decide` を直接呼んでいたが、
//! ここでは hook として実際に呼ばれる経路(stdin JSON → stdout JSON)で見る。

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn run_raw(args: &[&str], stdin: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_git-stash-guard"))
        .args(args)
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

fn bash_input(cmd: &str) -> String {
    json!({"tool_name": "Bash", "tool_input": {"command": cmd}}).to_string()
}

/// stdout を返す(deny なら JSON、通すなら空)。終了コードは常に 0。
fn run(args: &[&str], cmd: &str) -> String {
    let (out, code) = run_raw(args, &bash_input(cmd));
    assert_eq!(code, 0, "exit code for {cmd}");
    out
}

fn reason_if_deny(args: &[&str], cmd: &str) -> Option<String> {
    let out = run(args, cmd);
    if out.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&out).expect("deny output is JSON");
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    Some(h["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn expect_deny(cmd: &str) {
    let r = reason_if_deny(&[], cmd).unwrap_or_else(|| panic!("deny 期待, 通った: {cmd}"));
    assert!(!r.is_empty(), "理由が空: {cmd}");
}

fn expect_pass(cmd: &str) {
    if let Some(r) = reason_if_deny(&[], cmd) {
        panic!("pass 期待, deny された: {cmd} ({r})");
    }
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn deny_basic_forms() {
    for c in [
        "git stash",
        "git stash push",
        "git stash push -u",
        "git stash push -m wip",
        "git stash pop",
        "git stash apply",
        "git stash drop",
        "git stash clear",
        "git -C /some/worktree stash pop",
        "git -C /some/worktree stash",
        "git stash --keep-index",
    ] {
        expect_deny(c);
    }
}

#[test]
fn deny_compound_commands() {
    for c in [
        "git status; git stash pop",
        "git stash pop && echo done",
        "git stash pop | cat",
        "git stash apply $(evil)",
    ] {
        expect_deny(c);
    }
}

#[test]
fn pass_allowed_forms() {
    let apply = format!("git stash apply {SHA}");
    let drop = format!("git stash drop {SHA}");
    let apply_c = format!("git -C /some/worktree stash apply {SHA}");
    for c in [
        "git stash list",
        "git stash list --format='%H %gs'",
        "git stash show -p",
        "git stash push -u -m rescue-tag",
        "git stash push --include-untracked --message=rescue-tag",
        "git -C /some/worktree stash push -u -m rescue-tag",
        &apply,
        &drop,
        &apply_c,
    ] {
        expect_pass(c);
    }
}

#[test]
fn pass_unrelated_git() {
    for c in [
        "git status",
        "git commit -m 'add stash guard'",
        "git -C /some/worktree commit -m wip",
        "git log --oneline -- stash-notes.md",
    ] {
        expect_pass(c);
    }
}

#[test]
fn regression_multiline_paths_with_stash_word() {
    expect_pass("echo hi\nls -la ~/.claude/hooks/git-stash-guard.sh");
    expect_pass("git status\ngit commit -m wip");
    expect_pass("echo about-stash-guard.md\ngit log --oneline");
    // 片方の文だけが実際の stash 呼び出しならその文で deny。
    expect_deny("echo hi\ngit stash pop");
}

#[test]
fn regression_hyphenated_git_stash_is_not_an_invocation() {
    expect_pass("man git-stash 2>/dev/null | col -b");
    expect_pass("echo stash > /tmp/f");
    expect_pass("cat ~/.claude/hooks/git-stash-guard.sh > /tmp/out");
    // リダイレクト付きでも実際の呼び出しなら deny。
    expect_deny("git stash pop > /tmp/log");
    expect_deny("git -C /some/worktree stash pop 2>&1 | tee /tmp/log");
}

#[test]
fn shelve_and_unshelve_are_out_of_scope() {
    expect_pass("git shelve wip-note");
    expect_pass("git unshelve");
}

#[test]
fn deny_reasons_are_verbatim() {
    let r = reason_if_deny(&[], "git stash pop").unwrap();
    assert_eq!(
        r,
        "git stash pop は他の worktree の WIP を巻き込みます(deny)。この worktree の退避は git unshelve で戻せます。"
    );
    let r = reason_if_deny(&[], "git stash push").unwrap();
    assert!(r.starts_with("git stash push は -u と -m <tag> を両方付けてください(deny)。"));
    assert!(r.contains("git shelve \"<メモ>\" で積めます(推奨)"));
    let r = reason_if_deny(&[], "git stash apply").unwrap();
    assert!(r.contains("git stash list --format=\"%H %gs\" で確認してから"));
    let r = reason_if_deny(&[], "git stash --keep-index").unwrap();
    assert_eq!(
        r,
        "未知の git stash 呼び出しです(deny): git stash --keep-index"
    );
    let r = reason_if_deny(&[], "git stash pop > /tmp/log").unwrap();
    assert_eq!(
        r,
        "複合コマンドの中に git stash が含まれています(deny): git stash pop > /tmp/log"
    );
}

#[test]
fn degenerate_inputs_are_silent_exit_0() {
    // 不正 JSON / stash 語なし / Bash 以外 / command 空は黙って exit 0。
    for stdin in [
        "not json stash",
        "",
        r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#,
        r#"{"tool_name":"Write","tool_input":{"command":"git stash pop"}}"#,
        r#"{"tool_name":"Bash","tool_input":{"command":""}}"#,
        r#"{"tool_name":"Bash","tool_input":{}}"#,
    ] {
        let (out, code) = run_raw(&[], stdin);
        assert_eq!((out.as_str(), code), ("", 0), "stdin: {stdin}");
    }
}

// --- Codex adapter の `--selftest`(5 ケース)---------------------------------

#[test]
fn codex_host_adapter_cases() {
    let h = ["--host", "codex"];
    for c in ["git stash pop", "git stash"] {
        let r = reason_if_deny(&h, c);
        assert!(r.is_some(), "codex deny 期待: {c}");
    }
    for c in [
        "git stash list",
        "git status",
        "git stash push -u -m unique-tag",
    ] {
        assert_eq!(reason_if_deny(&h, c), None, "codex pass 期待: {c}");
    }
    // 出力は claude host とバイト単位で同一。
    assert_eq!(run(&h, "git stash pop"), run(&[], "git stash pop"));
    assert_eq!(
        run(&["--host=codex"], "git stash pop"),
        run(&[], "git stash pop")
    );
}

#[test]
fn unknown_host_is_non_blocking_error() {
    // exit 2 は Claude/Codex でブロック扱いになるため 1 を返す。
    let (out, code) = run_raw(&["--host", "bogus"], &bash_input("git stash pop"));
    assert_eq!((out.as_str(), code), ("", 1));
}
