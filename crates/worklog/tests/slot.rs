//! `slot` kind の完了記録の畳み込み・遷移の試験(config/agents/task-slot/vocabulary.json)。
//! 語彙 JSON から読んだ遷移表を実装が守ることを、表の全マスで確かめる。fixture は架空の値。

use worklog::slot::{self, SlotVocabulary};
use worklog::{Entry, Kind, Row, Vault};

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

/// 出来事 `event` の見本の行(鍵は `key`)。
fn sample(event: &str, key: &str) -> String {
    match event {
        "done" | "drop" => format!("{event}: {key}"),
        other => panic!("見本が無い出来事: {other}"),
    }
}

/// `state` に入るまでの行(`none` は何も無い)。
fn reach(state: &str, key: &str) -> Vec<String> {
    match state {
        "none" => vec![],
        "done" => vec![sample("done", key)],
        "dropped" => vec![sample("drop", key)],
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
            lines.push(sample(&def.id, "k"));
            let f = fold_rows(&rows_of(&lines));
            let now = f.get("k").map(|c| c.state.as_str());
            let allowed = def.from.iter().any(|x| x == state);
            if allowed {
                // 表が許す遷移は、食い違いなく行き先の状態になる。
                assert!(f.discrepancies.is_empty(), "{state} --{}-->", def.id);
                assert_eq!(now, Some(def.to.as_str()), "{state} --{}-->", def.id);
            } else if *state == def.to {
                // すでにその終端にいて、同じ事象を繰り返しただけ。状態は変わらず、食い違いでもない。
                assert!(f.discrepancies.is_empty(), "{state} --{}-->", def.id);
                assert_eq!(now, Some(*state), "{state} --{}-->", def.id);
            } else {
                // 別の終端への事象は、先に書かれた終端を変えず、食い違いに数える。
                assert_eq!(f.discrepancies.len(), 1, "{state} --{}-->", def.id);
                assert_eq!(now, Some(*state), "{state} --{}-->", def.id);
            }
            cells += 1;
        }
    }
    assert_eq!(cells, 3 * 2, "none と 2 状態 × 出来事 2");
}

#[test]
fn terminal_states_have_no_way_out_in_the_table() {
    let v = SlotVocabulary::get();
    for (s, terminal) in &v.states {
        assert!(
            *terminal,
            "状態 {s} は終端でない。完了記録の語彙に未了は無い"
        );
        assert!(
            v.events.iter().all(|e| !e.from.contains(s)),
            "終端 {s} から出る出来事がある"
        );
    }
    // 事象はどれも、まだ記録の無い枠(none)からだけ出て、終端に入る。
    for e in &v.events {
        assert_eq!(e.from, ["none"], "{}", e.id);
        assert!(v.is_terminal(&e.to), "{}", e.id);
    }
}

#[test]
fn the_vocabulary_has_exactly_the_events_the_grammar_knows() {
    let v = SlotVocabulary::get();
    for def in &v.events {
        // 見本が文法を通る(語彙に出来事を足して文法を足し忘れると、ここで落ちる)。
        slot::parse_slot(&sample(&def.id, "k")).unwrap();
        assert_ne!(def.by, "machine", "{}", def.id);
        // 済んだか・やめたかは本人にしか分からない。
        assert_eq!(def.by, "person", "{}", def.id);
    }
    assert_eq!(v.event_names(), "done|drop");
    assert_eq!(v.event("done").unwrap().to, "done");
    assert_eq!(v.event("drop").unwrap().to, "dropped");
}

#[test]
fn the_first_terminal_wins_and_the_later_one_is_a_discrepancy() {
    for (first, second, state) in [("done", "drop", "done"), ("drop", "done", "dropped")] {
        let f = fold_rows(&[
            row("2026-11-01", "09:00", &sample(first, "k")),
            row("2026-11-02", "09:00", &sample(second, "k")),
        ]);
        assert_eq!(f.get("k").unwrap().state, state);
        assert_eq!(f.get("k").unwrap().day, 20_758, "先に書かれた日が残る");
        assert_eq!(f.discrepancies.len(), 1, "{:?}", f.discrepancies);
        assert_eq!(f.discrepancies[0].key, "k");
        assert!(
            f.discrepancies[0].message.contains("の後の"),
            "{:?}",
            f.discrepancies
        );
    }
    // 同じ分に両方が書かれても(合流した別の機体の行)、並びは行の順でなく内容で決まる。
    for order in [[0, 1], [1, 0]] {
        let lines = [sample("done", "k"), sample("drop", "k")];
        let rows: Vec<Row> = order
            .iter()
            .map(|&i| row("2026-11-01", "09:00", &lines[i]))
            .collect();
        let f = fold_rows(&rows);
        assert_eq!(f.get("k").unwrap().state, "done", "{order:?}");
        assert_eq!(f.discrepancies.len(), 1, "{order:?}");
    }
    // 違う鍵は互いに干渉しない。
    let f = fold_rows(&rows_of(&[sample("done", "a"), sample("drop", "b")]));
    assert!(f.discrepancies.is_empty());
    assert_eq!(f.get("a").unwrap().state, "done");
    assert_eq!(f.get("b").unwrap().state, "dropped");
}

#[test]
fn fold_is_deterministic_whatever_order_the_rows_come_in() {
    // 時刻が違う行は、読み込みの順に依らず、時刻の順に畳み込まれる。同じ分の行も内容の順で決まる。
    let rows = vec![
        row("2026-11-01", "09:00", &sample("done", "k")),
        row("2026-11-02", "08:00", &sample("drop", "k")),
        row("2026-11-01", "09:05", &sample("drop", "other")),
        row("2026-11-02", "08:30", &sample("done", "other")),
        row("2026-11-01", "09:05", &sample("done", "same-minute")),
    ];
    let key = |f: &slot::Folded| {
        (
            f.closed
                .iter()
                .map(|c| (c.key.clone(), c.state.clone(), c.day))
                .collect::<Vec<_>>(),
            f.discrepancies.len(),
        )
    };
    let base = fold_rows(&rows);
    assert_eq!(base.get("k").unwrap().state, "done");
    assert_eq!(base.get("other").unwrap().state, "dropped");
    assert_eq!(base.discrepancies.len(), 2);
    // 全順列(5! = 120 通り)で同じ結果。書かれた順(closed の並び)も同じ。
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let mut seen = 0;
    permute(&mut idx, 0, &mut |p| {
        let shuffled: Vec<Row> = p.iter().map(|&i| rows[i].clone()).collect();
        let f = fold_rows(&shuffled);
        assert_eq!(key(&f), key(&base), "{p:?}");
        seen += 1;
    });
    assert_eq!(seen, 120);
    // closed は書かれた順(日付・時刻。同じ分は出来事の名前の順)。
    let keys: Vec<_> = base.closed.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["k", "same-minute", "other"]);
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
        row(
            "2026-11-01",
            "09:01",
            "plan: k [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
        ),
        row("2026-11-01", "09:02", "done: BAD KEY"),
        row("2026-11-01", "09:03", &sample("done", "k")),
    ]);
    assert_eq!(evs.len(), 1);
    assert_eq!(bad.len(), 3);
    assert!(bad[0].message.contains("解析できない行"));
}

#[test]
fn closed_entries_render_as_tsv_and_json() {
    let f = fold_rows(&[
        row("2026-11-01", "09:00", &sample("done", "a-1")),
        row("2026-11-02", "09:00", &sample("drop", "b-2")),
    ]);
    let tsv: Vec<_> = f.closed.iter().map(|c| c.tsv()).collect();
    assert_eq!(tsv, ["a-1\tdone\t2026-11-01", "b-2\tdropped\t2026-11-02"]);
    assert_eq!(
        f.closed[1].to_json(),
        serde_json::json!({"key": "b-2", "state": "dropped", "day": "2026-11-02"})
    );
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
fn the_writer_refuses_a_second_terminal_and_is_idempotent_on_retries() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    // 記録の無い枠には、done も drop も書ける(予定データの予定 id が鍵)。
    assert!(!add(&v, D, "done: k").unwrap().duplicate);
    assert!(!add(&v, D, "drop: other").unwrap().duplicate);
    // 同じ事象の再実行は、日が変わっても記録済み。行は増えない。
    assert!(add(&v, D, "done: k").unwrap().duplicate);
    assert!(add(&v, D + 1, "done: k").unwrap().duplicate);
    assert!(add(&v, D + 1, "drop: other").unwrap().duplicate);
    // 別の終端は拒否する(先に書かれた終端が勝つ)。
    let e = add(&v, D + 2, "drop: k").unwrap_err();
    assert!(e.contains("既に done") && e.contains("2026-11-01"), "{e}");
    assert!(add(&v, D + 2, "done: other").is_err());
    // 出来事の語彙の外と、時間枠つきの旧い形は書けない。
    for bad in [
        "plan: k [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
        "resched: k [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
        "done: k [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
    ] {
        assert!(add(&v, D + 2, bad).is_err(), "{bad}");
    }

    let (_, folded) = slot::load(&v, D + 2).unwrap();
    assert!(
        folded.discrepancies.is_empty(),
        "{:?}",
        folded.discrepancies
    );
    assert_eq!(folded.get("k").unwrap().state, "done");
    assert_eq!(folded.get("other").unwrap().state, "dropped");
    let note = std::fs::read_to_string(v.note_path(D)).unwrap();
    assert_eq!(
        note,
        "## 作業ログ\n- 09:00 [slot] done: k\n- 09:00 [slot] drop: other\n"
    );
    assert!(!v.note_path(D + 1).exists());
}

#[test]
fn the_writer_keeps_owner_repo_references_and_non_canonical_text_out() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    assert!(worklog::add(
        &v,
        D,
        "09:00",
        Kind::Slot,
        "done: k",
        worklog::Reference::parse("o/r#1")
    )
    .is_err());
    // 正規の形でない本文は lib の書き込み口が弾く。
    assert!(worklog::add(&v, D, "09:00", Kind::Slot, "done:  k", None).is_err());
    assert!(worklog::add(&v, D, "09:00", Kind::Slot, "done: k", None).is_ok());
}

#[test]
fn records_before_the_window_are_still_read_back() {
    let d = tempfile::tempdir().unwrap();
    let v = vault(d.path());
    add(&v, D, "done: old").unwrap();
    // 400 日近く後でも、読む範囲は list の上限に収まり、エラーにならない。
    let (_, f) = slot::load(&v, D + 380).unwrap();
    assert_eq!(f.get("old").unwrap().state, "done");
    let far = slot::load(&v, D + 2000);
    assert!(far.is_ok(), "{:?}", far.err());
}
