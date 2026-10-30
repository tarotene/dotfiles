//! `slot` kind の畳み込み・遷移・overdue の試験(config/agents/task-slot/vocabulary.json)。
//! 語彙 JSON から読んだ遷移表を実装が守ることを、表の全マスで確かめる。fixture は架空の値。

use worklog::slot::{self, parse_instant, SlotVocabulary};
use worklog::{Entry, Kind, Row, Vault};

const W1: &str = "start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00";
const W2: &str = "start:2026-11-03T10:00+09:00 end:2026-11-03T12:00+09:00";

fn row(date: &str, time: &str, text: &str) -> Row {
    Row {
        date: date.into(),
        entry: Entry {
            time: Some(time.into()),
            kind: Some(Kind::Slot),
            text: text.into(),
            reference: None,
        },
    }
}

fn fold_rows(rows: &[Row]) -> slot::Folded {
    let (evs, bad) = slot::collect(rows);
    assert!(bad.is_empty(), "{bad:?}");
    slot::fold(&evs)
}

/// 出来事 `event` の見本の行(鍵は `k`)。
fn sample(event: &str, key: &str) -> String {
    match event {
        "plan" => format!("plan: {key} [{W1}]"),
        "resched" => format!("resched: {key} [{W2}]"),
        "done" | "drop" => format!("{event}: {key}"),
        other => panic!("見本が無い出来事: {other}"),
    }
}

/// `state` に入るまでの行(`none` は何も無い)。
fn reach(state: &str, key: &str) -> Vec<String> {
    match state {
        "none" => vec![],
        "scheduled" => vec![sample("plan", key)],
        "done" => vec![sample("plan", key), sample("done", key)],
        "dropped" => vec![sample("plan", key), sample("drop", key)],
        other => panic!("到達の手順が無い状態: {other}"),
    }
}

fn rows_of(lines: &[String]) -> Vec<Row> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| row("2026-11-01", &format!("09:{i:02}"), l))
        .collect()
}

#[test]
fn every_cell_of_the_vocabulary_table_is_obeyed() {
    let v = SlotVocabulary::get();
    let mut states: Vec<&str> = vec!["none"];
    states.extend(v.states.iter().map(|(s, _)| s.as_str()));
    let mut cells = 0;
    for state in &states {
        for def in &v.events {
            let mut lines = reach(state, "k");
            let before = lines.len();
            lines.push(sample(&def.id, "k"));
            let f = fold_rows(&rows_of(&lines));
            // 許される遷移(from に現在の状態がある)だけが食い違いなく通り、行き先の状態になる。
            let allowed = def.from.iter().any(|x| x == state);
            assert_eq!(
                f.discrepancies.is_empty(),
                allowed,
                "{state} --{}--> : {:?}",
                def.id,
                f.discrepancies
            );
            let now = f.slots.get("k").map(|s| s.state.as_str());
            if allowed {
                assert_eq!(now, Some(def.to.as_str()), "{state} --{}-->", def.id);
            } else {
                // 弾かれた行は状態を変えない。
                let expected = (before > 0).then(|| fold_rows(&rows_of(&lines[..before])));
                assert_eq!(
                    now,
                    expected
                        .as_ref()
                        .and_then(|e| e.slots.get("k"))
                        .map(|s| s.state.as_str()),
                    "{state} --{}--> は状態を変えない",
                    def.id
                );
            }
            cells += 1;
        }
    }
    assert_eq!(cells, 4 * 4, "none と 3 状態 × 出来事 4");
}

#[test]
fn terminal_states_have_no_way_out_in_the_table() {
    let v = SlotVocabulary::get();
    for (s, terminal) in &v.states {
        if *terminal {
            assert!(
                v.events.iter().all(|e| !e.from.contains(s)),
                "終端 {s} から出る出来事がある"
            );
        }
    }
    // 終端の後に何を書いても、終端のまま。
    for end in ["done", "dropped"] {
        for def in &v.events {
            let mut lines = reach(end, "k");
            lines.push(sample(&def.id, "k"));
            let f = fold_rows(&rows_of(&lines));
            assert_eq!(f.slots["k"].state, end, "{end} の後の {}", def.id);
            assert_eq!(f.discrepancies.len(), 1, "{end} の後の {}", def.id);
        }
    }
}

#[test]
fn the_vocabulary_has_exactly_the_events_the_grammar_knows() {
    let v = SlotVocabulary::get();
    for def in &v.events {
        // 見本が文法を通る(語彙に出来事を足して文法を足し忘れると、ここで落ちる)。
        slot::parse_slot(&sample(&def.id, "k")).unwrap();
        assert_ne!(def.by, "machine", "{}", def.id);
    }
    assert_eq!(v.event_names(), "plan|resched|done|drop");
    // 人の裁定が要る出来事は person。plan だけが agent。
    for (id, by) in [
        ("plan", "agent"),
        ("resched", "person"),
        ("done", "person"),
        ("drop", "person"),
    ] {
        assert_eq!(v.event(id).unwrap().by, by, "{id}");
    }
}

#[test]
fn the_same_key_cannot_be_planned_twice_even_after_a_terminal_state() {
    for state in ["scheduled", "done", "dropped"] {
        let mut lines = reach(state, "k");
        lines.push(format!("plan: k [{W2}]"));
        let f = fold_rows(&rows_of(&lines));
        assert_eq!(f.discrepancies.len(), 1, "{state}: {:?}", f.discrepancies);
        // 最初の plan の時間枠が残る。
        assert_eq!(f.slots["k"].start.text, "2026-11-02T10:00+09:00");
    }
}

#[test]
fn resched_replaces_the_window_and_keeps_the_issues() {
    let f = fold_rows(&rows_of(&[
        format!("plan: k [{W1} issue:example/repo#1 issue:example/repo#2]"),
        sample("resched", "k"),
    ]));
    let s = &f.slots["k"];
    assert_eq!(s.state, "scheduled");
    assert_eq!(
        (s.start.text.as_str(), s.end.text.as_str()),
        ("2026-11-03T10:00+09:00", "2026-11-03T12:00+09:00")
    );
    assert_eq!(s.issues, ["example/repo#1", "example/repo#2"]);
    assert!(f.discrepancies.is_empty());
}

#[test]
fn fold_is_deterministic_whatever_order_the_rows_come_in() {
    // 時刻が違う行は、読み込みの順に依らず、時刻の順に畳み込まれる。
    let rows = vec![
        row("2026-11-01", "09:00", &sample("plan", "k")),
        row("2026-11-01", "09:10", &sample("resched", "k")),
        row("2026-11-02", "08:00", &sample("done", "k")),
        row("2026-11-01", "09:05", &sample("plan", "other")),
        row("2026-11-02", "08:30", &sample("drop", "other")),
    ];
    let key = |f: &slot::Folded| {
        f.slots
            .values()
            .map(|s| (s.key.clone(), s.state.clone(), s.start.text.clone()))
            .collect::<Vec<_>>()
    };
    let base = fold_rows(&rows);
    assert!(base.discrepancies.is_empty());
    assert_eq!(base.slots["k"].state, "done");
    assert_eq!(base.slots["other"].state, "dropped");
    // 全順列(5! = 120 通り)で同じ結果。
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let mut seen = 0;
    permute(&mut idx, 0, &mut |p| {
        let shuffled: Vec<Row> = p.iter().map(|&i| rows[i].clone()).collect();
        let f = fold_rows(&shuffled);
        assert_eq!(key(&f), key(&base), "{p:?}");
        assert_eq!(f.discrepancies.len(), 0, "{p:?}");
        seen += 1;
    });
    assert_eq!(seen, 120);
}

fn permute(a: &mut Vec<usize>, k: usize, f: &mut impl FnMut(&[usize])) {
    if k == a.len() {
        f(a);
        return;
    }
    for i in k..a.len() {
        a.swap(k, i);
        permute(a, k + 1, f);
        a.swap(k, i);
    }
}

#[test]
fn a_line_that_does_not_parse_is_a_discrepancy_not_a_panic() {
    let (evs, bad) = slot::collect(&[
        row("2026-11-01", "09:00", "メモ書き"),
        row("2026-11-01", "09:01", "plan: BAD KEY [x]"),
        row("2026-11-01", "09:02", &sample("plan", "k")),
    ]);
    assert_eq!(evs.len(), 1);
    assert_eq!(bad.len(), 2);
    assert!(bad[0].message.contains("解析できない行"));
}

#[test]
fn overdue_is_derived_from_the_end_time_and_never_stored() {
    let mk = |k: &str, s: &str, e: &str| format!("plan: {k} [start:{s} end:{e}]");
    let f = fold_rows(&rows_of(&[
        mk("late-b", "2026-11-02T10:00+09:00", "2026-11-02T12:00+09:00"),
        mk("late-a", "2026-11-01T10:00+09:00", "2026-11-01T11:00+09:00"),
        mk("later", "2026-11-04T10:00+09:00", "2026-11-04T12:00+09:00"),
        mk("zone", "2026-11-02T09:00+09:00", "2026-11-02T12:00+09:00"),
        mk("fin", "2026-10-31T10:00+09:00", "2026-10-31T11:00+09:00"),
        "done: fin".to_string(),
    ]));
    let now = parse_instant("2026-11-03T00:00+09:00").unwrap().epoch;
    let keys = |v: Vec<&slot::Slot>| v.iter().map(|s| s.key.clone()).collect::<Vec<_>>();
    // 終了の古い順。同じ終了(`late-b` と `zone`)は鍵の順。済んだ枠と、これからの枠は含まない。
    assert_eq!(keys(f.overdue(now)), ["late-a", "late-b", "zone"]);
    // 終了時刻ちょうどは、まだ過ぎていない。
    let at_end = parse_instant("2026-11-01T11:00+09:00").unwrap().epoch;
    assert!(f.overdue(at_end).is_empty());
    assert_eq!(keys(f.overdue(at_end + 1)), ["late-a"]);
    // オフセットが違っても、時刻は同じ瞬間として比べる(02:00Z = 11:00+09:00)。
    let z = parse_instant("2026-11-01T02:00:01Z").unwrap().epoch;
    assert_eq!(keys(f.overdue(z)), ["late-a"]);
    // 未了の一覧は開始の早い順(fin は終端なので含まない)。
    assert_eq!(
        keys(f.open()),
        ["late-a", "zone", "late-b", "later"],
        "開始の早い順、同じなら鍵の順"
    );
    // 状態は scheduled のまま。overdue という状態は無い。
    assert!(f.slots.values().all(|s| s.state != "overdue"));
    assert!(SlotVocabulary::get()
        .states
        .iter()
        .all(|(s, _)| s != "overdue"));
}

#[test]
fn resched_moves_a_slot_out_of_overdue_and_done_or_drop_clears_it() {
    let now = parse_instant("2026-11-05T00:00+09:00").unwrap().epoch;
    let late = |extra: &[String]| {
        let mut lines = reach("scheduled", "k");
        lines.extend(extra.iter().cloned());
        fold_rows(&rows_of(&lines))
    };
    assert_eq!(late(&[]).overdue(now).len(), 1);
    let moved = "resched: k [start:2026-11-06T10:00+09:00 end:2026-11-06T12:00+09:00]".to_string();
    assert!(late(&[moved]).overdue(now).is_empty());
    assert!(late(&[sample("done", "k")]).overdue(now).is_empty());
    assert!(late(&[sample("drop", "k")]).overdue(now).is_empty());
}

// ---------------------------------------------------------------- 書き込み口(vault)

fn vault(dir: &std::path::Path) -> Vault {
    std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
    std::fs::write(
        dir.join(".obsidian/daily-notes.json"),
        r#"{"folder":"journal","format":"YYYY/YYYY-MM-DD"}"#,
    )
    .unwrap();
    Vault::open(dir).unwrap()
}

/// 2026-11-01(日数)。
const D: i64 = 20_758;

fn add(v: &Vault, day: i64, text: &str) -> Result<slot::SlotAdded, String> {
    slot::add_slot(v, day, "09:00", text)
}

#[test]
fn the_writer_refuses_what_the_table_forbids_and_is_idempotent_on_retries() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    let plan = format!("plan: k [{W1} issue:example/repo#1]");
    // 開いていない枠には、plan 以外を書けない。
    for e in ["resched", "done", "drop"] {
        assert!(add(&v, D, &sample(e, "k")).is_err(), "{e}");
    }
    assert!(!add(&v, D, &plan).unwrap().duplicate);
    // 同じ plan の再実行は記録済み。違う内容の plan は、鍵が使い回せないので拒否する。
    assert!(add(&v, D, &plan).unwrap().duplicate);
    let e = add(&v, D + 1, &format!("plan: k [{W2}]")).unwrap_err();
    assert!(e.contains("既にある"), "{e}");
    // 反対の向きの比較: ほかの鍵なら通る。
    assert!(add(&v, D, &sample("plan", "other")).is_ok());

    // resched は今の時間枠と同じなら記録済み、違えば足す。
    assert!(add(&v, D, &sample("resched", "k")).is_ok());
    assert!(add(&v, D + 1, &sample("resched", "k")).unwrap().duplicate);
    // done は 1 回。同じ終端の再実行は記録済み、別の終端は拒否する。
    assert!(!add(&v, D + 1, "done: k").unwrap().duplicate);
    assert!(add(&v, D + 2, "done: k").unwrap().duplicate);
    assert!(add(&v, D + 2, "drop: k").is_err());
    assert!(add(&v, D + 2, &sample("resched", "k")).is_err());
    assert!(add(&v, D + 2, &sample("plan", "k")).is_err());

    let (_, folded) = slot::load(&v, D + 2).unwrap();
    assert!(
        folded.discrepancies.is_empty(),
        "{:?}",
        folded.discrepancies
    );
    assert_eq!(folded.slots["k"].state, "done");
    assert_eq!(folded.slots["other"].state, "scheduled");
}

#[test]
fn rescheduling_back_to_an_earlier_window_works_across_days_but_not_within_one_note() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    add(&v, D, &sample("plan", "k")).unwrap();
    add(&v, D, &sample("resched", "k")).unwrap(); // W2
    let back = format!("resched: k [{W1}]");
    // 同じ日のノートに W1 の行は plan のものしか無い(resched の行としては新しい)ので通る。
    add(&v, D, &back).unwrap();
    // さらに W2 へ戻すと、同じ日に同じ resched の行が既にあり、状態が変わらないまま黙るのを避けて止める。
    let e = add(&v, D, &sample("resched", "k")).unwrap_err();
    assert!(e.contains("同じ行が既にある"), "{e}");
    // 翌日なら書ける。
    add(&v, D + 1, &sample("resched", "k")).unwrap();
    let (_, f) = slot::load(&v, D + 1).unwrap();
    assert_eq!(f.slots["k"].start.text, "2026-11-03T10:00+09:00");
    assert!(f.discrepancies.is_empty());
}

#[test]
fn the_writer_keeps_owner_repo_references_and_non_canonical_text_out() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    let t = sample("plan", "k");
    assert!(worklog::add(
        &v,
        D,
        "09:00",
        Kind::Slot,
        &t,
        worklog::Reference::parse("o/r#1")
    )
    .is_err());
    // 正規の形でない本文は lib の書き込み口が弾く(add_slot は正規の形に直して書く)。
    let swapped = "plan: k [end:2026-11-02T12:00+09:00 start:2026-11-02T10:00+09:00]";
    assert!(worklog::add(&v, D, "09:00", Kind::Slot, swapped, None).is_err());
    assert!(add(&v, D, swapped).is_ok());
    let note = std::fs::read_to_string(v.note_path(D)).unwrap();
    assert_eq!(
        note,
        format!("## 作業ログ\n- 09:00 [slot] plan: k [{W1}]\n")
    );
}

#[test]
fn slots_planned_before_the_window_are_still_read_back() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    add(&v, D, &sample("plan", "old")).unwrap();
    // 400 日近く後でも、読む範囲は list の上限に収まり、エラーにならない。
    let (_, f) = slot::load(&v, D + 380).unwrap();
    assert_eq!(f.slots["old"].state, "scheduled");
    let far = slot::load(&v, D + 2000);
    assert!(far.is_ok(), "{:?}", far.err());
}
