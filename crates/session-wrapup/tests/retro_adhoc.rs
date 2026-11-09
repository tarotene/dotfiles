//! その場で書いたコードの痕跡による retro の発火と開き直し(ADR-1276)の統合テスト。
//!
//! 痕跡は、セッション専用の scratchpad(`$CLAUDE_CODE_TMPDIR/<project>/<sid>/scratchpad`)のコードと、
//! `cmd-hashes.jsonl`(HOME は隔離済み)の形つきのハッシュ。PR の台帳は置かない(PR の無いセッション)。

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::process::Command;

const SID: &str = "s1";

fn project(e: &Env) -> PathBuf {
    repo(
        &e.path().join("proj"),
        Some("https://github.com/acme/x.git"),
    )
}

fn gate(e: &Env) -> Command {
    let mut c = e.gate();
    c.env("CLAUDE_CODE_TMPDIR", e.path().join("cct"))
        .env_remove("CMD_HASH_LOG_PATH");
    c
}

fn stop(e: &Env, p: &Path) -> Run {
    run(
        gate(e),
        &format!(
            r#"{{"cwd":"{}","session_id":"{SID}","stop_hook_active":false}}"#,
            p.display()
        ),
    )
}

fn add_row(e: &Env, p: &Path, row: &str) -> Run {
    let mut c = gate(e);
    c.env("CLAUDE_PROJECT_DIR", p);
    run_args(c, &["--retro-add", SID, row])
}

fn close(e: &Env) -> Run {
    run_args(gate(e), &["--retro-close", SID])
}

fn procedure(e: &Env) -> Run {
    run_args(gate(e), &["--retro-procedure", SID])
}

/// このセッション専用の scratchpad にファイルを置く。
fn scratch(e: &Env, name: &str, body: &str) {
    let d = e
        .path()
        .join("cct/some-project")
        .join(SID)
        .join("scratchpad");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join(name), body).unwrap();
}

/// `cmd-hashes.jsonl` に 1 行足す。`shape` は閉じた語(無ければ古い形式の行)。
fn hash_line(e: &Env, hash: &str, shape: Option<&[&str]>) {
    let f = e.home().join(".local/state/claude/cmd-hashes.jsonl");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    let shape = shape
        .map(|s| format!(r#","shape":{}"#, serde_json_array(s)))
        .unwrap_or_default();
    let line =
        format!(r#"{{"hash":"{hash}","repo_key":"/r","session_id":"{SID}"{shape},"ts_unix":1}}"#);
    let mut old = std::fs::read_to_string(&f).unwrap_or_default();
    old.push_str(&line);
    old.push('\n');
    std::fs::write(f, old).unwrap();
}

fn serde_json_array(items: &[&str]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn codify_row(id: &str) -> String {
    format!(
        r#"{{"kind":"codify","what":"w","evidence":"{id}","disposition":"none:r","mechanism":"none:one-off"}}"#
    )
}

#[test]
fn a_session_without_traces_or_a_pr_is_silent() {
    let e = Env::new();
    let p = project(&e);
    let r = stop(&e, &p);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn a_plain_repeat_without_a_shape_does_not_fire() {
    let e = Env::new();
    let p = project(&e);
    for _ in 0..5 {
        hash_line(&e, "aaaaaaaaaaaaaaaa", Some(&[])); // cargo test のやり直し
        hash_line(&e, "bbbbbbbbbbbbbbbb", None); // 形を足す前の古い行
    }
    let r = stop(&e, &p);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn code_in_the_scratchpad_fires_but_other_files_do_not() {
    let e = Env::new();
    let p = project(&e);
    scratch(&e, "body.md", "# pr body");
    scratch(&e, "findings.json", "{}");
    assert_eq!(stop(&e, &p).code, 0);

    scratch(&e, "x.py", "print(1)");
    let r = stop(&e, &p);
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(r.stderr.contains("wrote ad-hoc code"), "{}", r.stderr);
    assert!(!r.stderr.contains("created a PR"), "{}", r.stderr);

    let proc = procedure(&e);
    assert!(
        proc.stdout.contains("[TODO] adhoc:scratch:x.py"),
        "{}",
        proc.stdout
    );
}

#[test]
fn a_single_interpreter_one_liner_does_not_fire_but_three_shapes_do() {
    let e = Env::new();
    let p = project(&e);
    hash_line(&e, "1111111111111111", Some(&["inline-interp"]));
    assert_eq!(stop(&e, &p).code, 0);
    hash_line(&e, "2222222222222222", Some(&["inline-interp"]));
    hash_line(&e, "3333333333333333", Some(&["heredoc-interp"]));
    let r = stop(&e, &p);
    assert_eq!(r.code, 2, "{}", r.stderr);
    let proc = procedure(&e);
    assert!(
        proc.stdout.contains("adhoc:shape:inline-interp"),
        "{}",
        proc.stdout
    );
    assert!(
        proc.stdout.contains("adhoc:shape:heredoc-interp"),
        "{}",
        proc.stdout
    );
}

#[test]
fn a_shaped_command_that_ran_twice_fires() {
    let e = Env::new();
    let p = project(&e);
    hash_line(&e, "4444444444444444", Some(&["pipeline"]));
    assert_eq!(stop(&e, &p).code, 0);
    hash_line(&e, "4444444444444444", Some(&["pipeline"]));
    assert_eq!(stop(&e, &p).code, 2);
    assert!(
        procedure(&e).stdout.contains("adhoc:repeat:44444444"),
        "{}",
        procedure(&e).stdout
    );
}

#[test]
fn only_a_codify_row_clears_an_adhoc_event() {
    let e = Env::new();
    let p = project(&e);
    scratch(&e, "x.py", "print(1)");
    // insight が id を引いても、決定論化を問うたことにならない。
    let insight =
        r#"{"kind":"insight","what":"w","evidence":"adhoc:scratch:x.py","disposition":"none:r"}"#;
    assert_eq!(add_row(&e, &p, insight).code, 0);
    let r = close(&e);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("adhoc:scratch:x.py"), "{}", r.stderr);

    assert_eq!(add_row(&e, &p, &codify_row("adhoc:scratch:x.py")).code, 0);
    let r = close(&e);
    assert_eq!(r.code, 0, "{}", r.stderr);
    // 閉じた後は、新しい痕跡が無ければ黙る。
    let r = stop(&e, &p);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn a_codify_row_needs_a_mechanism() {
    let e = Env::new();
    let p = project(&e);
    let bad =
        r#"{"kind":"codify","what":"w","evidence":"adhoc:scratch:x.py","disposition":"none:r"}"#;
    let r = add_row(&e, &p, bad);
    assert_eq!(r.code, 64, "{}", r.stderr);
    assert!(r.stderr.contains("codify"), "{}", r.stderr);
}

#[test]
fn new_traces_after_closing_reopen_the_retro_at_most_twice() {
    let e = Env::new();
    let p = project(&e);

    // 1 回目: 閉じるまで。
    scratch(&e, "a.py", "print(1)");
    assert_eq!(stop(&e, &p).code, 2);
    assert_eq!(add_row(&e, &p, &codify_row("adhoc:scratch:a.py")).code, 0);
    assert_eq!(close(&e).code, 0);
    assert_eq!(stop(&e, &p).code, 0);

    // 閉じた後に書いたコード → 開き直す(1 回目)。
    scratch(&e, "b.py", "print(2)");
    let r = stop(&e, &p);
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(
        r.stderr.contains("after the retrospective was closed"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("1 event(s) not yet covered"),
        "{}",
        r.stderr
    );
    assert_eq!(add_row(&e, &p, &codify_row("adhoc:scratch:b.py")).code, 0);
    assert_eq!(close(&e).code, 0);
    assert_eq!(stop(&e, &p).code, 0);

    // 2 回目。
    scratch(&e, "c.py", "print(3)");
    assert_eq!(stop(&e, &p).code, 2);
    assert_eq!(add_row(&e, &p, &codify_row("adhoc:scratch:c.py")).code, 0);
    assert_eq!(close(&e).code, 0);

    // 3 回目は開き直さない(上限 2)。
    scratch(&e, "d.py", "print(4)");
    let r = stop(&e, &p);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn a_pr_session_keeps_saying_it_created_a_pr() {
    let e = Env::new();
    let p = project(&e);
    let f = e.home().join(".claude/gh-edit-allow/state/s1.ledger");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, "pr acme/x 5\n").unwrap();
    scratch(&e, "x.py", "print(1)");
    let r = stop(&e, &p);
    assert_eq!(r.code, 2);
    assert!(
        r.stderr.contains("This session created a PR"),
        "{}",
        r.stderr
    );
    // PR の有無にかかわらず、痕跡は行に引かれなければならない。
    assert!(procedure(&e).stdout.contains("adhoc:scratch:x.py"));
}
