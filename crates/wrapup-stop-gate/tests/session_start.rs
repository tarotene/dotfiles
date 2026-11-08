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
         When something outside the current task's scope is worth recording (a sign of a\n\
         bug, debt, an improvement idea), append it right then as one line per finding:\n\
         \x20 '{gate}' --add '{inbox}' '{{\"title\": \"<title>\", \"detail\": \"<what and why>\"}}'\n\
         (the timestamp \"ts\" is added for you; do not build it with $(date))\n\
         A finding becomes a note in the user's vault (inbox/, one note per title; the same title\n\
         adds a row instead of a duplicate) or, on a machine without a vault, a line in the jsonl above.\n\
         It is NOT filed as a GitHub Issue: the user adjudicates findings later with /my-cleanup,\n\
         which fixes them by PR and files an Issue only for the ones the user picks. Do not file an\n\
         Issue for a finding yourself, and do not edit the inbox directly (always go through --add).",
        gate = gate.display()
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThere are {pending} unadjudicated finding(s) for this project (including leftovers from past sessions)."
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
    assert!(r.stdout.contains("2 unadjudicated finding(s)"));
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

const SUGGESTION: &str = "Propose running /my-cleanup to the user";

fn start_in(e: &Env, project: &Path, vault: Option<&Path>) -> String {
    let mut c = e.session_start();
    c.env("CLAUDE_PROJECT_DIR", project);
    if let Some(v) = vault {
        c.env("OBSIDIAN_VAULT", v);
    }
    let r = run(c, &hook_input(project));
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    r.stdout
}

fn days_ago(n: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    issue_inbox::vault::jst_date(now - n * 86_400)
}

/// vault を宣言し、`rows` の日付の行を持つ open なノートを 1 枚置く。repo は `example/vaulted`。
fn put_note(vault: &Path, title: &str, first_seen: &str, rows: &[String]) {
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    std::fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    std::fs::create_dir_all(vault.join("inbox")).unwrap();
    let body: String = rows.iter().map(|d| format!("- {d} 行\n")).collect();
    std::fs::write(
        vault.join("inbox").join(format!("{title}.md")),
        format!(
            "---\ntitle: \"{title}\"\nrepo: \"example/vaulted\"\nstatus: \"open\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"{first_seen}\"\n---\n{body}"
        ),
    )
    .unwrap();
}

#[test]
fn suggests_the_cleanup_when_ten_findings_have_piled_up() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let inbox = inbox_of(&e, &repo);
    for i in 0..9 {
        run_args(e.gate(), &["--add", &inbox, &format!("{{\"a\":{i}}}")]);
    }
    let out = start_in(&e, &repo, None);
    assert!(out.contains("9 unadjudicated finding(s)"), "{out}");
    assert!(!out.contains(SUGGESTION), "9 件ではまだ提案しない: {out}");

    run_args(e.gate(), &["--add", &inbox, "{\"a\":9}"]);
    let out = start_in(&e, &repo, None);
    assert!(out.contains("10 unadjudicated finding(s)"), "{out}");
    assert!(out.contains(SUGGESTION), "10 件で提案する: {out}");
}

#[test]
fn suggests_the_cleanup_when_an_open_note_is_a_week_past_its_last_row() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/vaulted.git"),
    );
    let vault = e.path().join("vault");
    put_note(&vault, "放置", &days_ago(8), &[days_ago(8)]);
    let out = start_in(&e, &repo, Some(&vault));
    assert!(out.contains("1 unadjudicated finding(s)"), "{out}");
    assert!(out.contains(SUGGESTION), "最後の行から 8 日: {out}");
    assert!(out.contains("8 day(s) ago"), "{out}");
}

#[test]
fn does_not_suggest_for_a_recent_note_even_if_it_was_first_seen_long_ago() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/vaulted.git"),
    );
    let vault = e.path().join("vault");
    // 初出は 60 日前だが、再発が 3 日前。時計は最後の行(失効と同じ)から数える。
    put_note(
        &vault,
        "再発中",
        &days_ago(60),
        &[days_ago(60), days_ago(3)],
    );
    let out = start_in(&e, &repo, Some(&vault));
    assert!(out.contains("1 unadjudicated finding(s)"), "{out}");
    assert!(!out.contains(SUGGESTION), "{out}");
}

#[test]
fn does_not_suggest_without_findings() {
    let e = Env::new();
    let repo = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/example.git"),
    );
    let out = start_in(&e, &repo, None);
    assert!(!out.contains(SUGGESTION), "{out}");
}
