//! bash 版 `wrapup-stop-gate.sh --selftest` の SessionStart 節
//! (「session-start は additionalContext を返す」「session-start は未処理件数を
//! 報告する」)と、`wrapup-session-start.sh` の出力全体を固定する(#413)。

mod common;

use common::*;
use std::path::Path;

fn expected_json(inbox: &str, pending: usize) -> String {
    let gate = session_start_gate_path();
    let mut ctx = format!(
        "<hook-directive source=\"wrapup-session-start\" event=\"SessionStart\">\n\
         wrap-up inbox for this project: {inbox}\n\
         When something outside the current task's scope is worth an Issue (a sign of a\n\
         bug, debt, an improvement idea), append it right then as one line per finding:\n\
         \x20 bash '{gate}' --add '{inbox}' '{{\"ts\": \"<ISO8601>\", \"title\": \"<Issue title>\", \"detail\": \"<what and why>\"}}'\n\
         Do not edit the inbox directly (always go through --add). The Stop hook at the\n\
         end of the turn points to the filing procedure for appended items.",
        gate = gate.display()
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThe inbox currently holds {pending} unprocessed item(s) (including leftovers from past sessions)."
        ));
    }
    ctx.push_str("\n</hook-directive>");
    format!(
        "{{\n  \"hookSpecificOutput\": {{\n    \"hookEventName\": \"SessionStart\",\n    \"additionalContext\": {}\n  }}\n}}\n",
        serde_json::to_string(&ctx).unwrap()
    )
}

fn inbox_of(e: &Env, project: &Path) -> String {
    run_args(e.gate(), &["--inbox-path", project.to_str().unwrap()]).stdout
}

#[test]
fn injects_context_and_pending_count() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let inbox = inbox_of(&e, &repo);

    let mut c = e.session_start();
    c.env("CLAUDE_PROJECT_DIR", &repo);
    let r = run(c, &hook_input(&repo));
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    assert_eq!(r.stdout, expected_json(&inbox, 0));

    for l in ["{\"a\":1}", "{\"b\":2}"] {
        run_args(e.gate(), &["--add", &inbox, l]);
    }
    let mut c = e.session_start();
    c.env("CLAUDE_PROJECT_DIR", &repo);
    let r = run(c, &hook_input(&repo));
    assert_eq!(r.stdout, expected_json(&inbox, 2));
    assert!(r.stdout.contains("2 unprocessed item(s)"));
}

/// `.cwd` から project を解決し、旧 inbox の自己修復マージと
/// feedback stamp の刻印も行う。project が無ければ何も出さない。
#[test]
fn migrates_and_stamps() {
    let e = Env::new();
    let repo = repo(&e.path().join("r"), Some("git@github.com:Example/Mig.git"));
    let legacy = e.legacy_inbox(&repo);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "{\"x\":1}\n").unwrap();
    let r = run(
        e.session_start(),
        &format!(r#"{{"cwd":"{}","session_id":"s/1"}}"#, repo.display()),
    );
    assert_eq!(r.code, 0);
    let inbox = e.inbox_for_slug("github-com-example-mig");
    assert_eq!(r.stdout, expected_json(&inbox.display().to_string(), 1));
    assert!(!legacy.exists());
    assert!(e.stamp_dir().join("s_1.stamp").is_file());

    let r = run(e.session_start(), r#"{"session_id":"x"}"#);
    assert_eq!((r.code, r.stdout.as_str()), (0, ""));
    // session_id が無ければ "unknown"
    let r = run(
        e.session_start(),
        &format!(r#"{{"cwd":"{}"}}"#, repo.display()),
    );
    assert_eq!(r.code, 0);
    assert!(e.stamp_dir().join("unknown.stamp").is_file());
}
