//! 作業終了時のレトロ(`src/retro.rs`)の統合テスト。
//!
//! PR 作成の判定は gh-edit-allow の台帳 `~/.claude/gh-edit-allow/state/<sid>.ledger`
//! (HOME は隔離済み)に `pr ...` 行を置いて作る。完了(`--retro-close`)は、台帳から vault の
//! `retro/` のノートを書き出して読み戻す。vault は、テストごとの一時ディレクトリを
//! `OBSIDIAN_VAULT` で明示したときだけ宣言される(実機の vault には書かない)。

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

fn close(e: &Env) -> Run {
    run_args(e.gate(), &["--retro-close", SID])
}

/// vault を宣言した `--retro-close`。作業中のプロジェクトは `p`。
fn close_in(e: &Env, vault: &Path, p: &Path, sid: &str) -> Run {
    let mut c = e.gate();
    c.env("OBSIDIAN_VAULT", vault).env("CLAUDE_PROJECT_DIR", p);
    run_args(c, &["--retro-close", sid])
}

fn vault(e: &Env) -> PathBuf {
    let v = e.path().join("vault");
    std::fs::create_dir_all(v.join(".obsidian")).unwrap();
    std::fs::write(v.join(".obsidian/app.json"), "{}").unwrap();
    v
}

/// `retro/<年>/` 配下のノートを、パスの昇順で返す。
fn retro_notes(v: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(years) = std::fs::read_dir(v.join("retro")) {
        for y in years.flatten() {
            for f in std::fs::read_dir(y.path()).unwrap().flatten() {
                out.push(f.path());
            }
        }
    }
    out.sort();
    out
}

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
    assert!(r.stderr.contains("rows recorded, not yet closed"));

    // vault の無い機体: 台帳に残したまま完了になる(PR にはコメントしない)。
    let r = close(&e);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("ledger only"), "{}", r.stdout);
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
        // user-correction は feedback。inbox(vault のノート)には置けず、Issue か none:<理由>。
        r#"{"kind":"user-correction","what":"w","evidence":"e","disposition":"inbox","mechanism":"gate"}"#,
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
    let row = r#"{"kind":"friction","what":"gate X leaked","evidence":"user: no","disposition":"inbox","mechanism":"existing:gate X"}"#;
    assert_eq!(add_row(&e, &p, row).code, 0);
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    let inbox = run_args(c, &["--inbox-path", p.to_str().unwrap()]).stdout;
    let body = read(Path::new(&inbox));
    assert!(body.contains(r#""title":"gate X leaked""#), "{body}");
    assert!(body.contains("mechanism: existing:gate X"), "{body}");
}

/// user-correction は feedback。Issue(`issue:#N`)か `none:<理由>` で記録でき、inbox には流れない。
#[test]
fn a_user_correction_is_recorded_against_an_issue_not_the_inbox() {
    let e = Env::new();
    let p = project(&e);
    let row = r#"{"kind":"user-correction","what":"gate X leaked","evidence":"user: no","disposition":"issue:#12","mechanism":"gate","signal":"repeated-violation","existing":"拡張:pr-gate","searched":"pipeline-map,crates,skills"}"#;
    assert_eq!(add_row(&e, &p, row).code, 0);
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    let inbox = run_args(c, &["--inbox-path", p.to_str().unwrap()]).stdout;
    assert!(
        !Path::new(&inbox).exists(),
        "Issue 化した行は inbox に流さない"
    );
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
    assert_eq!(close(&e).code, 1);

    let cite = r#"{"kind":"gate-hit","what":"pr-gate blocked","evidence":"pr-gate blocked twice","disposition":"none:handled","mechanism":"none:fine"}"#;
    assert_eq!(add_row(&e, &p, cite).code, 0);
    let r = stop(&e, &p, false);
    assert!(
        r.stderr.contains("rows recorded, not yet closed"),
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
        r.stderr.contains("rows recorded, not yet closed"),
        "{}",
        r.stderr
    );
    let r = close(&e);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("ledger only"));
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
fn close_writes_one_note_into_the_vault_and_reads_it_back() {
    let e = Env::new();
    let (p, v) = (project(&e), vault(&e));
    pr_ledger(&e);
    assert_eq!(close_in(&e, &v, &p, SID).code, 1); // 行が無い
    let rows = [
        r#"{"kind":"friction","what":"a | b","evidence":"e","disposition":"inbox","mechanism":"prose","signal":"unclassified"}"#,
        r#"{"kind":"user-correction","what":"直された","evidence":"user: 違う","disposition":"issue:#12","mechanism":"gate","signal":"repeated-violation","existing":"拡張:pr-gate","searched":"pipeline-map,crates,skills"}"#,
        ROW_OK,
    ];
    for r in rows {
        assert_eq!(add_row(&e, &p, r).code, 0);
    }

    let r = close_in(&e, &v, &p, SID);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stdout.starts_with("retro closed (note: retro/"),
        "{}",
        r.stdout
    );
    let notes = retro_notes(&v);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let name = notes[0].file_name().unwrap().to_string_lossy().into_owned();
    // 日付(JST)・owner-repo・PR 番号(台帳の `pr acme/x 5`)。
    assert!(name.ends_with("-acme-x-5.md"), "{name}");
    let text = read(&notes[0]);
    assert!(
        text.starts_with("---\nrepo: \"acme/x\"\npr: \"5\"\nsession: \"s1\"\ndate: \""),
        "{text}"
    );
    // kind ごとに、user-correction → friction → insight の順で表になる。
    let pos = |h: &str| text.find(h).unwrap_or_else(|| panic!("{h}\n{text}"));
    assert!(pos("## user-correction") < pos("## friction"));
    assert!(pos("## friction") < pos("## insight"));
    assert!(text.contains("| a \\| b | e | inbox | prose |"), "{text}");
    // PR にはコメントしない(gh は一度も呼ばれない)。
    assert_eq!(read(&e.gh_log()), "");
    // 閉じたので、Stop は通る。閉じ直しても同じノートで、内容は変わらない。
    assert_eq!(stop(&e, &p, false).code, 0);
    assert_eq!(close_in(&e, &v, &p, SID).code, 0);
    assert_eq!(retro_notes(&v), notes);
    assert_eq!(read(&notes[0]), text);
}

#[test]
fn another_session_on_the_same_pr_does_not_overwrite_the_note() {
    let e = Env::new();
    let (p, v) = (project(&e), vault(&e));
    pr_ledger(&e);
    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    assert_eq!(close_in(&e, &v, &p, SID).code, 0);

    // 別のセッション(同じ PR、同じ日)。
    let f = e.home().join(".claude/gh-edit-allow/state/s2.ledger");
    std::fs::write(f, "pr acme/x 5\n").unwrap();
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", &p);
    assert_eq!(run_args(c, &["--retro-add", "s2", ROW_OK]).code, 0);
    assert_eq!(close_in(&e, &v, &p, "s2").code, 0);
    let notes = retro_notes(&v);
    assert_eq!(notes.len(), 2, "{notes:?}");
    let sessions: Vec<bool> = notes
        .iter()
        .map(|n| read(n).contains("session: \"s1\""))
        .collect();
    assert!(
        sessions.contains(&true) && sessions.contains(&false),
        "{sessions:?}"
    );
}

#[test]
fn without_a_pr_the_note_is_named_by_the_session() {
    let e = Env::new();
    let (p, v) = (project(&e), vault(&e));
    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    assert_eq!(close_in(&e, &v, &p, SID).code, 0);
    let notes = retro_notes(&v);
    assert_eq!(notes.len(), 1);
    let name = notes[0].file_name().unwrap().to_string_lossy().into_owned();
    // PR が分からなければ、番号の代わりに `s<session の先頭 8 文字>`。repo は origin から。
    assert!(name.ends_with("-acme-x-ss1.md"), "{name}");
}

#[test]
fn an_unusable_vault_never_wedges_the_retro() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    let gone = e.path().join("no-such-vault");
    let r = close_in(&e, &gone, &p, SID);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("vault を使えない"), "{}", r.stderr);
    assert!(r.stdout.contains("ledger only"), "{}", r.stdout);
    assert_eq!(stop(&e, &p, false).code, 0);
}

#[test]
fn an_old_style_url_argument_is_ignored_with_a_warning() {
    let e = Env::new();
    let p = project(&e);
    assert_eq!(add_row(&e, &p, ROW_OK).code, 0);
    let r = run_args(
        e.gate(),
        &[
            "--retro-close",
            SID,
            "https://github.com/acme/x/pull/5#issuecomment-99",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("ignoring"), "{}", r.stderr);
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
    // PR コメントは投稿しない。完了は --retro-close(vault のノート)。
    assert!(r.stdout.contains("--retro-close"), "{}", r.stdout);
    assert!(
        r.stdout.contains("Nothing is posted to GitHub"),
        "{}",
        r.stdout
    );
    assert!(!r.stdout.contains("gh pr comment"), "{}", r.stdout);
    assert!(!r.stdout.contains("wrapup-retro"), "{}", r.stdout);
    // user-correction は inbox に置けない。
    assert!(r.stdout.contains("never kept in the inbox"), "{}", r.stdout);
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
