//! 実バイナリ越しの検証: stdin JSON → stdout JSON。一時 repo と linked
//! worktree を作り、deny/allow の境界と Stop の事後検出を見る。

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Fixture {
    _tmp: tempfile::TempDir,
    main: PathBuf,
    linked: PathBuf,
    state: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    // `-c core.hooksPath=`: ホストの protected-branch guard を無効化する(#428)。
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.hooksPath=",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let main = root.join("repo");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "init"]);
    let linked = root.join("wt");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            linked.to_str().unwrap(),
        ],
    );
    Fixture {
        state: root.join("state"),
        main,
        linked,
        _tmp: tmp,
    }
}

fn run(f: &Fixture, sub: &str, input: &Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_main-checkout-guard"))
        .arg(sub)
        .env("MAIN_CHECKOUT_GUARD_STATE_DIR", &f.state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn tool(name: &str, input: Value, cwd: &Path) -> Value {
    json!({"session_id": "s1", "cwd": cwd, "tool_name": name, "tool_input": input})
}

fn bash(cmd: &str, cwd: &Path) -> Value {
    tool("Bash", json!({ "command": cmd }), cwd)
}

fn stop(cwd: &Path, active: bool) -> Value {
    json!({"session_id": "s1", "cwd": cwd, "hook_event_name": "Stop", "stop_hook_active": active})
}

fn is_deny(out: &str) -> bool {
    serde_json::from_str::<Value>(out)
        .map(|v| v["hookSpecificOutput"]["permissionDecision"] == "deny")
        .unwrap_or(false)
}

#[test]
fn edit_in_main_checkout_is_denied_but_linked_worktree_is_not() {
    let f = fixture();
    let out = run(
        &f,
        "pre",
        &tool(
            "Edit",
            json!({"file_path": f.main.join("a.txt")}),
            &f.linked,
        ),
    );
    assert!(is_deny(&out), "{out}");
    assert!(out.contains("herdr worktree create"));
    // 存在しない新規ファイルでも、最も近い祖先で判定する。
    let out = run(
        &f,
        "pre",
        &tool(
            "Write",
            json!({"file_path": f.main.join("new/dir/x.rs")}),
            &f.linked,
        ),
    );
    assert!(is_deny(&out), "{out}");
    let out = run(
        &f,
        "pre",
        &tool(
            "Edit",
            json!({"file_path": f.linked.join("a.txt")}),
            &f.linked,
        ),
    );
    assert_eq!(out, "");
}

#[test]
fn mutating_git_is_denied_and_read_only_git_passes() {
    let f = fixture();
    let m = f.main.to_str().unwrap();
    for cmd in [
        format!("git -C {m} switch -c x"),
        format!("git\t-C\t{m}\tcommit -m x"),
        format!("echo hi && git -C {m} reset --hard"),
        format!("cd {m} && git add ."),
        format!("git -C {m} branch newbranch"),
    ] {
        let out = run(&f, "pre", &bash(&cmd, &f.linked));
        assert!(is_deny(&out), "{cmd}: {out}");
    }
    for cmd in [
        format!("git -C {m} log --oneline"),
        format!("git -C {m} status"),
        format!("git -C {m} fetch origin"),
        format!("git -C {m} pull --ff-only"),
        format!("git -C {m} branch --list"),
        "git status".to_string(),
    ] {
        let out = run(&f, "pre", &bash(&cmd, &f.linked));
        assert!(!is_deny(&out), "{cmd}: {out}");
    }
    // linked worktree 側の変更系 git は通る。
    let out = run(&f, "pre", &bash("git commit -m x", &f.linked));
    assert!(!is_deny(&out), "{out}");
}

#[test]
fn bash_bypass_is_caught_at_stop_once() {
    let f = fixture();
    // 触れた時点: クリーンな main。
    let out = run(&f, "pre", &bash("sed -i s/a/b/ a.txt", &f.main));
    assert_eq!(out, "");
    // 字面からは書き込みと分からない迂回。
    std::fs::write(f.main.join("a.txt"), "changed\n").unwrap();
    let out = run(&f, "stop", &stop(&f.linked, false));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["decision"], "block");
    assert!(v["reason"]
        .as_str()
        .unwrap()
        .contains("working tree or index changed"));
    // 報告済みの状態は baseline が更新され、次のターンで繰り返さない。
    assert_eq!(run(&f, "stop", &stop(&f.linked, false)), "");
}

#[test]
fn stop_is_silent_on_stop_hook_active_and_when_nothing_was_touched() {
    let f = fixture();
    run(&f, "pre", &bash("true", &f.main));
    std::fs::write(f.main.join("a.txt"), "changed\n").unwrap();
    assert_eq!(run(&f, "stop", &stop(&f.linked, true)), "");

    let g = fixture();
    std::fs::write(g.main.join("a.txt"), "changed\n").unwrap();
    assert_eq!(run(&g, "stop", &stop(&g.linked, false)), "");
}

#[test]
fn read_tools_record_a_baseline() {
    let f = fixture();
    let out = run(
        &f,
        "pre",
        &tool(
            "Read",
            json!({"file_path": f.main.join("a.txt")}),
            &f.linked,
        ),
    );
    assert_eq!(out, "");
    git(&f.main, &["switch", "-q", "-c", "elsewhere"]);
    let out = run(&f, "stop", &stop(&f.linked, false));
    assert!(out.contains("branch main -> elsewhere"), "{out}");
}

#[test]
fn fast_forward_is_not_a_drift() {
    let f = fixture();
    run(&f, "pre", &bash("true", &f.main));
    std::fs::write(f.main.join("b.txt"), "b\n").unwrap();
    git(&f.main, &["add", "b.txt"]);
    git(&f.main, &["commit", "-q", "-m", "second"]);
    // クリーンなまま HEAD が前進しただけ(pull --ff-only 相当)。
    assert_eq!(run(&f, "stop", &stop(&f.linked, false)), "");
}

#[test]
fn already_collapsed_checkout_warns_but_never_blocks() {
    let f = fixture();
    std::fs::write(f.main.join("a.txt"), "dirty before the session\n").unwrap();
    let out = run(&f, "pre", &bash("true", &f.main));
    let v: Value = serde_json::from_str(&out).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("not clean on the default branch"), "{ctx}");
    // 2 回目以降は繰り返さない。
    assert_eq!(run(&f, "pre", &bash("true", &f.main)), "");
    std::fs::write(f.main.join("a.txt"), "dirty differently\n").unwrap();
    assert_eq!(run(&f, "stop", &stop(&f.linked, false)), "");
}

#[test]
fn unusable_input_is_fail_open() {
    let f = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_main-checkout-guard"))
        .arg("pre")
        .env("MAIN_CHECKOUT_GUARD_STATE_DIR", &f.state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(b"not json");
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}
