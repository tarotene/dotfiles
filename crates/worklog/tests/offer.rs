//! `worklog offer` の状態機械・回復の計画・ジャーナルの読み込みの試験
//! (config/agents/slot-offer/vocabulary.json、docs/agents/slot-offer.md)。
//! カレンダーと送信は偽の世界(`World`)に差し替える。fixture は架空の値。

use std::collections::BTreeMap;
use std::process::Command;
use worklog::offer::{
    self, apply, hold_title, next_steps, record_cancelled, record_chosen, record_holds,
    record_offered, recover, Action, Event, OfferVocabulary, Plan, RecordKind, Sent, State, Store,
};

const ID: &str = "offer-20261105-a";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::at(dir.path().join("slot-offer"));
    (dir, s)
}

fn three_slots() -> Vec<(String, String)> {
    vec![
        ("2026-11-10T10:00".into(), "2026-11-10T11:00".into()),
        ("2026-11-11T10:00".into(), "2026-11-11T11:00".into()),
        ("2026-11-12T10:00".into(), "2026-11-12T11:00".into()),
    ]
}

fn holds_of(ns: &[u32]) -> BTreeMap<u32, String> {
    ns.iter().map(|n| (*n, format!("ev{n}"))).collect()
}

/// 実際の遷移をたどって、`state` の提案を作る(ジャーナルを手で書かない)。
fn journal_in(store: &Store, state: State) {
    store.begin(ID, "thread-1", &three_slots()).unwrap();
    match state {
        State::Intended => {}
        State::Held => {
            record_holds(store, ID, &holds_of(&[1, 2, 3])).unwrap();
        }
        State::Offered => {
            record_holds(store, ID, &holds_of(&[1, 2, 3])).unwrap();
            record_offered(store, ID).unwrap();
        }
        State::Chosen => {
            record_holds(store, ID, &holds_of(&[1, 2, 3])).unwrap();
            record_offered(store, ID).unwrap();
            record_chosen(store, ID, &[2]).unwrap();
        }
        State::Cancelled => {
            record_cancelled(store, ID, false, &BTreeMap::new()).unwrap();
        }
    }
    assert_eq!(store.load(ID).unwrap().unwrap().state, state);
}

// ---------------------------------------------------------------- 語彙と実装の一致

#[test]
fn vocabulary_and_enums_cover_each_other() {
    let v = OfferVocabulary::get();
    let mut vs: Vec<&str> = v.states.iter().map(|(s, _)| s.as_str()).collect();
    let mut es: Vec<&str> = State::ALL.iter().map(|s| s.name()).collect();
    vs.sort_unstable();
    es.sort_unstable();
    assert_eq!(vs, es, "語彙の状態と State が一致しない");
    let mut ve: Vec<&str> = v.events.iter().map(|e| e.id.as_str()).collect();
    let mut ee: Vec<&str> = Event::ALL.iter().map(|e| e.id()).collect();
    ve.sort_unstable();
    ee.sort_unstable();
    assert_eq!(ve, ee, "語彙の事象と Event が一致しない");
    for (s, terminal) in &v.states {
        let outgoing = v
            .events
            .iter()
            .any(|e| e.from.iter().any(|f| f == s) && e.to != *s && e.to != "same");
        assert_eq!(*terminal, !outgoing, "{s}: 終端と出口の食い違い");
    }
}

#[test]
fn every_cell_of_the_vocabulary_table_is_obeyed() {
    let v = OfferVocabulary::get();
    let mut cells = 0;
    let mut states: Vec<Option<State>> = vec![None];
    states.extend(State::ALL.map(Some));
    for st in &states {
        for ev in Event::ALL {
            let def = v.event(ev.id()).unwrap();
            let from = st.map_or("none", State::name);
            let allowed = def.from.iter().any(|f| f == from);
            let got = apply(*st, ev);
            assert_eq!(got.is_ok(), allowed, "{from} --{}--> のマス", ev.id());
            if let Ok(to) = got {
                let want = if def.to == "same" { from } else { &def.to };
                assert_eq!(to.name(), want, "{from} --{}--> の行き先", ev.id());
            }
            cells += 1;
        }
    }
    assert_eq!(cells, 6 * 8);
}

#[test]
fn hold_title_is_read_from_the_handshake_vocabulary() {
    assert_eq!(hold_title(), "[hold] ");
}

// ---------------------------------------------------------------- 遷移の関門

#[test]
fn present_needs_every_hold_read_back_first() {
    let (_d, s) = store();
    s.begin(ID, "thread-1", &three_slots()).unwrap();
    let e = record_offered(&s, ID).unwrap_err();
    assert!(e.contains("押さえが全スロット揃っていない"), "{e}");
    // 2 件だけ読み戻せたときは、held にならず intended のまま。
    let j = record_holds(&s, ID, &holds_of(&[1, 3])).unwrap();
    assert_eq!(j.state, State::Intended);
    assert!(record_offered(&s, ID).is_err());
    // 揃えば held、その次に提示済み。
    assert_eq!(
        record_holds(&s, ID, &holds_of(&[1, 2, 3])).unwrap().state,
        State::Held
    );
    assert_eq!(record_offered(&s, ID).unwrap().state, State::Offered);
}

#[test]
fn a_hold_lost_before_presenting_sends_the_proposal_back_to_intended() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let j = record_holds(&s, ID, &holds_of(&[1, 3])).unwrap();
    assert_eq!(j.state, State::Intended);
    assert_eq!(j.slots[1].event_id, None);
}

#[test]
fn an_offer_that_is_out_stays_offered_when_a_hold_goes_missing() {
    let (_d, s) = store();
    journal_in(&s, State::Offered);
    let j = record_holds(&s, ID, &holds_of(&[1, 3])).unwrap();
    assert_eq!(j.state, State::Offered, "出た提示は取り消さない");
    assert_eq!(j.slots[1].event_id, None);
}

#[test]
fn cancelling_needs_the_read_back_to_show_no_holds_left() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let e = record_cancelled(&s, ID, false, &holds_of(&[2])).unwrap_err();
    assert!(e.contains("押さえが残っている"), "{e}");
    let j = record_cancelled(&s, ID, false, &BTreeMap::new()).unwrap();
    assert_eq!(j.state, State::Cancelled);
    assert!(j.slots.iter().all(|x| x.event_id.is_none()));
}

#[test]
fn an_offer_that_is_out_can_only_be_retracted_by_the_person() {
    let (_d, s) = store();
    journal_in(&s, State::Offered);
    let e = record_cancelled(&s, ID, false, &BTreeMap::new()).unwrap_err();
    assert!(e.contains("offered から abort は許されない"), "{e}");
    assert_eq!(
        record_cancelled(&s, ID, true, &BTreeMap::new())
            .unwrap()
            .state,
        State::Cancelled
    );
    // 出ていない提案を retract とは書けない。
    let (_d2, s2) = store();
    journal_in(&s2, State::Held);
    assert!(record_cancelled(&s2, ID, true, &BTreeMap::new()).is_err());
}

#[test]
fn choosing_needs_an_offer_and_real_slots() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    assert!(record_chosen(&s, ID, &[1]).is_err(), "提示前は選ばれない");
    let (_d2, s2) = store();
    journal_in(&s2, State::Offered);
    assert!(record_chosen(&s2, ID, &[]).is_err());
    assert!(record_chosen(&s2, ID, &[9]).is_err());
    let j = record_chosen(&s2, ID, &[3, 1, 3]).unwrap();
    assert_eq!((j.state, j.chosen), (State::Chosen, vec![1, 3]));
    // 終端からは動かない。
    assert!(record_holds(&s2, ID, &holds_of(&[1])).is_err());
}

#[test]
fn begin_is_idempotent_for_the_same_content_and_refuses_a_different_one() {
    let (_d, s) = store();
    assert!(matches!(
        s.begin(ID, "thread-1", &three_slots()).unwrap(),
        offer::Began::Created(_)
    ));
    assert!(matches!(
        s.begin(ID, "thread-1", &three_slots()).unwrap(),
        offer::Began::Duplicate(_)
    ));
    let e = s.begin(ID, "thread-2", &three_slots()).unwrap_err();
    assert!(e.contains("別の内容で記録済み"), "{e}");
    let e = s.begin(ID, "thread-1", &three_slots()[..2]).unwrap_err();
    assert!(e.contains("別の内容で記録済み"), "{e}");
}

#[test]
fn begin_rejects_malformed_input() {
    let (_d, s) = store();
    let bad = |id: &str, thread: &str, slots: &[(String, String)]| {
        s.begin(id, thread, slots).unwrap_err()
    };
    assert!(bad("../escape", "t", &three_slots()).contains("提案 id"));
    assert!(bad("Upper", "t", &three_slots()).contains("提案 id"));
    assert!(bad(ID, "has space", &three_slots()).contains("1 語"));
    assert!(bad(ID, "t", &[]).contains("スロットは"));
    let one = |a: &str, b: &str| vec![(a.to_string(), b.to_string())];
    let e = bad(ID, "t", &one("2026-11-10 10:00", "2026-11-10T11:00"));
    assert!(e.contains("YYYY-MM-DDTHH:MM"), "{e}");
    let e = bad(ID, "t", &one("2026-11-10T11:00", "2026-11-10T10:00"));
    assert!(e.contains("終了が開始以前"), "{e}");
    let e = bad(ID, "t", &one("2026-13-10T10:00", "2026-13-10T11:00"));
    assert!(e.contains("YYYY-MM-DDTHH:MM"), "{e}");
}

// ---------------------------------------------------------------- 読み込みで落ちる

#[test]
fn a_broken_or_unknown_journal_fails_at_load() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let path = s.path(ID);
    let good = std::fs::read_to_string(&path).unwrap();
    let cases: Vec<(&str, String)> = vec![
        ("JSON でない", "{ not json".to_string()),
        ("知らない状態", good.replace("\"held\"", "\"pending\"")),
        (
            "知らない項目",
            good.replacen("\"thread\"", "\"extra\": 1,\n  \"thread\"", 1),
        ),
        ("版が違う", good.replace("\"version\": 1", "\"version\": 2")),
        ("id がファイル名と違う", good.replace(ID, "offer-other")),
        (
            "held なのに予定 id が無い",
            good.replacen("\"event_id\": \"ev2\"", "\"event_id\": null", 1),
        ),
        (
            "終了が開始以前",
            good.replace("2026-11-10T11:00", "2026-11-10T09:00"),
        ),
        (
            "chosen なのに選択が無い",
            good.replace("\"held\"", "\"chosen\""),
        ),
    ];
    for (name, body) in cases {
        std::fs::write(&path, &body).unwrap();
        let e = s.load(ID).expect_err(name);
        assert!(e.contains("壊れている"), "{name}: {e}");
        // 壊れたジャーナルへの記録・計画も、読み込みで落ちる(黙って上書きしない)。
        assert!(record_offered(&s, ID).is_err(), "{name}");
        assert!(s.list(true).is_err(), "{name}");
    }
    std::fs::write(&path, good).unwrap();
    assert!(s.load(ID).unwrap().is_some());
}

#[test]
fn the_write_leaves_no_temporary_file_and_replaces_atomically() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let names: Vec<String> = std::fs::read_dir(s.path(ID).parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    assert!(names.contains(&format!("{ID}.json")));
}

// ---------------------------------------------------------------- 回復の表

fn tags(plan: &Plan) -> Vec<&'static str> {
    plan.tags()
}

/// 回復の表。Issue #1319 の 3 行(意図だけ・押さえ済み提示前・提示済み)と、押さえと提示の食い違いの各形。
#[test]
fn the_recovery_table_gives_the_safe_plan_for_each_state() {
    struct Row {
        name: &'static str,
        state: State,
        sent: Sent,
        holds: &'static [u32],
        tags: Vec<&'static str>,
    }
    let rows = vec![
        // Issue の表の 3 行
        Row {
            name: "意図だけ: 押さえを揃える。提示はせず、本人に聞く",
            state: State::Intended,
            sent: Sent::No,
            holds: &[],
            tags: vec![
                "create_hold",
                "create_hold",
                "create_hold",
                "read_back_holds",
                "record",
                "ask_person",
            ],
        },
        Row {
            name: "押さえ済み(提示前)で画面に文面が無い: abort(押さえを消す)",
            state: State::Held,
            sent: Sent::No,
            holds: &[1, 2, 3],
            tags: vec![
                "delete_hold",
                "delete_hold",
                "delete_hold",
                "read_back_holds",
                "record",
            ],
        },
        Row {
            name: "提示済みで押さえが欠けた: 欠けた分だけ作り直す",
            state: State::Offered,
            sent: Sent::Yes,
            holds: &[1, 3],
            tags: vec!["create_hold", "read_back_holds", "record"],
        },
        // 押さえと提示の食い違いの各形
        Row {
            name: "提示だけが出て押さえが無い: 押さえを作り、提示済みにする",
            state: State::Held,
            sent: Sent::Yes,
            holds: &[],
            tags: vec![
                "create_hold",
                "create_hold",
                "create_hold",
                "read_back_holds",
                "record",
                "record",
            ],
        },
        Row {
            name: "両方あり: 提示済みと記録するだけ",
            state: State::Held,
            sent: Sent::Yes,
            holds: &[1, 2, 3],
            tags: vec!["record"],
        },
        Row {
            name: "押さえだけが残る(意図の段階で提示なし): 押さえを揃えて本人に聞く",
            state: State::Intended,
            sent: Sent::No,
            holds: &[1, 2, 3],
            tags: vec!["read_back_holds", "record", "ask_person"],
        },
        Row {
            name: "確かめられない(held): 消さず・送らず、本人に聞く",
            state: State::Held,
            sent: Sent::Unknown,
            holds: &[1, 2, 3],
            tags: vec!["ask_person"],
        },
        Row {
            name: "提示済みと記録したが画面に無い: 押さえは残し、本人に聞く",
            state: State::Offered,
            sent: Sent::No,
            holds: &[1, 2, 3],
            tags: vec!["ask_person"],
        },
        Row {
            name: "提示済みで全部揃っている: することが無い",
            state: State::Offered,
            sent: Sent::Yes,
            holds: &[1, 2, 3],
            tags: vec!["nothing"],
        },
        Row {
            name: "確定済みで選ばれなかった押さえが残る: finalize",
            state: State::Chosen,
            sent: Sent::Yes,
            holds: &[1, 2, 3],
            tags: vec!["finalize"],
        },
        Row {
            name: "取消済みで押さえが残る: 消す",
            state: State::Cancelled,
            sent: Sent::Yes,
            holds: &[2],
            tags: vec!["delete_hold", "read_back_holds"],
        },
        Row {
            name: "取消済みで押さえが無い: することが無い",
            state: State::Cancelled,
            sent: Sent::No,
            holds: &[],
            tags: vec!["nothing"],
        },
    ];
    for r in rows {
        let (_d, s) = store();
        journal_in(&s, r.state);
        let j = s.load(ID).unwrap().unwrap();
        let plan = recover(&j, r.sent, &holds_of(r.holds)).unwrap();
        assert_eq!(tags(&plan), r.tags, "{}", r.name);
    }
}

#[test]
fn the_create_plan_is_idempotent_by_the_marker_and_uses_the_handshake_token() {
    let (_d, s) = store();
    journal_in(&s, State::Intended);
    let j = s.load(ID).unwrap().unwrap();
    let plan = recover(&j, Sent::No, &holds_of(&[2])).unwrap();
    let creates: Vec<&Action> = plan
        .actions
        .iter()
        .filter(|a| matches!(a, Action::CreateHold { .. }))
        .collect();
    assert_eq!(creates.len(), 2, "読み戻せた 2 番は作らない");
    match creates[0] {
        Action::CreateHold {
            slot,
            title,
            description,
            start,
            end,
        } => {
            assert_eq!(*slot, 1);
            assert_eq!(title, "[hold] ");
            assert_eq!(description, &format!("[offer:{ID}#1]"));
            assert_eq!(
                (start.as_str(), end.as_str()),
                ("2026-11-10T10:00", "2026-11-10T11:00")
            );
        }
        other => panic!("{other:?}"),
    }
    let read_back = plan
        .actions
        .iter()
        .find_map(|a| match a {
            Action::ReadBackHolds { search, expect } => Some((search.clone(), expect.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(read_back, (format!("[offer:{ID}#"), vec![1, 2, 3]));
}

#[test]
fn recover_refuses_a_hold_for_a_slot_the_proposal_does_not_have() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let j = s.load(ID).unwrap().unwrap();
    assert!(recover(&j, Sent::No, &holds_of(&[7])).is_err());
}

#[test]
fn the_normal_flow_plans_the_next_step_for_each_state() {
    let want = [
        (
            State::Intended,
            vec![
                "create_hold",
                "create_hold",
                "create_hold",
                "read_back_holds",
                "record",
            ],
        ),
        (State::Held, vec!["present", "read_back_sent", "record"]),
        (State::Offered, vec!["nothing"]),
        (State::Chosen, vec!["nothing"]),
        (State::Cancelled, vec!["nothing"]),
    ];
    for (state, tags) in want {
        let (_d, s) = store();
        journal_in(&s, state);
        let j = s.load(ID).unwrap().unwrap();
        assert_eq!(next_steps(&j).tags(), tags, "{}", state.name());
    }
}

#[test]
fn abort_plans_delete_every_slot_and_the_retract_asks_the_person_first() {
    let (_d, s) = store();
    journal_in(&s, State::Held);
    let j = s.load(ID).unwrap().unwrap();
    let p = offer::abort_plan(&j, false).unwrap();
    assert_eq!(
        p.tags(),
        vec![
            "delete_hold",
            "delete_hold",
            "delete_hold",
            "read_back_holds",
            "record"
        ]
    );
    // 出ていない提案を retract とは言えない。
    assert!(offer::abort_plan(&j, true).is_err());
    let (_d2, s2) = store();
    journal_in(&s2, State::Offered);
    let j2 = s2.load(ID).unwrap().unwrap();
    assert!(offer::abort_plan(&j2, false).is_err());
    assert_eq!(
        offer::abort_plan(&j2, true).unwrap().tags()[0],
        "ask_person",
        "取り下げは、相手への連絡を本人が決めてから押さえを消す"
    );
}

// ---------------------------------------------------------------- 中断の 3 段

/// 偽の世界: カレンダーの押さえと、送信済みの画面。
struct World {
    sent: bool,
    holds: BTreeMap<u32, String>,
    asked: bool,
}

/// 計画を偽の世界で実行し、結果を `record` で返す(エージェントの役)。
fn settle(store: &Store, world: &mut World, seen: Sent) -> Plan {
    let j = store.load(ID).unwrap().unwrap();
    let plan = recover(&j, seen, &world.holds).unwrap();
    for a in &plan.actions {
        match a {
            Action::CreateHold { slot, .. } => {
                let fresh = world.holds.insert(*slot, format!("ev{slot}"));
                assert!(fresh.is_none(), "既にある押さえを重複して作る計画: {slot}");
            }
            Action::DeleteHold { slot, .. } => {
                world.holds.remove(slot);
            }
            Action::ReadBackHolds { expect, .. } => {
                let got: Vec<u32> = world.holds.keys().copied().collect();
                assert_eq!(&got, expect, "読み戻しが期待と違う");
            }
            Action::AskPerson { .. } => world.asked = true,
            Action::Record { kind, .. } => match kind {
                RecordKind::Holds => {
                    record_holds(store, ID, &world.holds).unwrap();
                }
                RecordKind::Offered => {
                    record_offered(store, ID).unwrap();
                }
                RecordKind::Cancelled => {
                    record_cancelled(store, ID, false, &world.holds).unwrap();
                }
                RecordKind::Chosen => panic!("回復は選択を記録しない"),
            },
            Action::ReadBackSent {}
            | Action::Present { .. }
            | Action::Finalize { .. }
            | Action::Nothing { .. } => {}
        }
    }
    plan
}

#[test]
fn an_interruption_at_each_of_the_three_stages_settles_to_a_safe_state() {
    struct Row {
        stage: &'static str,
        journal: State,
        sent: bool,
        seen: Sent,
        holds: &'static [u32],
        want: State,
        want_holds: &'static [u32],
        asked: bool,
    }
    let rows = [
        // 1. 意図の記録後(押さえはまだ)
        Row {
            stage: "意図の記録後",
            journal: State::Intended,
            sent: false,
            seen: Sent::No,
            holds: &[],
            want: State::Held,
            want_holds: &[1, 2, 3],
            asked: true,
        },
        Row {
            stage: "意図の記録後(提示が先に出ていた)",
            journal: State::Intended,
            sent: true,
            seen: Sent::Yes,
            holds: &[],
            want: State::Offered,
            want_holds: &[1, 2, 3],
            asked: false,
        },
        // 2. 押さえ作成後(読み戻しの記録の前)
        Row {
            stage: "押さえ作成の途中",
            journal: State::Intended,
            sent: false,
            seen: Sent::No,
            holds: &[1],
            want: State::Held,
            want_holds: &[1, 2, 3],
            asked: true,
        },
        Row {
            stage: "押さえ作成後(記録の前)",
            journal: State::Intended,
            sent: false,
            seen: Sent::No,
            holds: &[1, 2, 3],
            want: State::Held,
            want_holds: &[1, 2, 3],
            asked: true,
        },
        Row {
            stage: "押さえ作成後に、送信に失敗した",
            journal: State::Held,
            sent: false,
            seen: Sent::No,
            holds: &[1, 2, 3],
            want: State::Cancelled,
            want_holds: &[],
            asked: false,
        },
        Row {
            stage: "押さえ作成後に、送信画面が読めない",
            journal: State::Held,
            sent: true,
            seen: Sent::Unknown,
            holds: &[1, 2, 3],
            want: State::Held,
            want_holds: &[1, 2, 3],
            asked: true,
        },
        // 3. 提示後
        Row {
            stage: "提示後(記録の前)",
            journal: State::Held,
            sent: true,
            seen: Sent::Yes,
            holds: &[1, 2, 3],
            want: State::Offered,
            want_holds: &[1, 2, 3],
            asked: false,
        },
        Row {
            stage: "提示後(記録の前)に押さえが 1 件消えた",
            journal: State::Held,
            sent: true,
            seen: Sent::Yes,
            holds: &[1, 3],
            want: State::Offered,
            want_holds: &[1, 2, 3],
            asked: false,
        },
        Row {
            stage: "提示の記録後に押さえが全部消えた",
            journal: State::Offered,
            sent: true,
            seen: Sent::Yes,
            holds: &[],
            want: State::Offered,
            want_holds: &[1, 2, 3],
            asked: false,
        },
    ];
    for r in rows {
        let (_d, s) = store();
        journal_in(&s, r.journal);
        let mut world = World {
            sent: r.sent,
            holds: holds_of(r.holds),
            asked: false,
        };
        settle(&s, &mut world, r.seen);
        let j = s.load(ID).unwrap().unwrap();
        assert_eq!(j.state, r.want, "{}: 状態", r.stage);
        assert_eq!(world.holds, holds_of(r.want_holds), "{}: 押さえ", r.stage);
        assert_eq!(world.asked, r.asked, "{}: 本人に聞く", r.stage);
        // 不変条件: 提示が出ているなら、全スロットの押さえがある(提示だけがあって押さえが無い状態を残さない)。
        if world.sent && r.seen != Sent::Unknown {
            assert_eq!(
                world.holds.len(),
                3,
                "{}: 提示だけが出て押さえが無い",
                r.stage
            );
        }
        // ジャーナルは、世界の押さえと食い違わない(読み戻しの結果がそのまま入っている)。
        let journal_holds = j.slots.iter().filter(|x| x.event_id.is_some()).count();
        assert_eq!(journal_holds, world.holds.len(), "{}: 記録と世界", r.stage);
        // もう一度回復しても、新しい作成・削除は出ない(冪等)。ただし held のまま画面に文面が無いときの
        // 回復は、表のとおり abort になる(本人が送ると決めたなら、回復でなく `plan` の提示の手順へ進む)。
        if !(j.state == State::Held && r.seen == Sent::No) {
            let again = settle(&s, &mut world, r.seen);
            assert!(
                !again.tags().contains(&"create_hold") && !again.tags().contains(&"delete_hold"),
                "{}: 2 回目の計画 {:?}",
                r.stage,
                again.tags()
            );
        }
    }
}

// ---------------------------------------------------------------- コマンド行

fn bin(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_worklog"))
        .env("WORKLOG_OFFER_DIR", dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn the_binary_runs_the_two_phase_flow_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    let ok = |args: &[&str]| {
        let o = bin(dir, args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    ok(&[
        "offer",
        "begin",
        ID,
        "--thread",
        "thread-1",
        "--slot",
        "2026-11-10T10:00/2026-11-10T11:00",
        "--slot",
        "2026-11-11T10:00/2026-11-11T11:00",
    ]);
    let plan: serde_json::Value = serde_json::from_str(&ok(&["offer", "plan", ID])).unwrap();
    assert_eq!(plan["state"], "intended");
    assert_eq!(plan["actions"][0]["action"], "create_hold");
    assert_eq!(plan["actions"][0]["description"], format!("[offer:{ID}#1]"));
    // 提示は、押さえを読み戻す前は記録できない。
    let o = bin(dir, &["offer", "record", ID, "offered"]);
    assert_eq!(o.status.code(), Some(2));
    ok(&["offer", "record", ID, "holds", "--holds", "1=evA,2=evB"]);
    ok(&["offer", "record", ID, "offered"]);
    assert_eq!(
        ok(&["offer", "list"]).trim(),
        format!("{ID}\toffered\tthread-1")
    );
    let rec: serde_json::Value = serde_json::from_str(&ok(&[
        "offer", "recover", ID, "--sent", "yes", "--holds", "1=evA",
    ]))
    .unwrap();
    assert_eq!(rec["actions"][0]["action"], "create_hold");
    assert_eq!(rec["actions"][0]["slot"], 2);
    ok(&["offer", "record", ID, "chosen", "--slots", "2"]);
    assert_eq!(
        ok(&["offer", "list"]).trim(),
        "",
        "終端は既定の一覧に出ない"
    );
    assert!(ok(&["offer", "list", "--all"]).contains("chosen"));
    // 知らないサブコマンド・状態を壊した書き込みは exit 2。
    assert_eq!(bin(dir, &["offer", "frobnicate"]).status.code(), Some(2));
    assert_eq!(bin(dir, &["offer"]).status.code(), Some(2));
}
