//! `slot` kind の作業の枠のライフサイクル(docs/agents/task-slot.md)。
//!
//! 作業の枠は、自分一人でやる作業に、時間枠(開始と終了)を取ったもの。状態は保存せず、vault の
//! 当日ノートの `[slot]` 行を出来事として畳み込んで決める(`mail` と同じ event sourcing)。
//! 状態・出来事・遷移の表は `config/agents/task-slot/vocabulary.json` が唯一の正本で、この crate は
//! `include_str!` で同じ表を読む。タイマーは持たない: 再通知も失効もせず、「過ぎた」は終了時刻から
//! 導く値で、状態として保存しない。行の形は次のとおり。
//!
//! ```text
//! - 09:12 [slot] plan: reader-bot-resume [start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00 issue:example/repo#12]
//! - 09:15 [slot] resched: reader-bot-resume [start:2026-11-03T10:00+09:00 end:2026-11-03T12:00+09:00]
//! - 12:30 [slot] done: reader-bot-resume
//! ```
//!
//! 鍵は枠の短い id(kebab-case)。同じ鍵は二度 `plan` できない(終端のあとも鍵は使い切り)。
//! 本文・個人名・メールの要旨は書かない。

use crate::mail::{parse_states_events, Discrepancy, EventDef};
use crate::{add as add_line, list, parse_date, Added, Kind, Reference, Row, Vault};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// 語彙の正本(`config/agents/task-slot/vocabulary.json`)。
const VOCABULARY_JSON: &str = include_str!("../../../config/agents/task-slot/vocabulary.json");

/// この日付より前のノートは畳み込まない(kind を足した日)。
pub const SLOT_SINCE: &str = "2026-10-30";
/// 枠の鍵の最大文字数。
pub const SLOT_KEY_MAX_CHARS: usize = 64;

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

// ---------------------------------------------------------------- 時刻

/// ISO 8601 の時刻(オフセット付き)。書かれた形のまま持ち、比べるときは `epoch`(unix 秒)を使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instant {
    pub text: String,
    pub epoch: i64,
}

fn two_digits(s: &str) -> Option<i64> {
    (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// `YYYY-MM-DDTHH:MM[:SS]` に `Z` か `±HH:MM` が付いた形だけ。オフセットの無い時刻は
/// どの時刻か決まらないので受けない。
pub fn parse_instant(s: &str) -> Result<Instant, String> {
    let bad =
        || format!("時刻は `YYYY-MM-DDTHH:MM[:SS]` にオフセット(`Z` か `+09:00`)が付いた形: '{s}'");
    if !s.is_ascii() {
        return Err(bad());
    }
    let (date, rest) = s.split_once('T').ok_or_else(bad)?;
    let days = parse_date(date).ok_or_else(bad)?;
    let (clock, offset_secs) = if let Some(c) = rest.strip_suffix('Z') {
        (c, 0)
    } else {
        let i = rest.rfind(['+', '-']).ok_or_else(bad)?;
        let (c, o) = rest.split_at(i);
        let sign = if o.starts_with('-') { -1 } else { 1 };
        let (oh, om) = o[1..].split_once(':').ok_or_else(bad)?;
        let (oh, om) = (
            two_digits(oh).ok_or_else(bad)?,
            two_digits(om).ok_or_else(bad)?,
        );
        if oh > 23 || om > 59 {
            return Err(bad());
        }
        (c, sign * (oh * 3600 + om * 60))
    };
    let mut parts = clock.split(':');
    let h = parts.next().and_then(two_digits).ok_or_else(bad)?;
    let m = parts.next().and_then(two_digits).ok_or_else(bad)?;
    let sec = match parts.next() {
        Some(x) => two_digits(x).ok_or_else(bad)?,
        None => 0,
    };
    if parts.next().is_some() || h > 23 || m > 59 || sec > 59 {
        return Err(bad());
    }
    Ok(Instant {
        text: s.to_string(),
        epoch: days * 86_400 + h * 3600 + m * 60 + sec - offset_secs,
    })
}

// ---------------------------------------------------------------- 行の文法

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLine {
    pub event: String,
    pub key: String,
    pub start: Option<Instant>,
    pub end: Option<Instant>,
    /// `OWNER/REPO#N`。`plan` の行だけが持つ。
    pub issues: Vec<String>,
}

const FORM: &str = "`<出来事>: <鍵> [start:<ISO 時刻> end:<ISO 時刻> [issue:OWNER/REPO#N …]]`(done・drop に印は付けない)";

fn check_key(k: &str) -> Result<(), String> {
    let kebab = !k.is_empty()
        && k.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !kebab {
        return Err(format!(
            "枠の鍵は kebab-case(小文字・数字・ハイフン。例 `reader-bot-resume`): '{k}'"
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
/// 書くのは枠の鍵と時間枠と参照 Issue だけ。要旨も個人名も持てない(持つ欄が無い)。
pub fn parse_slot(text: &str) -> Result<SlotLine, String> {
    let vocab = SlotVocabulary::get();
    let (event, rest) = text
        .split_once(": ")
        .ok_or_else(|| format!("slot は出来事と鍵が要る。形は {FORM}"))?;
    vocab.event(event).ok_or_else(|| {
        format!(
            "slot の出来事が閉語彙の外: '{event}'(使えるのは: {})",
            vocab.event_names()
        )
    })?;
    let rest = rest.trim();
    let (key, tag) = match rest.split_once(" [") {
        Some((k, t)) => (
            k.trim_end(),
            Some(
                t.strip_suffix(']')
                    .ok_or_else(|| format!("印の閉じ括弧が無い。形は {FORM}"))?,
            ),
        ),
        None => (rest, None),
    };
    check_key(key)?;
    let (mut start, mut end, mut issues) = (None, None, Vec::<String>::new());
    for t in tag.into_iter().flat_map(|t| t.split(' ')) {
        if let Some(x) = t.strip_prefix("start:") {
            if start.replace(parse_instant(x)?).is_some() {
                return Err("start: は 1 つだけ".into());
            }
        } else if let Some(x) = t.strip_prefix("end:") {
            if end.replace(parse_instant(x)?).is_some() {
                return Err("end: は 1 つだけ".into());
            }
        } else if let Some(x) = t.strip_prefix("issue:") {
            match Reference::parse(x) {
                Some(r) if r.number.is_some() => {
                    if issues.iter().any(|i| i == x) {
                        return Err(format!("issue: が重なっている: '{x}'"));
                    }
                    issues.push(x.to_string());
                }
                _ => return Err(format!("issue: は `OWNER/REPO#N`: '{x}'")),
            }
        } else {
            return Err(format!(
                "印に未知の項目: '{t}'(使えるのは start: と end: と issue:)"
            ));
        }
    }
    match event {
        "plan" | "resched" => {
            let (Some(s), Some(e)) = (&start, &end) else {
                return Err(format!("{event} には start: と end: が要る。形は {FORM}"));
            };
            if s.epoch >= e.epoch {
                return Err(format!(
                    "end: は start: より後の時刻にする(start:{} end:{})",
                    s.text, e.text
                ));
            }
            if event == "resched" && !issues.is_empty() {
                return Err("issue: を付けられるのは plan だけ".into());
            }
        }
        "done" | "drop" => {
            if tag.is_some() {
                return Err(format!("{event} に印は付けない。形は {FORM}"));
            }
        }
        other => return Err(format!("出来事 {other} の文法が slot.rs に無い")),
    }
    Ok(SlotLine {
        event: event.to_string(),
        key: key.to_string(),
        start,
        end,
        issues,
    })
}

/// 書き込む本文の正規の形。フラグの順や空白に依らず、同じ内容は同じ本文になる。
pub fn format_slot(l: &SlotLine) -> String {
    let mut s = format!("{}: {}", l.event, l.key);
    let mut tag = Vec::new();
    if let Some(x) = &l.start {
        tag.push(format!("start:{}", x.text));
    }
    if let Some(x) = &l.end {
        tag.push(format!("end:{}", x.text));
    }
    tag.extend(l.issues.iter().map(|i| format!("issue:{i}")));
    if !tag.is_empty() {
        s.push_str(&format!(" [{}]", tag.join(" ")));
    }
    s
}

/// 語彙の外・文法違反・正規でない形を `Err` にする(`lib.rs` の `add` から呼ぶ)。
pub fn validate_slot(text: &str) -> Result<(), String> {
    let line = parse_slot(text)?;
    let canon = format_slot(&line);
    if canon != text {
        return Err(format!(
            "slot の本文は正規の形で書く(start・end・issue の順、区切りは空白 1 つ): '{canon}'"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- 畳み込み

#[derive(Debug, Clone)]
pub struct Ev {
    pub day: i64,
    pub time: Option<String>,
    pub text: String,
    pub line: SlotLine,
}

/// 今の枠。
#[derive(Debug, Clone)]
pub struct Slot {
    pub key: String,
    pub state: String,
    pub start: Instant,
    pub end: Instant,
    pub issues: Vec<String>,
    /// 最初の `plan` の本文(同じ命令の再実行を記録済みと見分けるため)。
    pub planned_text: String,
    pub last_day: i64,
}

impl Slot {
    /// `鍵<TAB>開始<TAB>終了<TAB>Issue(`,` 区切り。無ければ `-`)`。
    pub fn tsv(&self) -> String {
        [
            self.key.as_str(),
            self.start.text.as_str(),
            self.end.text.as_str(),
            &if self.issues.is_empty() {
                "-".to_string()
            } else {
                self.issues.join(",")
            },
        ]
        .join("\t")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "state": self.state,
            "start": self.start.text,
            "end": self.end.text,
            "issues": self.issues,
        })
    }
}

#[derive(Debug, Default)]
pub struct Folded {
    pub slots: BTreeMap<String, Slot>,
    pub discrepancies: Vec<Discrepancy>,
}

impl Folded {
    /// 未了(scheduled)の枠。開始の早い順、同じなら鍵の順。
    pub fn open(&self) -> Vec<&Slot> {
        let v = SlotVocabulary::get();
        let mut out: Vec<&Slot> = self
            .slots
            .values()
            .filter(|s| !v.is_terminal(&s.state))
            .collect();
        out.sort_by(|a, b| (a.start.epoch, &a.key).cmp(&(b.start.epoch, &b.key)));
        out
    }

    /// 未了のうち、終了時刻が `now`(unix 秒)より前の枠。終了の古い順、同じなら鍵の順。
    /// 「過ぎた」は終了時刻から導く値で、状態としては保存しない。
    pub fn overdue(&self, now: i64) -> Vec<&Slot> {
        let mut out: Vec<&Slot> = self
            .open()
            .into_iter()
            .filter(|s| s.end.epoch < now)
            .collect();
        out.sort_by(|a, b| (a.end.epoch, &a.key).cmp(&(b.end.epoch, &b.key)));
        out
    }
}

/// `[slot]` の行を、日付・時刻・ノート内の並びの順に並べて解析する。解析できない行は食い違い。
pub fn collect(rows: &[Row]) -> (Vec<Ev>, Vec<Discrepancy>) {
    let mut evs = Vec::new();
    let mut bad = Vec::new();
    for (idx, r) in rows.iter().enumerate() {
        if r.entry.kind != Some(Kind::Slot) {
            continue;
        }
        let Some(day) = parse_date(&r.date) else {
            continue;
        };
        match parse_slot(&r.entry.text) {
            Ok(line) => evs.push((day, r.entry.time.clone(), idx, r.entry.text.clone(), line)),
            Err(m) => bad.push(Discrepancy {
                key: format!("{} {}", r.date, r.entry.text),
                message: format!("解析できない行: {m}"),
            }),
        }
    }
    evs.sort_by(|a, b| {
        (a.0, a.1.as_deref().unwrap_or("00:00"), a.2).cmp(&(
            b.0,
            b.1.as_deref().unwrap_or("00:00"),
            b.2,
        ))
    });
    (
        evs.into_iter()
            .map(|(day, time, _, text, line)| Ev {
                day,
                time,
                text,
                line,
            })
            .collect(),
        bad,
    )
}

/// 遷移の可否は語彙(`from`)だけが決める。許されない出来事は状態を変えず、食い違いに数える。
pub fn fold(events: &[Ev]) -> Folded {
    let vocab = SlotVocabulary::get();
    let mut out = Folded::default();
    for ev in events {
        let def = vocab
            .event(&ev.line.event)
            .expect("parse_slot が語彙を検査済み");
        let key = ev.line.key.clone();
        let problem = match out.slots.get_mut(&key) {
            None if !def.from.iter().any(|f| f == "none") => {
                Some(format!("開いていない枠への {}", def.id))
            }
            None => {
                let (Some(start), Some(end)) = (&ev.line.start, &ev.line.end) else {
                    unreachable!("開く出来事は時間枠を持つ(parse_slot が検査済み)");
                };
                out.slots.insert(
                    key.clone(),
                    Slot {
                        key: key.clone(),
                        state: def.to.clone(),
                        start: start.clone(),
                        end: end.clone(),
                        issues: ev.line.issues.clone(),
                        planned_text: ev.text.clone(),
                        last_day: ev.day,
                    },
                );
                None
            }
            Some(_) if def.from.iter().any(|f| f == "none") => {
                Some(format!("既にある枠への {}(鍵は使い回せない)", def.id))
            }
            Some(slot) if vocab.is_terminal(&slot.state) => {
                Some(format!("終端 {} の後の {}", slot.state, def.id))
            }
            Some(slot) if !def.from.contains(&slot.state) => {
                Some(format!("{} から {} は許されない", slot.state, def.id))
            }
            Some(slot) => {
                if def.to != "same" {
                    slot.state = def.to.clone();
                }
                slot.last_day = ev.day;
                if let (Some(s), Some(e)) = (&ev.line.start, &ev.line.end) {
                    slot.start = s.clone();
                    slot.end = e.clone();
                }
                None
            }
        };
        if let Some(message) = problem {
            out.discrepancies.push(Discrepancy { key, message });
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

/// 畳み込んだ今の状態から許される出来事だけを、正規の形で書く。許されなければ書かずに止める。
///
/// 同じ命令の再実行は記録済みにする: 同じ内容の `plan`、今の時間枠と同じ `resched`、既にその終端に
/// 入っている `done` / `drop`。
pub fn add_slot(vault: &Vault, days: i64, time: &str, text: &str) -> Result<SlotAdded, String> {
    let vocab = SlotVocabulary::get();
    let line = parse_slot(text.trim())?;
    let canon = format_slot(&line);
    let def = vocab
        .event(&line.event)
        .expect("parse_slot が語彙を検査済み");
    let (_, folded) = load(vault, days)?;
    let note = vault.note_path(days);
    let recorded = |duplicate| SlotAdded {
        path: note.clone(),
        duplicate,
    };
    let key = &line.key;
    match folded.slots.get(key) {
        None => {
            if !def.from.iter().any(|f| f == "none") {
                return Err(format!("{key} はまだ plan されていない。先に plan で開く"));
            }
        }
        Some(slot) => {
            if def.from.iter().any(|f| f == "none") {
                if slot.planned_text == canon {
                    return Ok(recorded(true));
                }
                return Err(format!(
                    "{key} は既にある({})。鍵は使い回せない。時間枠を変えるなら resched",
                    slot.state
                ));
            }
            if vocab.is_terminal(&slot.state) {
                if slot.state == def.to {
                    return Ok(recorded(true));
                }
                return Err(format!(
                    "{key} は終端 {} に入っていて、{} は書けない",
                    slot.state, def.id
                ));
            }
            if !def.from.contains(&slot.state) {
                return Err(format!("{key} は {} で、{} は書けない", slot.state, def.id));
            }
            if let (Some(s), Some(e)) = (&line.start, &line.end) {
                if slot.start.epoch == s.epoch && slot.end.epoch == e.epoch {
                    return Ok(recorded(true));
                }
            }
        }
    }
    match add_line(vault, days, time, Kind::Slot, &canon, None)? {
        Added::Appended(path) => Ok(SlotAdded {
            path,
            duplicate: false,
        }),
        // 同じ日のノートに同じ本文が既にある(例: 同じ日に A → B → A と組み直す)。状態を変える行を
        // 重ねられないので、黙って「記録済み」にせず止める。
        Added::Duplicate(path) => Err(format!(
            "{} に同じ行が既にある({canon})。状態は変わっていないので、別の時間枠にするか翌日に書く",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_vocabulary_parses_and_matches_the_plan() {
        let v = SlotVocabulary::get();
        assert_eq!(v.event_names(), "plan|resched|done|drop");
        assert!(!v.is_terminal("scheduled"));
        assert!(v.is_terminal("done") && v.is_terminal("dropped"));
        assert!(SlotVocabulary::parse(
            r#"{"states":[],"events":[{"id":"x","from":["nope"],"to":"same","by":"agent"}]}"#
        )
        .is_err());
        // タイマーを動かす書き手(machine)の出来事は置けない。
        assert!(SlotVocabulary::parse(
            r#"{"states":[{"id":"a","terminal":false}],"events":[{"id":"x","from":["a"],"to":"same","by":"machine"}]}"#
        )
        .is_err());
    }

    #[test]
    fn instants_need_an_offset_and_compare_across_offsets() {
        let a = parse_instant("2026-11-02T10:00+09:00").unwrap();
        let b = parse_instant("2026-11-02T01:00:00Z").unwrap();
        let c = parse_instant("2026-11-01T20:00-05:00").unwrap();
        assert_eq!(a.epoch, b.epoch);
        assert_eq!(b.epoch, c.epoch);
        assert_eq!(a.text, "2026-11-02T10:00+09:00");
        assert_eq!(a.epoch, 1_793_581_200);
        for bad in [
            "2026-11-02T10:00",
            "2026-11-02 10:00+09:00",
            "2026-02-30T10:00Z",
            "2026-11-02T24:00Z",
            "2026-11-02T10:60Z",
            "2026-11-02T10:00:00:00Z",
            "2026-11-02T10:00+9:00",
            "2026-11-02T10:00+24:00",
            "2026-11-02T10Z",
            "2026-11-02T10:00+09:00 ",
            "２026-11-02T10:00Z",
        ] {
            assert!(parse_instant(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn grammar_accepts_the_forms_and_rejects_the_rest() {
        let p = parse_slot(
            "plan: reader-bot-resume [start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00 issue:example/repo#12 issue:example/repo#13]",
        )
        .unwrap();
        assert_eq!(p.key, "reader-bot-resume");
        assert_eq!(p.issues, ["example/repo#12", "example/repo#13"]);
        assert_eq!(
            format_slot(&p),
            "plan: reader-bot-resume [start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00 issue:example/repo#12 issue:example/repo#13]"
        );
        assert!(parse_slot("done: reader-bot-resume").is_ok());
        assert!(parse_slot("drop: a1").is_ok());
        // 順が違っても読める(手書き)が、書き込み口は正規の形だけ。
        let swapped = "plan: a [end:2026-11-02T12:00Z start:2026-11-02T10:00Z]";
        assert!(parse_slot(swapped).is_ok());
        assert!(validate_slot(swapped).is_err());
        assert!(validate_slot(&format_slot(&parse_slot(swapped).unwrap())).is_ok());
        for bad in [
            "plan: a",
            "plan: a [start:2026-11-02T10:00Z]",
            "plan: a [start:2026-11-02T12:00Z end:2026-11-02T10:00Z]",
            "plan: a [start:2026-11-02T10:00Z end:2026-11-02T10:00Z]",
            "plan: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z issue:example/repo]",
            "plan: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z issue:example/repo#1 issue:example/repo#1]",
            "plan: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z x:1]",
            "plan: a [start:2026-11-02T10:00Z start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
            "resched: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z issue:example/repo#1]",
            "done: a [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
            "drop: a [issue:example/repo#1]",
            "skip: a",
            "plan: Reader",
            "plan: reader_bot",
            "plan: -a",
            "plan: a--b",
            "plan: reader bot [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
            "done a",
            "plan:  [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]",
        ] {
            assert!(parse_slot(bad).is_err(), "{bad}");
        }
        let long = "a".repeat(SLOT_KEY_MAX_CHARS + 1);
        assert!(parse_slot(&format!("done: {long}")).is_err());
    }
}
