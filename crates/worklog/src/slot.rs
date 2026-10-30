//! `slot` kind の作業の枠の完了記録(docs/agents/task-slot.md)。
//!
//! 作業の枠(時間枠つき)の時間の定義は予定データ(Git)に残る。作業ログが持つのは、その枠が
//! 「済んだ」「やめた」の完了記録だけ。状態は保存せず、vault の当日ノートの `[slot]` 行を出来事として
//! 畳み込んで決める(`mail` と同じ event sourcing)。状態・出来事・遷移の表は
//! `config/agents/task-slot/vocabulary.json` が唯一の正本で、この crate は `include_str!` で同じ表を読む。
//! 行の形は次のとおり。時間枠も Issue も行に持たない。
//!
//! ```text
//! - 12:30 [slot] done: light-task-cad-measure-20261026
//! - 12:31 [slot] drop: light-task-other-20261027
//! ```
//!
//! 鍵は予定データの予定 id(kebab-case)。同じ鍵に `done` と `drop` の両方は書けない: 先に書かれた
//! 終端が勝つ。本文・個人名・メールの要旨は書かない。

use crate::mail::{parse_states_events, Discrepancy, EventDef};
use crate::{add as add_line, format_date, list, parse_date, Added, Kind, Row, Vault};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

/// 語彙の正本(`config/agents/task-slot/vocabulary.json`)。
const VOCABULARY_JSON: &str = include_str!("../../../config/agents/task-slot/vocabulary.json");

/// この日付より前のノートは畳み込まない(kind を足した日)。
pub const SLOT_SINCE: &str = "2026-10-30";
/// 鍵の最大文字数(予定データの予定 id を受けるので、短い id を前提にしない)。
pub const SLOT_KEY_MAX_CHARS: usize = 128;

// ---------------------------------------------------------------- 語彙

/// 語彙 JSON(状態と出来事)。`mail` の語彙と同じ形で、タイマーだけが無い。
#[derive(Debug)]
pub struct SlotVocabulary {
    pub events: Vec<EventDef>,
    pub states: Vec<(String, bool)>,
}

impl SlotVocabulary {
    pub fn parse(json: &str) -> Result<SlotVocabulary, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("語彙が JSON でない: {e}"))?;
        let (states, events) = parse_states_events(&v)?;
        // 無人で動く書き手(`mail sweep` に当たるもの)は無い。機械の出来事は置けない。
        if let Some(e) = events.iter().find(|e| e.by == "machine") {
            return Err(format!(
                "出来事 {} は machine が書く。slot にタイマーは無い",
                e.id
            ));
        }
        Ok(SlotVocabulary { events, states })
    }

    pub fn get() -> &'static SlotVocabulary {
        static V: OnceLock<SlotVocabulary> = OnceLock::new();
        V.get_or_init(|| {
            SlotVocabulary::parse(VOCABULARY_JSON).expect("task-slot の語彙が壊れている")
        })
    }

    pub fn event(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn is_terminal(&self, state: &str) -> bool {
        self.states.iter().any(|(s, t)| s == state && *t)
    }

    /// `a|b|c` の形。エラー文言用。
    pub fn event_names(&self) -> String {
        self.events
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
            .join("|")
    }
}

// ---------------------------------------------------------------- 行の文法

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLine {
    pub event: String,
    pub key: String,
}

const FORM: &str = "`<done|drop>: <鍵>`(時間枠・Issue・印は付けない)";

fn check_key(k: &str) -> Result<(), String> {
    let kebab = !k.is_empty()
        && k.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !kebab {
        return Err(format!(
            "枠の鍵は予定の id(kebab-case。小文字・数字・ハイフン。例 `light-task-cad-measure-20261026`): '{k}'"
        ));
    }
    let n = k.chars().count();
    if n > SLOT_KEY_MAX_CHARS {
        return Err(format!("枠の鍵が長い({n} 文字。上限 {SLOT_KEY_MAX_CHARS})"));
    }
    Ok(())
}

/// `slot` の本文を解析し、文法と閉語彙を検査する。
///
/// 書くのは出来事と鍵だけ。時間枠も要旨も個人名も持てない(持つ欄が無い)。
pub fn parse_slot(text: &str) -> Result<SlotLine, String> {
    let vocab = SlotVocabulary::get();
    let (event, key) = text
        .split_once(": ")
        .ok_or_else(|| format!("slot は出来事と鍵が要る。形は {FORM}"))?;
    vocab.event(event).ok_or_else(|| {
        format!(
            "slot の出来事が閉語彙の外: '{event}'(使えるのは: {})",
            vocab.event_names()
        )
    })?;
    if key.contains(['[', ']']) {
        return Err(format!("{event} に印は付けない。形は {FORM}"));
    }
    check_key(key)?;
    Ok(SlotLine {
        event: event.to_string(),
        key: key.to_string(),
    })
}

/// 書き込む本文の正規の形。
pub fn format_slot(l: &SlotLine) -> String {
    format!("{}: {}", l.event, l.key)
}

/// 語彙の外・文法違反・正規でない形を `Err` にする(`lib.rs` の `add` から呼ぶ)。
pub fn validate_slot(text: &str) -> Result<(), String> {
    let line = parse_slot(text)?;
    let canon = format_slot(&line);
    if canon != text {
        return Err(format!(
            "slot の本文は正規の形で書く(`<出来事>: <鍵>`、区切りは `: `): '{canon}'"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- 畳み込み

#[derive(Debug, Clone)]
pub struct Ev {
    pub day: i64,
    pub time: Option<String>,
    pub line: SlotLine,
}

/// 完了記録(終端に入った枠)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    pub key: String,
    /// 語彙の終端の状態(`done` | `dropped`)。
    pub state: String,
    /// 記録した日(日数)。
    pub day: i64,
}

impl Closed {
    /// `鍵<TAB>状態<TAB>日付(YYYY-MM-DD)`。
    pub fn tsv(&self) -> String {
        [self.key.clone(), self.state.clone(), format_date(self.day)].join("\t")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "state": self.state,
            "day": format_date(self.day),
        })
    }
}

#[derive(Debug, Default)]
pub struct Folded {
    /// 記録済みの枠。書かれた順(日付・時刻の順)。
    pub closed: Vec<Closed>,
    pub discrepancies: Vec<Discrepancy>,
}

impl Folded {
    pub fn get(&self, key: &str) -> Option<&Closed> {
        self.closed.iter().find(|c| c.key == key)
    }
}

/// `[slot]` の行を、日付・時刻の順に並べて解析する。解析できない行は食い違い。
///
/// 日付と時刻が同じ行は、出来事・鍵の順に並べる。行がノートのどこにあるか(読み込みの順)には依らない。
pub fn collect(rows: &[Row]) -> (Vec<Ev>, Vec<Discrepancy>) {
    let mut evs = Vec::new();
    let mut bad = Vec::new();
    for r in rows {
        if r.entry.kind != Some(Kind::Slot) {
            continue;
        }
        let Some(day) = parse_date(&r.date) else {
            continue;
        };
        match parse_slot(&r.entry.text) {
            Ok(line) => evs.push(Ev {
                day,
                time: r.entry.time.clone(),
                line,
            }),
            Err(m) => bad.push(Discrepancy {
                key: format!("{} {}", r.date, r.entry.text),
                message: format!("解析できない行: {m}"),
            }),
        }
    }
    evs.sort_by(|a, b| {
        (
            a.day,
            a.time.as_deref().unwrap_or("00:00"),
            &a.line.event,
            &a.line.key,
        )
            .cmp(&(
                b.day,
                b.time.as_deref().unwrap_or("00:00"),
                &b.line.event,
                &b.line.key,
            ))
    });
    bad.sort_by(|a, b| (&a.key, &a.message).cmp(&(&b.key, &b.message)));
    (evs, bad)
}

/// 遷移の可否は語彙(`from`)だけが決める。先に書かれた終端が勝ち、あとから別の終端を書いた行は
/// 状態を変えず、食い違いに数える。同じ終端の繰り返しは、状態を変えないので食い違いにしない。
pub fn fold(events: &[Ev]) -> Folded {
    let vocab = SlotVocabulary::get();
    let mut out = Folded::default();
    for ev in events {
        let def = vocab
            .event(&ev.line.event)
            .expect("parse_slot が語彙を検査済み");
        match out.closed.iter().find(|c| c.key == ev.line.key) {
            None => out.closed.push(Closed {
                key: ev.line.key.clone(),
                state: def.to.clone(),
                day: ev.day,
            }),
            Some(c) if c.state == def.to => {}
            Some(c) => out.discrepancies.push(Discrepancy {
                key: ev.line.key.clone(),
                message: format!("終端 {} の後の {}", c.state, def.id),
            }),
        }
    }
    out
}

/// `until`(日数)までの `[slot]` 行を読んで畳み込む。読む範囲は `SLOT_SINCE` から、ただし `list` の
/// 上限(400 日)に収まる分まで。
pub fn load(vault: &Vault, until: i64) -> Result<(Vec<Ev>, Folded), String> {
    let since = parse_date(SLOT_SINCE)
        .expect("SLOT_SINCE")
        .max(until - (crate::MAX_RANGE_DAYS - 1))
        .min(until);
    let rows = list(vault, since, until)?;
    let (evs, bad) = collect(&rows);
    let mut folded = fold(&evs);
    folded.discrepancies.extend(bad);
    Ok((evs, folded))
}

// ---------------------------------------------------------------- 追記

#[derive(Debug, PartialEq, Eq)]
pub struct SlotAdded {
    pub path: PathBuf,
    /// 同じ出来事が既に記録済み(行は足していない)。
    pub duplicate: bool,
}

/// 畳み込んだ今の状態から許される出来事だけを、正規の形で書く。同じ鍵に別の終端が記録済みなら
/// 書かずに止める。同じ事象の再実行は記録済みにする。
pub fn add_slot(vault: &Vault, days: i64, time: &str, text: &str) -> Result<SlotAdded, String> {
    let vocab = SlotVocabulary::get();
    let line = parse_slot(text.trim())?;
    let def = vocab
        .event(&line.event)
        .expect("parse_slot が語彙を検査済み");
    let (_, folded) = load(vault, days)?;
    let note = vault.note_path(days);
    if let Some(c) = folded.get(&line.key) {
        if c.state == def.to {
            return Ok(SlotAdded {
                path: note,
                duplicate: true,
            });
        }
        return Err(format!(
            "{} は既に {}({})と記録されていて、{} は書けない",
            line.key,
            c.state,
            format_date(c.day),
            def.id
        ));
    }
    match add_line(vault, days, time, Kind::Slot, &format_slot(&line), None)? {
        Added::Appended(path) => Ok(SlotAdded {
            path,
            duplicate: false,
        }),
        // 畳み込みが見ていない範囲(切り替え日より前)に同じ行がある。記録済みと同じ。
        Added::Duplicate(path) => Ok(SlotAdded {
            path,
            duplicate: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_vocabulary_parses_and_matches_the_plan() {
        let v = SlotVocabulary::get();
        assert_eq!(v.event_names(), "done|drop");
        assert!(v.is_terminal("done") && v.is_terminal("dropped"));
        assert!(SlotVocabulary::parse(
            r#"{"states":[],"events":[{"id":"x","from":["nope"],"to":["same"],"by":"agent"}]}"#
        )
        .is_err());
        // タイマーを動かす書き手(machine)の出来事は置けない。
        assert!(SlotVocabulary::parse(
            r#"{"states":[{"id":"a","terminal":false}],"events":[{"id":"x","from":["a"],"to":["same"],"by":"machine"}]}"#
        )
        .is_err());
    }

    #[test]
    fn grammar_accepts_the_forms_and_rejects_the_rest() {
        let p = parse_slot("done: light-task-cad-measure-20261026").unwrap();
        assert_eq!(p.key, "light-task-cad-measure-20261026");
        assert_eq!(format_slot(&p), "done: light-task-cad-measure-20261026");
        assert!(parse_slot("drop: a1").is_ok());
        assert!(validate_slot("drop: a1").is_ok());
        for bad in [
            "plan: a",
            "resched: a",
            "skip: a",
            "done a",
            "done:a",
            "done: ",
            "done: Reader",
            "done: reader_bot",
            "done: -a",
            "done: a--b",
            "done: a b",
            "done: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
            "drop: a [issue:example/repo#1]",
            "done: 日本語",
        ] {
            assert!(parse_slot(bad).is_err(), "{bad}");
        }
        // 正規の形でない本文(余分な空白)は書き込み口が弾く。
        assert!(validate_slot("done:  a").is_err());
        let long = "a".repeat(SLOT_KEY_MAX_CHARS + 1);
        assert!(parse_slot(&format!("done: {long}")).is_err());
        let max = "a".repeat(SLOT_KEY_MAX_CHARS);
        assert!(parse_slot(&format!("done: {max}")).is_ok());
    }
}
