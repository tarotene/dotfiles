//! `worklog mail` の統合テスト: 一時の vault と終端の控えで実バイナリを走らせ、
//! 状態機械(config/agents/mail-lifecycle/vocabulary.json)を通しで確かめる。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 2026-10-31 09:00 JST(= 日数 20757)。
const T0: u64 = 1_793_404_800;
const DAY: u64 = 86_400;

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    vault: PathBuf,
    state: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let vault = root.join("vault");
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    std::fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    std::fs::write(
        vault.join(".obsidian/daily-notes.json"),
        r#"{"folder":"journal","format":"YYYY/YYYY-MM-DD"}"#,
    )
    .unwrap();
    Env {
        state: root.join("state"),
        _dir: dir,
        root,
        vault,
    }
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `days_after` は T0 から進める日数(時計)。
fn run(e: &Env, days_after: u64, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_worklog"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", &e.root)
        .env("XDG_CONFIG_HOME", e.root.join("config"))
        .env("OBSIDIAN_VAULT", &e.vault)
        .env("WORKLOG_STATE_DIR", &e.state)
        .env("WORKLOG_NOW", (T0 + days_after * DAY).to_string())
        .current_dir(&e.root)
        .args(args)
        .output()
        .unwrap();
    Out {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn add(e: &Env, days_after: u64, line: &str) -> Out {
    run(e, days_after, &["add", "mail", line])
}

fn note(e: &Env, date: &str) -> String {
    std::fs::read_to_string(note_path(e, date)).unwrap_or_default()
}

fn note_path(e: &Env, date: &str) -> PathBuf {
    e.vault.join(format!("journal/{}/{date}.md", &date[..4]))
}

fn ledger(e: &Env) -> String {
    std::fs::read_to_string(e.state.join("mail-terminal")).unwrap_or_default()
}

fn ok(o: &Out) {
    assert_eq!(o.code, 0, "stdout={} stderr={}", o.stdout, o.stderr);
}

fn rejected(o: &Out) {
    assert_eq!(o.code, 2, "stdout={} stderr={}", o.stdout, o.stderr);
}

const K1: &str = "gmail:t1/m1";

#[test]
fn an_item_walks_the_states_and_the_terminal_state_stops_the_writer() {
    let e = env();
    ok(&add(
        &e,
        0,
        "ask: 判断の表に載せた [gmail:t1/m1 due:2026-11-28]",
    ));
    // 開いていない項目に、開く以外の出来事は書けない。
    rejected(&add(&e, 0, "apply: 開いていない [gmail:t9/m9]"));
    ok(&add(&e, 0, "decide: やると決めた [gmail:t1/m1]"));
    // decided から decide は許されない。
    rejected(&add(&e, 0, "decide: もう一度 [gmail:t1/m1]"));
    let open = run(&e, 0, &["mail", "open"]);
    ok(&open);
    assert!(
        open.stdout
            .starts_with("decided\tgmail:t1/m1\t0日\t2026-11-28\t"),
        "{}",
        open.stdout
    );

    ok(&add(&e, 1, "apply: 設定した [gmail:t1/m1]"));
    // 終端のあとは何も書けず、読み直し(開く出来事)は記録済みになる。
    rejected(&add(&e, 1, "pass: 後から断る [gmail:t1/m1]"));
    let again = add(&e, 1, "ask: 読み直し [gmail:t1/m1]");
    ok(&again);
    assert!(again.stdout.starts_with("既に記録済み"), "{}", again.stdout);

    assert_eq!(run(&e, 1, &["mail", "open"]).stdout, "");
    assert_eq!(run(&e, 1, &["mail", "check"]).code, 0);
    assert!(
        ledger(&e).contains(r#""key":"gmail:t1/m1""#) && ledger(&e).contains(r#""state":"closed""#)
    );
    let n = note(&e, "2026-10-31");
    assert_eq!(n.matches("[mail]").count(), 2, "{n}");
}

#[test]
fn re_reading_a_receipt_never_duplicates_and_a_new_receipt_supersedes_the_old_one() {
    let e = env();
    ok(&add(
        &e,
        0,
        "ask: 紹介会社の初回 [linkedin:c1/20261029T2307]",
    ));
    let again = add(
        &e,
        0,
        "ask: 別の言い回しで読み直し [linkedin:c1/20261029T2307]",
    );
    assert!(again.stdout.starts_with("既に記録済み"), "{}", again.stdout);
    assert_eq!(note(&e, "2026-10-31").matches("[mail]").count(), 1);

    // 同じ会話に新しい受信。古い項目が同じ命令の中で先に移る。
    let next = add(
        &e,
        2,
        "draft: 2 通目への返信を用意 [linkedin:c1/20261031T1000]",
    );
    ok(&next);
    assert!(
        next.stdout
            .contains("古い項目を移した: linkedin:c1/20261029T2307"),
        "{}",
        next.stdout
    );
    let n = note(&e, "2026-11-02");
    assert!(
        n.contains(
            "move: 紹介会社の初回 [linkedin:c1/20261029T2307 to:mail:linkedin:c1/20261031T1000]"
        ),
        "{n}"
    );
    let open = run(&e, 2, &["mail", "open"]);
    assert_eq!(open.stdout.lines().count(), 1, "{}", open.stdout);
    assert!(
        open.stdout
            .starts_with("awaiting\tlinkedin:c1/20261031T1000\t"),
        "{}",
        open.stdout
    );
    // 別のスレッドには影響しない。
    ok(&add(&e, 2, "ask: 別の会話 [linkedin:c2/20261101T0900]"));
    assert_eq!(run(&e, 2, &["mail", "open"]).stdout.lines().count(), 2);
}

#[test]
fn machine_events_are_written_only_by_sweep_and_stale_always_follows_a_remind() {
    let e = env();
    ok(&add(&e, 0, "ask: 方針を決める [gmail:t1/m1]"));
    // remind / stale を手で書くことはできない。
    rejected(&add(&e, 0, "remind: 手で書く [gmail:t1/m1]"));
    rejected(&add(&e, 0, "stale: 手で書く [gmail:t1/m1]"));

    // 6 日目は何も無い。dry-run は書かない。
    assert_eq!(run(&e, 6, &["mail", "sweep"]).stdout, "");
    let dry = run(&e, 7, &["mail", "sweep", "--dry-run"]);
    ok(&dry);
    assert!(
        dry.stdout.starts_with("remind\tgmail:t1/m1\t"),
        "{}",
        dry.stdout
    );
    assert_eq!(note(&e, "2026-11-07"), "");

    // 7 日目: remind。同じ日の再実行は何もしない。
    let s = run(&e, 7, &["mail", "sweep"]);
    ok(&s);
    assert!(s.stdout.starts_with("remind\t"), "{}", s.stdout);
    assert!(note(&e, "2026-11-07").contains("[mail] remind: 方針を決める [gmail:t1/m1]"));
    assert_eq!(run(&e, 7, &["mail", "sweep"]).stdout, "");
    // 13 日目まで stale にならず、14 日目に stale(控えにも載る)。
    assert_eq!(run(&e, 13, &["mail", "sweep"]).stdout, "");
    let stale = run(&e, 14, &["mail", "sweep"]);
    assert!(stale.stdout.starts_with("stale\t"), "{}", stale.stdout);
    assert!(ledger(&e).contains(r#""state":"stale""#));
    assert_eq!(run(&e, 14, &["mail", "open"]).stdout, "");
    assert_eq!(run(&e, 30, &["mail", "sweep"]).stdout, "");
}

#[test]
fn a_decision_resets_the_clock_and_a_due_date_triggers_an_early_remind() {
    let e = env();
    ok(&add(
        &e,
        0,
        "ask: パスキーを設定 [gmail:t1/m1 due:2026-11-10]",
    ));
    // 期限の 3 日前(11-07)に remind。
    assert_eq!(run(&e, 6, &["mail", "sweep"]).stdout, "");
    let early = run(&e, 7, &["mail", "sweep"]);
    assert!(
        early.stdout.starts_with("remind\t") && early.stdout.contains("due:2026-11-10"),
        "{}",
        early.stdout
    );
    // 期限の翌日に stale。
    assert_eq!(run(&e, 10, &["mail", "sweep"]).stdout, "");
    assert!(run(&e, 11, &["mail", "sweep"])
        .stdout
        .starts_with("stale\t"));
}

#[test]
fn a_deleted_terminal_line_is_detected_not_reminded_and_can_be_restored() {
    let e = env();
    ok(&add(&e, 0, "ask: 設定する [gmail:t1/m1]"));
    ok(&add(&e, 0, "apply: 設定した [gmail:t1/m1]"));
    assert_eq!(run(&e, 0, &["mail", "check"]).code, 0);

    // 本人が apply の行を手で消した。
    let path = note_path(&e, "2026-10-31");
    let kept: String = note(&e, "2026-10-31")
        .lines()
        .filter(|l| !l.contains("apply:"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&path, kept).unwrap();

    let check = run(&e, 20, &["mail", "check"]);
    assert_eq!(check.code, 1, "{}", check.stdout);
    assert!(
        check.stdout.contains("終端の行が消えた"),
        "{}",
        check.stdout
    );
    // 未了に戻って見えても、再通知(sweep)には回さず、机には「消えた」と出す。
    assert_eq!(run(&e, 20, &["mail", "sweep"]).stdout, "");
    let open = run(&e, 20, &["mail", "open"]);
    assert!(
        open.stdout.starts_with("終端の行が消えた\tgmail:t1/m1\t"),
        "{}",
        open.stdout
    );

    // 復元すると、控えの終端と同じ行が書き戻り、食い違いは消える。
    ok(&run(&e, 20, &["mail", "restore", K1]));
    assert!(note(&e, "2026-11-20").contains("[mail] apply: 設定した [gmail:t1/m1]"));
    assert_eq!(run(&e, 20, &["mail", "check"]).code, 0);
}

#[test]
fn forgetting_a_ledger_entry_closes_the_discrepancy_without_a_line() {
    let e = env();
    ok(&add(&e, 0, "read: 読んだだけ [gmail:t1/m1]"));
    std::fs::write(note_path(&e, "2026-10-31"), "## 作業ログ\n").unwrap();
    assert_eq!(run(&e, 1, &["mail", "check"]).code, 1);
    ok(&run(&e, 1, &["mail", "forget", K1]));
    assert_eq!(run(&e, 1, &["mail", "check"]).code, 0);
    // 控えに無い鍵は restore も forget もできない。
    rejected(&run(&e, 1, &["mail", "restore", "gmail:none/none"]));
}

#[test]
fn move_needs_a_destination_and_rejects_unknown_forms() {
    let e = env();
    ok(&add(&e, 0, "ask: 予算の方針 [gmail:t1/m1]"));
    rejected(&add(&e, 0, "move: 移し先が無い [gmail:t1/m1]"));
    rejected(&add(&e, 0, "move: 移し先が違う [gmail:t1/m1 to:somewhere]"));
    ok(&add(
        &e,
        0,
        "move: Issue に移した [gmail:t1/m1 to:example/repo#564]",
    ));
    assert_eq!(run(&e, 0, &["mail", "open"]).stdout, "");
    assert!(ledger(&e).contains(r#""state":"superseded""#));
}

#[test]
fn lines_before_the_cutover_use_the_old_grammar_and_are_not_folded() {
    let e = env();
    let old = e.vault.join("journal/2026/2026-10-29.md");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(
        &old,
        "## 作業ログ\n- 23:46 [mail] pass: UNISEC 英語コースは受けない [gmail:1a167ca8a1f04a89]\n",
    )
    .unwrap();
    assert_eq!(run(&e, 0, &["mail", "check"]).code, 0);
    assert_eq!(run(&e, 0, &["mail", "open"]).stdout, "");
    // 旧い行は list では今までどおり読める。
    let l = run(
        &e,
        0,
        &[
            "list",
            "--kind",
            "mail",
            "--since",
            "2026-10-29",
            "--until",
            "2026-10-29",
        ],
    );
    assert!(l.stdout.contains("UNISEC"), "{}", l.stdout);
}

/// `MAIL_SINCE` から今日までが `list` の上限(400 日)を超えても、読む範囲を切ってエラーにしない。
#[test]
fn a_clock_far_past_the_cutover_still_reads_within_the_range_limit() {
    let e = env();
    ok(&add(&e, 0, "ask: 昔に開いた項目 [gmail:old/m1]"));
    // 399 日後は、読む範囲の端(ちょうど MAIL_SINCE から 400 日を超える日)。
    for days in [399, 800, 2000] {
        for sub in ["open", "check", "sweep"] {
            let o = run(&e, days, &["mail", sub]);
            assert!(
                o.code == 0 || o.code == 1,
                "day +{days} mail {sub}: code={} stdout={} stderr={}",
                o.code,
                o.stdout,
                o.stderr
            );
            assert!(
                !o.stderr.contains("範囲が"),
                "day +{days} {sub}: {}",
                o.stderr
            );
        }
    }
    // 窓の中に開いた項目は、遠い日付でも読める。
    ok(&add(&e, 800, "ask: 最近開いた項目 [gmail:new/m1]"));
    let open = run(&e, 800, &["mail", "open"]);
    ok(&open);
    assert!(open.stdout.contains("gmail:new/m1"), "{}", open.stdout);
    // 窓より前に開いた項目は読まれない(slot と同じ)。
    assert!(!open.stdout.contains("gmail:old/m1"), "{}", open.stdout);
}

/// 本人担当の作業を `decide` したのに、Issue への `move` も `apply` も無いまま日をまたいだ項目を、
/// sweep が(行を書かずに)挙げる。
#[test]
fn sweep_flags_a_decided_item_with_no_move_after_the_day_it_was_decided() {
    let e = env();
    ok(&add(&e, 0, "ask: 本人が対応する [gmail:t1/m1]"));
    ok(&add(&e, 0, "decide: 対応する [gmail:t1/m1]"));
    ok(&add(&e, 0, "ask: 見せただけ [gmail:t2/m1]"));
    // 当日は猶予。
    let same_day = run(&e, 0, &["mail", "sweep"]);
    ok(&same_day);
    assert!(!same_day.stdout.contains("flag\t"), "{}", same_day.stdout);
    // 翌日は、decided のまま残る t1 だけを挙げる(awaiting の t2 は remind の担当)。
    let before = note(&e, "2026-11-01");
    let next_day = run(&e, 1, &["mail", "sweep"]);
    ok(&next_day);
    let flags: Vec<&str> = next_day
        .stdout
        .lines()
        .filter(|l| l.starts_with("flag\t"))
        .collect();
    assert_eq!(flags.len(), 1, "{}", next_day.stdout);
    assert!(flags[0].starts_with("flag\tgmail:t1/m1\t"), "{}", flags[0]);
    // 行は書かない。
    assert_eq!(note(&e, "2026-11-01"), before);
    // move すると挙がらなくなる。
    ok(&add(
        &e,
        1,
        "move: Issue に移した [gmail:t1/m1 to:example/repo#1]",
    ));
    let after = run(&e, 2, &["mail", "sweep"]);
    ok(&after);
    assert!(!after.stdout.contains("flag\t"), "{}", after.stdout);
}

#[test]
fn lines_after_the_cutover_that_do_not_parse_are_reported() {
    let e = env();
    let p = note_path(&e, "2026-10-31");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "## 作業ログ\n- 09:00 [mail] read: 旧い文法の行 [gmail:x]\n",
    )
    .unwrap();
    let c = run(&e, 0, &["mail", "check"]);
    assert_eq!(c.code, 1);
    assert!(c.stdout.contains("解析できない行"), "{}", c.stdout);
}

#[test]
fn aggregate_lines_are_not_items() {
    let e = env();
    ok(&add(&e, 0, "read: 約 490 件を読んだ [none]"));
    rejected(&add(&e, 0, "apply: none は read だけ [none]"));
    assert_eq!(run(&e, 30, &["mail", "sweep"]).stdout, "");
    assert_eq!(run(&e, 30, &["mail", "check"]).code, 0);
    let _ = Path::new("");
}

#[test]
fn an_item_in_the_ledger_counts_as_recorded_so_re_reading_does_not_reopen_it() {
    let e = env();
    ok(&add(&e, 0, "read: 読んだだけ [gmail:t1/m1]"));
    // 開く行そのものが終端(read)で、その行が手で消された。
    std::fs::write(note_path(&e, "2026-10-31"), "## 作業ログ\n").unwrap();
    // 読み直しで未記録と見て開き直すと、消された終端が通常の未了に戻ってしまう。
    let again = add(&e, 1, "ask: 読み直して判断に出す [gmail:t1/m1]");
    ok(&again);
    assert!(again.stdout.starts_with("既に記録済み"), "{}", again.stdout);
    assert_eq!(run(&e, 1, &["mail", "open"]).stdout.lines().count(), 1);
    assert!(run(&e, 1, &["mail", "open"])
        .stdout
        .starts_with("終端の行が消えた"));
}

#[test]
fn a_late_older_receipt_does_not_supersede_the_newer_one() {
    let e = env();
    ok(&add(&e, 0, "ask: 新しい受信 [gmail:t1/m2]"));
    let late = add(&e, 0, "ask: 遅れて読んだ古い受信 [gmail:t1/m1]");
    ok(&late);
    // 新しい項目は移されない(古い方が畳み込みで移される)。
    assert!(!late.stdout.contains("古い項目を移した"), "{}", late.stdout);
    let open = run(&e, 0, &["mail", "open"]);
    assert_eq!(open.stdout.lines().count(), 1, "{}", open.stdout);
    assert!(open.stdout.contains("gmail:t1/m2"), "{}", open.stdout);
}

#[test]
fn a_send_decision_closes_only_after_confirm_and_instruct_hands_the_item_to_the_agent() {
    let e = env();
    ok(&add(&e, 0, "draft: 返信を用意 [gmail:t1/m1]"));
    // 「その他」の指示は instruct。項目はエージェントの番(instructed)になり、open に出る。
    ok(&add(&e, 0, "instruct: 来週の予定で返す [gmail:t1/m1]"));
    let open = run(&e, 0, &["mail", "open"]);
    ok(&open);
    assert!(
        open.stdout.starts_with("instructed\tgmail:t1/m1\t"),
        "{}",
        open.stdout
    );
    // instructed から instruct はもう書けない。decide は書ける(send の印つき)。
    rejected(&add(&e, 0, "instruct: もう一度 [gmail:t1/m1]"));
    // owner:・send: は decide だけ。send:yes は owner:agent と一緒に付ける。
    rejected(&add(
        &e,
        0,
        "apply: 印は decide だけ [gmail:t1/m1 send:yes]",
    ));
    rejected(&add(&e, 0, "decide: send だけ [gmail:t1/m1 send:yes]"));
    ok(&add(
        &e,
        0,
        "decide: おまかせで送る [gmail:t1/m1 owner:agent send:yes]",
    ));
    // confirm の前の apply は、書かずに止める(送信の前に、本人が画面・全文・宛先を見た記録が要る)。
    let early = add(&e, 0, "apply: 送った [gmail:t1/m1]");
    rejected(&early);
    assert!(early.stderr.contains("confirm"), "{}", early.stderr);
    ok(&add(
        &e,
        0,
        "confirm: 画面・全文・宛先を見て OK [gmail:t1/m1]",
    ));
    // 確認は 1 回だけ。続けて apply で閉じる。
    rejected(&add(&e, 0, "confirm: もう一度 [gmail:t1/m1]"));
    ok(&add(&e, 0, "apply: 送った [gmail:t1/m1]"));
    assert!(ledger(&e).contains(r#""state":"closed""#));
    assert_eq!(run(&e, 0, &["mail", "check"]).code, 0);

    // 送信でない decide(owner:agent だけ、印なし)には confirm を書けず、apply はそのまま通る。
    ok(&add(&e, 0, "ask: 別の件 [gmail:t2/m1]"));
    ok(&add(
        &e,
        0,
        "decide: 調べてもらう [gmail:t2/m1 owner:agent]",
    ));
    rejected(&add(&e, 0, "confirm: 要らない確認 [gmail:t2/m1]"));
    ok(&add(&e, 0, "apply: 報告した [gmail:t2/m1]"));
}

#[test]
fn an_agent_turn_item_is_reminded_sooner_than_a_person_turn_item() {
    let e = env();
    ok(&add(&e, 0, "ask: 本人の番 [gmail:t1/m1]"));
    ok(&add(&e, 0, "ask: 指示を待つ [gmail:t2/m1]"));
    ok(&add(&e, 0, "instruct: 調べて [gmail:t2/m1]"));
    // 3 日たつと、エージェントの番(instructed)だけが再通知される。本人の番は 7 日。
    let early = run(&e, 3, &["mail", "sweep", "--dry-run"]);
    ok(&early);
    assert!(
        early.stdout.contains("gmail:t2/m1") && !early.stdout.contains("gmail:t1/m1"),
        "{}",
        early.stdout
    );
    let later = run(&e, 7, &["mail", "sweep", "--dry-run"]);
    assert!(later.stdout.contains("gmail:t1/m1"), "{}", later.stdout);
}
