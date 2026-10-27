//! Stop hook が、作業の途中(バックグラウンド処理の待ち)に、振り返りと feedback の指示を
//! 毎ターン出さないことの統合テスト(#854)。気づきの inbox は Stop の指示を出さない
//! (ADR-1136)ので、`/backlog-cleanup` の実行中の印(#820)は無くなった。
//!
//! - #854: 複数の PR を積みながら実機の長い実行を待つあいだ、`wrapup-stop-gate` の
//!   振り返りの指示がターンごとに届いた(5 回以上)。作業は終わっておらず、振り返りの
//!   対象は「セッション全体」なので、途中で行うとやり直しになる。

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SID: &str = "s1";
const LINE: &str = r#"{"ts":"2026-10-21T00:00:00+09:00","title":"裁定待ち","detail":"d"}"#;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// UNIX 秒を `YYYY-MM-DDTHH:MM:SS.000Z` にする(transcript の `timestamp` の形)。
fn iso(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn project(e: &Env) -> PathBuf {
    repo(
        &e.path().join("proj"),
        Some("https://github.com/acme/x.git"),
    )
}

fn inbox(e: &Env, p: &Path) -> String {
    let r = run_args(e.gate(), &["--inbox-path", p.to_str().unwrap()]);
    assert_eq!(r.code, 0);
    r.stdout
}

fn add_row(e: &Env, inbox: &str) {
    assert_eq!(run_args(e.gate(), &["--add", inbox, LINE]).code, 0);
}

/// PR を作ったセッション(振り返りが必要)にする。
fn pr_ledger(e: &Env) {
    let f = e.home().join(".claude/gh-edit-allow/state/s1.ledger");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, "pr acme/x 5\n").unwrap();
}

fn retro_count(e: &Env) -> Option<String> {
    std::fs::read_to_string(e.home().join(".local/state/claude/wrapup/retro/s1.count")).ok()
}

fn stop_with_transcript(e: &Env, p: &Path, transcript: &Path) -> Run {
    let input = format!(
        r#"{{"cwd":"{}","session_id":"{SID}","stop_hook_active":false,"transcript_path":"{}"}}"#,
        p.display(),
        transcript.display()
    );
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", p);
    run(c, &input)
}

fn stop(e: &Env, p: &Path) -> Run {
    let mut c = e.gate();
    c.env("CLAUDE_PROJECT_DIR", p);
    run(c, &hook_input(p))
}

fn bash_started(id: &str, at: u64) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{}","toolUseResult":{{"backgroundTaskId":"{id}"}}}}"#,
        iso(at)
    ) + "\n"
}

fn notified(id: &str, at: u64) -> String {
    format!(
        r#"{{"type":"attachment","timestamp":"{}","attachment":{{"type":"queued_command","prompt":"<task-notification>\n<task-id>{id}</task-id>\n<status>completed</status>\n</task-notification>"}}}}"#,
        iso(at)
    ) + "\n"
}

// --- #854: バックグラウンド処理を待つターンでは発火しない ---

#[test]
fn waiting_turn_does_not_fire_retro_and_does_not_spend_the_counter() {
    let e = Env::new();
    let p = project(&e);
    pr_ledger(&e);
    let tr = e.path().join("t.jsonl");

    // 実機の長い実行を背景で待っている(開始のみ)。
    std::fs::write(&tr, bash_started("b1", now() - 30)).unwrap();
    let r = stop_with_transcript(&e, &p, &tr);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""), "待ちターンは素通り");
    assert_eq!(
        retro_count(&e),
        None,
        "待ちターンは上限 3 回のカウンタを消費しない"
    );

    // 完了通知が届いたあとは、従来どおり振り返りを求める(対照)。
    std::fs::write(
        &tr,
        bash_started("b1", now() - 30) + &notified("b1", now() - 5),
    )
    .unwrap();
    let r = stop_with_transcript(&e, &p, &tr);
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(r.stderr.contains(r#"kind="retro""#), "{}", r.stderr);
    assert_eq!(retro_count(&e).as_deref(), Some("1"));
}

#[test]
fn waiting_turn_does_not_fire_the_feedback_instruction_either() {
    let e = Env::new();
    let p = project(&e);
    arm_feedback(&e, SID);
    let tr = e.path().join("t.jsonl");

    std::fs::write(&tr, bash_started("b1", now() - 30)).unwrap();
    let r = stop_with_transcript(&e, &p, &tr);
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));

    // 対照: 待っていなければ、従来どおり feedback の指示が出る。
    std::fs::write(&tr, "").unwrap();
    let r = stop_with_transcript(&e, &p, &tr);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("type: feedback"), "{}", r.stderr);
}

#[test]
fn a_task_that_never_finishes_does_not_silence_the_gate_forever() {
    // 終わらない監視(`tail -F` など)。1 時間より古い開始は、待ちとして数えない。
    let e = Env::new();
    let p = project(&e);
    arm_feedback(&e, SID);
    let tr = e.path().join("t.jsonl");
    std::fs::write(&tr, bash_started("old", now() - 2 * 3600)).unwrap();
    let r = stop_with_transcript(&e, &p, &tr);
    assert_eq!(r.code, 2, "{}", r.stderr);
}

// --- ADR-1136: 溜まった気づきは Stop を止めない ---

#[test]
fn unadjudicated_findings_never_hold_the_stop_even_across_turns() {
    let e = Env::new();
    let p = project(&e);
    let ib = inbox(&e, &p);
    add_row(&e, &ib);
    for _ in 0..3 {
        let r = stop(&e, &p);
        assert_eq!((r.code, r.stderr.as_str()), (0, ""), "毎ターン素通り");
    }
    // 行は消えない(裁定を待つ)。
    assert_eq!(lines(Path::new(&ib)), 1);
}
