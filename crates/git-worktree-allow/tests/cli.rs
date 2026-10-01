//! bash 版 `git-worktree-allow.sh --selftest`(全ケース)を実バイナリ越しに
//! 再現する。bash 版が `decide` を直接呼んでいたのに対し、ここでは stdin JSON
//! → stdout JSON の hook 経路で見る。実ディレクトリと symlink は tempfile で
//! 実際に作る。

use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    wt: PathBuf,
    base: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    // tempdir 自体が symlink 配下でも root が realpath と一致するよう解決しておく。
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let root = base.join("worktrees");
    let wt = root.join("wt1");
    std::fs::create_dir_all(wt.join("nested")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    symlink(base.join("outside"), root.join("escape")).unwrap();
    Fixture {
        _tmp: tmp,
        root,
        wt,
        base,
    }
}

fn run_raw(f: &Fixture, stdin: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_git-worktree-allow"))
        .env("HERDR_WORKTREES_DIR", &f.root)
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

fn allow_reason(f: &Fixture, cmd: &str) -> Option<String> {
    let (out, code) = run_raw(
        f,
        &json!({"tool_name":"Bash","tool_input":{"command":cmd}}).to_string(),
    );
    assert_eq!(code, 0);
    if out.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "allow");
    Some(h["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn expect_allow(f: &Fixture, cmd: &str) {
    assert!(allow_reason(f, cmd).is_some(), "allow 期待: {cmd}");
}
fn expect_deny(f: &Fixture, cmd: &str) {
    assert_eq!(allow_reason(f, cmd), None, "フォールスルー期待: {cmd}");
}

#[test]
fn allowed_forms() {
    let f = fixture();
    let wt = f.wt.display();
    for c in [
        format!("git -C {wt} status"),
        format!("git -C {wt} diff --stat"),
        format!("git -C {wt} add -A"),
        format!("git -C {wt} commit -m wip"),
        format!("git -C {wt} push origin HEAD"),
        format!("git -C {wt}/nested log --oneline -5"),
    ] {
        expect_allow(&f, &c);
    }
}

#[test]
fn allow_reason_text_is_verbatim() {
    let f = fixture();
    let r = allow_reason(&f, &format!("git -C {}/nested log -1", f.wt.display())).unwrap();
    assert_eq!(
        r,
        format!("git -C {}/nested log (herdr worktree)", f.wt.display())
    );
}

#[test]
fn out_of_range_missing_symlink_escape_and_root_itself() {
    let f = fixture();
    let r = f.root.display();
    expect_deny(&f, "git -C /tmp status");
    expect_deny(&f, &format!("git -C {r}/missing status"));
    expect_deny(&f, &format!("git -C {r}/escape status"));
    expect_deny(&f, &format!("git -C {r} status"));
    let _ = &f.base;
}

#[test]
fn option_injection_and_malformed_shape() {
    let f = fixture();
    let wt = f.wt.display();
    expect_deny(&f, "git -C --exec-path=/evil add .");
    expect_deny(&f, &format!("git -c core.pager=evil -C {wt} status"));
    expect_deny(&f, &format!("git --exec-path=/evil -C {wt} status"));
    expect_deny(&f, &format!("git -C {wt}"));
    expect_deny(&f, &format!("env git -C {wt} status"));
}

#[test]
fn disallowed_subcommands() {
    let f = fixture();
    let wt = f.wt.display();
    expect_deny(&f, &format!("git -C {wt} rebase main"));
    expect_deny(&f, &format!("git -C {wt} config user.name evil"));
}

#[test]
fn compound_redirect_expansion() {
    let f = fixture();
    let wt = f.wt.display();
    expect_deny(&f, &format!("git -C {wt} status; rm -rf /"));
    expect_deny(&f, &format!("git -C {wt} status && evil"));
    expect_deny(&f, &format!("git -C {wt} status | tee /tmp/x"));
    expect_deny(&f, &format!("git -C {wt} add $(evil)"));
    expect_deny(&f, &format!("git -C {wt} status > /tmp/x"));
}

#[test]
fn args_that_redirect_git_to_another_executable() {
    let f = fixture();
    let wt = f.wt.display();
    expect_deny(&f, &format!("git -C {wt} push --receive-pack=/evil origin"));
    expect_deny(&f, &format!("git -C {wt} push --upload-pack=/evil origin"));
    expect_deny(&f, &format!("git -C {wt} push 'ext::sh -c evil'"));
}

#[test]
fn degenerate_inputs_are_silent_exit_0() {
    let f = fixture();
    let wt = f.wt.display();
    for stdin in [
        "not json".to_string(),
        String::new(),
        json!({"tool_name":"Write","tool_input":{"command":format!("git -C {wt} status")}})
            .to_string(),
        json!({"tool_name":"Bash","tool_input":{}}).to_string(),
    ] {
        let (out, code) = run_raw(&f, &stdin);
        assert_eq!((out.as_str(), code), ("", 0), "stdin: {stdin}");
    }
}
