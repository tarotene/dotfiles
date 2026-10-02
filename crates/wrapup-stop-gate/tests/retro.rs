//! 作業終了時のレトロ(`src/retro.rs`)の統合テスト。
//!
//! PR 作成の判定は gh-edit-allow の台帳 `~/.claude/gh-edit-allow/state/<sid>.ledger`
//! (HOME は隔離済み)に `pr ...` 行を置いて作る。

mod common;

use common::*;
use std::path::{Path, PathBuf};

const SID: &str = "s1";
const ROW_OK: &str = r#"{"kind":"insight","what":"w","evidence":"e","disposition":"none:r"}"#;

fn stop_input(cwd: &Path, active: bool) -> String {
    format!(
        r#"{{"cwd":"{}","session_id":"{SID}","stop_hook_active":{active}}}"#,
        cwd.display()
    )
}

fn pr_ledger(e: &Env) {
    let f = e.home().join(".claude/gh-edit-allow/state/s1.ledger");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, "pr acme/x 5\n").unwrap();
}

fn pr_gate_blocked(e: &Env) {
    let f = e.home().join(".claude/pr-gate/state/s1.count");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, "2").unwrap();
}

fn project(e: &Env) -> PathBuf {
    repo(
        &e.path().join("proj"),
        Some("https://github.com/acme/x.git"),
    )
}

fn stop(e: &Env, p: &Path, active: bool) -> Run {
    run(e.gate(), &stop_input(p, active))
}

fn add_row(e: &Env, p: &Path, row: &str) -> Run {
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", p);
    run_args(c, &["--retro-add", SID, row])
}

fn close(e: &Env, target: &str) -> Run {
    run_args(e.gate(), &["--retro-close", SID, target])
}

fn gh_api_stub(e: &Env, body: &str, fail: bool) {
    let script = if fail {
        "#!/bin/sh\necho 'HTTP 404' >&2\nexit 1\n".to_string()
    } else {
        format!("#!/bin/sh\ncat <<'X'\n{body}\nX\n")
    };
    write_exec(&e.bin_dir().join("gh"), &script);
}

const URL: &str = "https://github.com/acme/x/pull/5#issuecomment-99";

#[test]
fn no_pr_means_no_retro() {
    let e = Env::new();
    let p = project(&e);
    let r = stop(&e, &p, false);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn pr_created_blocks_until_closed() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);

    // stop_hook_active でも抜けない(複数往復が要る)
    let r = stop(&e, &p, true);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains(r#"kind="retro""#));
    assert!(r.stderr.contains("not started"));
    assert!(r.stderr.contains("--retro-procedure"));

    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    let r = stop(&e, &p, false);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("rows recorded, not yet posted"));

    gh_api_stub(&e, &format!("{}\nbody", "<!-- wrapup-retro -->"), false);
    let r = close(&e, URL);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("read back acme/x comment 99"));
    let r = stop(&e, &p, false);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn vocabulary_is_enforced() {
    let e = Env::new();
    let p = project(&e);
    for bad in [
        "not json",
        r#"{"kind":"vibes","what":"w","evidence":"e","disposition":"inbox"}"#,
        r#"{"kind":"friction","what":"w","evidence":"e","disposition":"inbox"}"#,
        r#"{"kind":"friction","what":"w","evidence":"e","disposition":"inbox","mechanism":"later"}"#,
        r#"{"kind":"none","what":"w","evidence":"e","disposition":"inbox"}"#,
        r#"{"kind":"skipped","what":"w","evidence":"e","disposition":"none:r"}"#,
        r#"{"kind":"insight","what":"w","evidence":"e","disposition":"issue:#x"}"#,
    ] {
        let r = add_row(&e, &p, bad);
        assert_eq!(r.code, 64, "{bad}: {}", r.stderr);
    }
    let ledger = e.home().join(".local/state/claude/wrapup/retro/s1.jsonl");
    assert_eq!(lines(&ledger), 0);
    let ok = r#"{"kind":"skipped","what":"w","evidence":"e","disposition":"none:r","quote":"レトロは不要"}"#;
    assert_eq!(add_row(&e, &p, ok).code, 0);
    assert_eq!(lines(&ledger), 1);
}

#[test]
fn inbox_disposition_reaches_the_wrapup_inbox() {
    let e = Env::new();
    let p = project(&e);
    let row = r#"{"kind":"user-correction","what":"gate X leaked","evidence":"user: no","disposition":"inbox","mechanism":"existing:gate X"}"#;
    assert_eq!(add_row(&e, &p, row).code, 0);
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    let inbox = run_args(c, &["--inbox-path", p.to_str().unwrap()]).stdout;
    let body = read(Path::new(&inbox));
    assert!(body.contains(r#""title":"gate X leaked""#), "{body}");
    assert!(body.contains("mechanism: existing:gate X"), "{body}");
}

#[test]
fn deterministic_events_must_be_cited() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    pr_gate_blocked(&e);

    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    let r = stop(&e, &p, false);
    assert!(
        r.stderr.contains("1 event(s) not yet covered"),
        "{}",
        r.stderr
    );
    // 引かれていなければ close できない
    assert_eq!(close(&e, "none:no github target").code, 1);

    let cite = r#"{"kind":"gate-hit","what":"pr-gate blocked","evidence":"pr-gate blocked twice","disposition":"none:handled","mechanism":"none:fine"}"#;
    assert_eq!(add_row(&e, &p, cite).code, 0);
    let r = stop(&e, &p, false);
    assert!(
        r.stderr.contains("rows recorded, not yet posted"),
        "{}",
        r.stderr
    );
}

#[test]
fn skipped_row_waives_events_but_still_needs_close() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    pr_gate_blocked(&e);
    let skip = r#"{"kind":"skipped","what":"user declined","evidence":"user said so","disposition":"none:declined","quote":"今回は省略で"}"#;
    assert_eq!(add_row(&e, &p, skip).code, 0);
    let r = stop(&e, &p, false);
    assert!(
        r.stderr.contains("rows recorded, not yet posted"),
        "{}",
        r.stderr
    );
    let r = close(&e, "none:no github target");
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("unconfirmed"));
}

#[test]
fn block_count_is_bounded() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    for _ in 0..3 {
        assert_eq!(stop(&e, &p, false).code, 2);
    }
    let r = stop(&e, &p, false);
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains("blocked 3 times"), "{}", r.stderr);
    // 警告は 1 回だけ
    let r = stop(&e, &p, false);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn close_reads_the_comment_back() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    assert_eq!(close(&e, URL).code, 1); // 行が無い
    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);

    assert_eq!(close(&e, "https://example.com/x").code, 1); // URL 形式違い
    gh_api_stub(&e, "", true);
    assert_eq!(close(&e, URL).code, 3); // 読み戻せない
    gh_api_stub(&e, "a comment without the marker", false);
    assert_eq!(close(&e, URL).code, 1); // 目印が無い
    assert_eq!(stop(&e, &p, false).code, 2); // どれも完了扱いにならない
}

#[test]
fn procedure_lists_events_and_user_messages() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    pr_gate_blocked(&e);
    let tr = e.path().join("transcript.jsonl");
    std::fs::write(
        &tr,
        concat!(
            r#"{"type":"user","message":{"content":"それは違う、こうして"}}"#,
            "\n",
            r#"{"type":"user","promptSource":"system","origin":{"kind":"task-notification"},"message":{"content":"通知です"}}"#,
            "\n",
        ),
    )
    .unwrap();
    let input = format!(
        r#"{{"cwd":"{}","session_id":"{SID}","transcript_path":"{}"}}"#,
        p.display(),
        tr.display()
    );
    assert_eq!(run(e.gate(), &input).code, 2);

    let r = run_args(e.gate(), &["--retro-procedure", SID]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("[TODO] pr-gate"), "{}", r.stdout);
    assert!(r.stdout.contains("それは違う、こうして"), "{}", r.stdout);
    assert!(!r.stdout.contains("通知です"));
    assert!(r.stdout.contains("<!-- wrapup-retro -->"));
    assert!(r.stdout.contains("Generated with [Claude Code]"));
}

#[test]
fn procedure_without_transcript_says_so() {
    let e = Env::new();
    let r = run_args(e.gate(), &["--retro-procedure", SID]);
    assert!(r.stdout.contains("not available: no transcript path"));
}

#[test]
fn branch_fallback_for_agents_without_the_ledger() {
    let e = Env::new();
    let p = project(&e);
    git(
        &p,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "--allow-empty",
            "-qm",
            "x",
        ],
    );
    let mut c = e.gate();
    c.args(["--stamp-feedback-session", SID]);
    assert_eq!(run(c, "").code, 0);

    // セッション開始前に作られた PR は数えない
    write_exec(
        &e.bin_dir().join("gh"),
        "#!/bin/sh\necho 2000-01-01T00:00:00Z\n",
    );
    assert_eq!(stop(&e, &p, false).code, 0);
    // 開始後に作られた PR は数える
    write_exec(
        &e.bin_dir().join("gh"),
        "#!/bin/sh\necho 2999-01-01T00:00:00Z\n",
    );
    assert_eq!(stop(&e, &p, false).code, 2);
}
