//! `offer` — 日程の提示(返信の送信)と仮押さえ(カレンダーの予定)を、1 提案 1 ジャーナルで見張る
//! (docs/agents/slot-offer.md、#1319)。
//!
//! 提示と押さえは別のシステムなので、1 つのトランザクションにはできない。この crate が持つのは、提案ごとの
//! ジャーナル(意図 → 押さえ → 提示 → 確定/取消)の状態機械と、中断したあとの回復の計画だけで、
//! **カレンダーの操作と送信は行わない**(コネクタを呼べるのはエージェントだけ。D11)。スクリプトは「次に何を
//! すべきか」の計画(作る押さえ・消す押さえ・本人に聞くこと)を出し、エージェントが実行して、結果を
//! `worklog offer record` で返す。
//!
//! - 状態と事象の表は `config/agents/slot-offer/vocabulary.json` が唯一の正本。この crate は `include_str!` で
//!   同じ表を読み、遷移の可否はその表で決める。状態・事象は閉じた enum で、`match` は網羅的に書く。
//! - ジャーナルは `~/.local/state/claude/slot-offer/<提案 id>.json`(`WORKLOG_OFFER_DIR` で置き場を上書き)。
//!   追記でなく原子的な置換(一時ファイルに書いて `rename`)で、提案ごとのロックの下で読み書きする。
//!   読み込みは `deny_unknown_fields` で、壊れた・知らない状態のジャーナルは読み込みで落ちる。
//! - メール本文・個人名・宛先の氏名は書かない。持つのは、提案 id・スレッドの識別子・スロットの日時・
//!   押さえの予定 id だけ。

use crate::mail::{parse_states_events, EventDef};
use crate::parse_date;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;

/// 語彙の正本。
const VOCABULARY_JSON: &str = include_str!("../../../config/agents/slot-offer/vocabulary.json");
/// 仮押さえの題名の印の正本(握手の語彙の `candidate` の `token`)。読むだけで、書き換えない。
const HANDSHAKE_JSON: &str =
    include_str!("../../../config/agents/calendar-handshake/vocabulary.json");

/// ジャーナルの形の版。
pub const JOURNAL_VERSION: u32 = 1;
/// 提案 id の最大文字数。
pub const ID_MAX_CHARS: usize = 64;
/// スレッドの識別子・押さえの予定 id の最大文字数。
pub const REF_MAX_CHARS: usize = 128;
/// 1 提案のスロット数の上限(誤入力で巨大なジャーナルを作らない)。
pub const MAX_SLOTS: usize = 50;

// ---------------------------------------------------------------- 状態と事象

/// 提案の状態(語彙の `states` と 1 対 1。試験が突き合わせる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// 意図を記録した。押さえはまだ揃っていない。
    Intended,
    /// 全スロットの押さえを読み戻して確かめた。まだ提示していない。
    Held,
    /// 提示した(送信済みの画面を読み戻した)。
    Offered,
    /// 相手が日を選んだ(終端)。
    Chosen,
    /// 取り消した(終端)。押さえは残っていない。
    Cancelled,
}

impl State {
    pub const ALL: [State; 5] = [
        State::Intended,
        State::Held,
        State::Offered,
        State::Chosen,
        State::Cancelled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            State::Intended => "intended",
            State::Held => "held",
            State::Offered => "offered",
            State::Chosen => "chosen",
            State::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> Option<State> {
        State::ALL.into_iter().find(|x| x.name() == s)
    }
}

/// 提案に起きる事象(語彙の `events` と 1 対 1。試験が突き合わせる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Intend,
    Observe,
    ConfirmHolds,
    LoseHold,
    Present,
    Abort,
    Retract,
    Choose,
}

impl Event {
    pub const ALL: [Event; 8] = [
        Event::Intend,
        Event::Observe,
        Event::ConfirmHolds,
        Event::LoseHold,
        Event::Present,
        Event::Abort,
        Event::Retract,
        Event::Choose,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Event::Intend => "intend",
            Event::Observe => "observe",
            Event::ConfirmHolds => "confirm_holds",
            Event::LoseHold => "lose_hold",
            Event::Present => "present",
            Event::Abort => "abort",
            Event::Retract => "retract",
            Event::Choose => "choose",
        }
    }
}

/// 語彙 JSON(状態と事象)。遷移の表はこれが正本で、コードは同じ表を引く。
#[derive(Debug)]
pub struct OfferVocabulary {
    pub events: Vec<EventDef>,
    pub states: Vec<(String, bool)>,
}

impl OfferVocabulary {
    pub fn parse(json: &str) -> Result<OfferVocabulary, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("語彙が JSON でない: {e}"))?;
        let (states, events) = parse_states_events(&v)?;
        Ok(OfferVocabulary { events, states })
    }

    pub fn get() -> &'static OfferVocabulary {
        static V: OnceLock<OfferVocabulary> = OnceLock::new();
        V.get_or_init(|| {
            OfferVocabulary::parse(VOCABULARY_JSON).expect("slot-offer の語彙が壊れている")
        })
    }

    pub fn event(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }
}

/// `state`(まだ無ければ `None`)で `ev` が起きたときの次の状態。語彙の表にない遷移は `Err`。
pub fn apply(state: Option<State>, ev: Event) -> Result<State, String> {
    let def = OfferVocabulary::get()
        .event(ev.id())
        .ok_or_else(|| format!("語彙に事象 {} が無い", ev.id()))?;
    let from = state.map_or("none", State::name);
    if !def.from.iter().any(|f| f == from) {
        let state_label = state.map_or("(提案なし)", State::name);
        return Err(format!(
            "{state_label} から {} は許されない(許されるのは {} から)",
            ev.id(),
            def.from.join("|")
        ));
    }
    let to = if def.to == "same" { from } else { &def.to };
    State::parse(to).ok_or_else(|| format!("事象 {} の遷移先 '{to}' が状態にない", ev.id()))
}

/// 押さえの題名の印(握手の語彙の `candidate` の `token`。例 `[hold] `)。
pub fn hold_title() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| {
        let v: Value = serde_json::from_str(HANDSHAKE_JSON).expect("握手の語彙が壊れている");
        v["states"]
            .as_array()
            .and_then(|a| a.iter().find(|s| s["id"] == "candidate"))
            .and_then(|s| s["token"].as_str())
            .expect("握手の語彙に candidate の token が無い")
            .to_string()
    })
}

// ---------------------------------------------------------------- ジャーナル

/// 候補の 1 枠。日時は JST の `YYYY-MM-DDTHH:MM`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// 1 から始まる番号(押さえの説明欄の印に入る)。
    pub n: u32,
    pub start: String,
    pub end: String,
    /// 最後の読み戻しで見つかった押さえの予定 id。無ければ `None`。
    pub event_id: Option<String>,
}

/// 1 提案 1 ファイルのジャーナル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub version: u32,
    pub id: String,
    pub state: State,
    /// 宛先の所在(スレッドの識別子)。本文・氏名は書かない。
    pub thread: String,
    pub slots: Vec<Slot>,
    /// 相手が選んだスロットの番号。`chosen` のときだけ空でない。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chosen: Vec<u32>,
}

fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.chars().count() <= ID_MAX_CHARS
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "提案 id は小文字・数字・ハイフンの {ID_MAX_CHARS} 文字まで(先頭はハイフンにしない): '{id}'"
        ))
    }
}

/// スレッドの識別子・予定 id のような、空白も制御文字も含まない 1 語。
fn check_token(what: &str, s: &str) -> Result<(), String> {
    let ok = !s.is_empty()
        && s.chars().count() <= REF_MAX_CHARS
        && s.chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, ',' | '='));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{what} は空白・制御文字・`,`・`=` を含まない 1 語で {REF_MAX_CHARS} 文字まで: '{s}'"
        ))
    }
}

/// `YYYY-MM-DDTHH:MM`。
fn check_datetime(s: &str) -> Result<(), String> {
    let bad = || format!("日時は YYYY-MM-DDTHH:MM の形: '{s}'");
    let (date, time) = s.split_once('T').ok_or_else(bad)?;
    parse_date(date).ok_or_else(bad)?;
    let (h, m) = time.split_once(':').ok_or_else(bad)?;
    let num = |x: &str, max: u32| {
        x.len() == 2
            && x.chars().all(|c| c.is_ascii_digit())
            && x.parse::<u32>().is_ok_and(|v| v <= max)
    };
    if num(h, 23) && num(m, 59) {
        Ok(())
    } else {
        Err(bad())
    }
}

impl Journal {
    /// 形・不変条件の検査。読み込みと書き込みの両方で通す。
    pub fn validate(&self) -> Result<(), String> {
        if self.version != JOURNAL_VERSION {
            return Err(format!(
                "ジャーナルの版が違う: {}(読めるのは {JOURNAL_VERSION})",
                self.version
            ));
        }
        check_id(&self.id)?;
        check_token("スレッドの識別子", &self.thread)?;
        if self.slots.is_empty() || self.slots.len() > MAX_SLOTS {
            return Err(format!("スロットは 1〜{MAX_SLOTS} 件"));
        }
        for (i, s) in self.slots.iter().enumerate() {
            if s.n as usize != i + 1 {
                return Err(format!(
                    "スロットの番号は 1 から連番: {} 番目が {}",
                    i + 1,
                    s.n
                ));
            }
            check_datetime(&s.start)?;
            check_datetime(&s.end)?;
            if s.end <= s.start {
                return Err(format!(
                    "スロット {} の終了が開始以前: {}〜{}",
                    s.n, s.start, s.end
                ));
            }
            if let Some(e) = &s.event_id {
                check_token("押さえの予定 id", e)?;
            }
        }
        match self.state {
            State::Held if self.slots.iter().any(|s| s.event_id.is_none()) => {
                return Err("held なのに、押さえの予定 id が無いスロットがある".into());
            }
            State::Cancelled if self.slots.iter().any(|s| s.event_id.is_some()) => {
                return Err("cancelled なのに、押さえの予定 id が残っている".into());
            }
            State::Held | State::Cancelled | State::Intended | State::Offered | State::Chosen => {}
        }
        match (self.state, self.chosen.is_empty()) {
            (State::Chosen, true) => return Err("chosen なのに、選ばれたスロットが無い".into()),
            (State::Chosen, false) => {}
            (State::Intended | State::Held | State::Offered | State::Cancelled, false) => {
                return Err(format!(
                    "{} に選ばれたスロットは持てない",
                    self.state.name()
                ))
            }
            (State::Intended | State::Held | State::Offered | State::Cancelled, true) => {}
        }
        for n in &self.chosen {
            if !self.slots.iter().any(|s| s.n == *n) {
                return Err(format!("選ばれたスロット {n} がジャーナルに無い"));
            }
        }
        Ok(())
    }

    /// スロット `n` の押さえの説明欄に書く印。再実行で重複しないための鍵。
    pub fn marker(&self, n: u32) -> String {
        format!("[offer:{}#{n}]", self.id)
    }

    /// 提案のすべての押さえを探す検索語(説明欄の部分一致)。
    pub fn search(&self) -> String {
        format!("[offer:{}#", self.id)
    }

    fn all_held(&self) -> bool {
        self.slots.iter().all(|s| s.event_id.is_some())
    }

    fn known_holds(&self) -> BTreeMap<u32, String> {
        self.slots
            .iter()
            .filter_map(|s| s.event_id.clone().map(|e| (s.n, e)))
            .collect()
    }
}

// ---------------------------------------------------------------- 置き場

/// ジャーナルの置き場。
pub struct Store {
    dir: PathBuf,
}

/// `begin` の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Began {
    Created(PathBuf),
    Duplicate(PathBuf),
}

impl Store {
    /// `WORKLOG_OFFER_DIR`、無ければ `~/.local/state/claude/slot-offer`。
    pub fn from_env() -> Result<Store, String> {
        let dir = match std::env::var("WORKLOG_OFFER_DIR") {
            Ok(d) if !d.is_empty() => PathBuf::from(d),
            _ => PathBuf::from(
                std::env::var("HOME").map_err(|_| "HOME が無く、ジャーナルの置き場が決まらない")?,
            )
            .join(".local/state/claude/slot-offer"),
        };
        Ok(Store { dir })
    }

    pub fn at(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// 提案ごとのロックを取る。落とす(スコープを出る)と外れる。
    fn lock(&self, id: &str) -> Result<File, String> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("{} を作れない: {e}", self.dir.display()))?;
        let p = self.dir.join(format!("{id}.lock"));
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&p)
            .map_err(|e| format!("{} を開けない: {e}", p.display()))?;
        f.lock()
            .map_err(|e| format!("{} をロックできない: {e}", p.display()))?;
        Ok(f)
    }

    /// 読む。無ければ `None`。壊れている・知らない状態や項目があるときは `Err`。
    pub fn load(&self, id: &str) -> Result<Option<Journal>, String> {
        check_id(id)?;
        let p = self.path(id);
        let s = match fs::read_to_string(&p) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{} を読めない: {e}", p.display())),
        };
        let j: Journal = serde_json::from_str(&s)
            .map_err(|e| format!("{} が壊れている(読み込めない): {e}", p.display()))?;
        if j.id != id {
            return Err(format!(
                "{} が壊れている: id '{}' がファイル名と違う",
                p.display(),
                j.id
            ));
        }
        j.validate()
            .map_err(|m| format!("{} が壊れている: {m}", p.display()))?;
        Ok(Some(j))
    }

    fn require(&self, id: &str) -> Result<Journal, String> {
        self.load(id)?.ok_or_else(|| {
            format!("提案 {id} のジャーナルが無い(`worklog offer begin` で意図を記録する)")
        })
    }

    /// 一時ファイルに書いて `rename` で置き換える(読む側は古いか新しいかのどちらかだけを見る)。
    fn write(&self, j: &Journal) -> Result<PathBuf, String> {
        j.validate()?;
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("{} を作れない: {e}", self.dir.display()))?;
        let tmp = self.dir.join(format!(".{}.json.tmp", j.id));
        let dest = self.path(&j.id);
        let body = serde_json::to_string_pretty(j).map_err(|e| format!("書き出せない: {e}"))?;
        let mut f = File::create(&tmp).map_err(|e| format!("{} を作れない: {e}", tmp.display()))?;
        f.write_all(body.as_bytes())
            .and_then(|()| f.write_all(b"\n"))
            .and_then(|()| f.sync_all())
            .map_err(|e| format!("{} に書けない: {e}", tmp.display()))?;
        fs::rename(&tmp, &dest)
            .map_err(|e| format!("{} へ置き換えられない: {e}", dest.display()))?;
        Ok(dest)
    }

    /// 意図の記録。同じ内容の再実行は `Duplicate`、違う内容の同じ id は `Err`。
    pub fn begin(
        &self,
        id: &str,
        thread: &str,
        slots: &[(String, String)],
    ) -> Result<Began, String> {
        check_id(id)?;
        let state = apply(None, Event::Intend)?;
        let journal = Journal {
            version: JOURNAL_VERSION,
            id: id.to_string(),
            state,
            thread: thread.to_string(),
            slots: slots
                .iter()
                .enumerate()
                .map(|(i, (start, end))| Slot {
                    n: u32::try_from(i + 1).unwrap_or(u32::MAX),
                    start: start.clone(),
                    end: end.clone(),
                    event_id: None,
                })
                .collect(),
            chosen: Vec::new(),
        };
        journal.validate()?;
        let _lock = self.lock(id)?;
        match self.load(id)? {
            None => self.write(&journal).map(Began::Created),
            Some(old) if old.thread == journal.thread && old.slots_same_times(&journal) => {
                Ok(Began::Duplicate(self.path(id)))
            }
            Some(_) => Err(format!(
                "提案 {id} は別の内容で記録済み。新しい提案には別の id を使う"
            )),
        }
    }

    /// ロックの下で読み、`f` で書き換えて、検査して、置き換える。
    pub fn update<F>(&self, id: &str, f: F) -> Result<Journal, String>
    where
        F: FnOnce(&mut Journal) -> Result<(), String>,
    {
        check_id(id)?;
        let _lock = self.lock(id)?;
        let mut j = self.require(id)?;
        f(&mut j)?;
        self.write(&j)?;
        Ok(j)
    }

    /// 終端でない提案(`all` なら全部)。壊れたジャーナルが 1 つでもあれば `Err`。
    pub fn list(&self, all: bool) -> Result<Vec<Journal>, String> {
        let mut ids = Vec::new();
        match fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd {
                    let name = e
                        .map_err(|e| format!("{} を読めない: {e}", self.dir.display()))?
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    if let Some(id) = name.strip_suffix(".json") {
                        if !id.starts_with('.') {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} を読めない: {e}", self.dir.display())),
        }
        ids.sort();
        let mut out = Vec::new();
        for id in ids {
            let j = self.require(&id)?;
            let terminal = matches!(j.state, State::Chosen | State::Cancelled);
            if all || !terminal {
                out.push(j);
            }
        }
        Ok(out)
    }
}

impl Journal {
    fn slots_same_times(&self, other: &Journal) -> bool {
        self.slots.len() == other.slots.len()
            && self
                .slots
                .iter()
                .zip(&other.slots)
                .all(|(a, b)| a.start == b.start && a.end == b.end)
    }
}

// ---------------------------------------------------------------- 記録(事象の適用)

/// 押さえの読み戻しの結果を記録する。全スロットが揃えば `held`、`held` で欠ければ `intended` に戻る。
/// 状態を変えない場合(`intended` で欠けたまま、`offered` の押さえの出入り)は `observe`。
/// `holds` は、読み戻して見つかった押さえ(スロット番号 → 予定 id)。見つからないスロットは載せない。
pub fn record_holds(
    store: &Store,
    id: &str,
    holds: &BTreeMap<u32, String>,
) -> Result<Journal, String> {
    store.update(id, |j| {
        check_holds_slots(j, holds)?;
        for s in &mut j.slots {
            s.event_id = holds.get(&s.n).cloned();
        }
        let ev = match (j.state, j.all_held()) {
            (State::Intended, true) => Event::ConfirmHolds,
            (State::Held, false) => Event::LoseHold,
            (
                State::Intended | State::Held | State::Offered | State::Chosen | State::Cancelled,
                _,
            ) => Event::Observe,
        };
        j.state = apply(Some(j.state), ev)?;
        Ok(())
    })
}

/// 提示した(送信済みの画面を読み戻した)ことを記録する。押さえが揃った `held` からだけ進める。
pub fn record_offered(store: &Store, id: &str) -> Result<Journal, String> {
    store.update(id, |j| {
        if !j.all_held() {
            return Err("押さえが全スロット揃っていないので、提示済みにできない。\
                 先に `worklog offer record <id> holds` で読み戻した押さえを記録する"
                .into());
        }
        j.state = apply(Some(j.state), Event::Present)?;
        Ok(())
    })
}

/// 相手が日を選んだことを記録する。
pub fn record_chosen(store: &Store, id: &str, chosen: &[u32]) -> Result<Journal, String> {
    store.update(id, |j| {
        if chosen.is_empty() {
            return Err("選ばれたスロットの番号が要る".into());
        }
        if let Some(n) = chosen.iter().find(|n| !j.slots.iter().any(|s| s.n == **n)) {
            return Err(format!("スロット {n} は提案に無い"));
        }
        j.state = apply(Some(j.state), Event::Choose)?;
        j.chosen = chosen.to_vec();
        j.chosen.sort_unstable();
        j.chosen.dedup();
        Ok(())
    })
}

/// 取消を記録する。押さえをすべて消し、読み戻して 1 件も残っていないこと(`holds` が空)が前提。
/// 出た提示の取り下げは `retracted`(本人の判断)でだけ書ける。
pub fn record_cancelled(
    store: &Store,
    id: &str,
    retracted: bool,
    holds: &BTreeMap<u32, String>,
) -> Result<Journal, String> {
    store.update(id, |j| {
        if !holds.is_empty() {
            return Err(format!(
                "押さえが残っている({} 件)ので、取消にできない。先にすべて消して、読み戻す",
                holds.len()
            ));
        }
        let ev = if retracted {
            Event::Retract
        } else {
            Event::Abort
        };
        j.state = apply(Some(j.state), ev)?;
        for s in &mut j.slots {
            s.event_id = None;
        }
        Ok(())
    })
}

fn check_holds_slots(j: &Journal, holds: &BTreeMap<u32, String>) -> Result<(), String> {
    for (n, e) in holds {
        if !j.slots.iter().any(|s| s.n == *n) {
            return Err(format!("スロット {n} は提案に無い"));
        }
        check_token("押さえの予定 id", e)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- 計画

/// 押さえの読み戻しで、その提案の予定がどう見えているべきか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Holds,
    Offered,
    Chosen,
    Cancelled,
}

/// エージェントが実行する 1 手。カレンダーと送信に触れる手は、この crate は実行しない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// 押さえを作る。`description` の印で、再実行しても重複させない(同じ印の予定があれば作らない)。
    CreateHold {
        slot: u32,
        title: String,
        description: String,
        start: String,
        end: String,
    },
    /// 押さえを消す。`event_id` が分からなければ、`marker` を説明欄に持つ予定を探して消す。
    DeleteHold {
        slot: u32,
        event_id: Option<String>,
        marker: String,
    },
    /// 押さえを読み戻す(`search` を説明欄に持つ予定)。`expect` のスロットがすべて揃い、それ以外が無いこと。
    ReadBackHolds { search: String, expect: Vec<u32> },
    /// 送信済みの画面を読み戻して、提示の文面が出ているかを確かめる。
    ReadBackSent {},
    /// 本人の送信前の確認(画面・全文・宛先)を経て、提示を送る。スクリプトの外の手順。
    Present { requires: String },
    /// 本人に状態を見せて聞く。勝手に消したり送ったりしない。
    AskPerson { reason: String },
    /// 選ばれたスロットを確定し、残りを解放する(`slot-hit.sh finalize`)。
    Finalize { chosen: Vec<u32> },
    /// 結果をジャーナルに返す。
    Record { kind: RecordKind, command: String },
    /// することが無い。
    Nothing { reason: String },
}

/// 1 提案の計画。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub id: String,
    pub state: State,
    pub actions: Vec<Action>,
}

impl Plan {
    /// 手の種類の並び(試験用)。
    pub fn tags(&self) -> Vec<&'static str> {
        self.actions
            .iter()
            .map(|a| match a {
                Action::CreateHold { .. } => "create_hold",
                Action::DeleteHold { .. } => "delete_hold",
                Action::ReadBackHolds { .. } => "read_back_holds",
                Action::ReadBackSent {} => "read_back_sent",
                Action::Present { .. } => "present",
                Action::AskPerson { .. } => "ask_person",
                Action::Finalize { .. } => "finalize",
                Action::Record { .. } => "record",
                Action::Nothing { .. } => "nothing",
            })
            .collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("plan は JSON にできる")
    }
}

/// 送信済みの画面を読み戻した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// 提示の文面が出ていた。
    Yes,
    /// 画面は読めて、出ていなかった。
    No,
    /// 読めなかった(ログインの壁・画面が開かない)。確かめられていない。
    Unknown,
}

impl Sent {
    pub fn parse(s: &str) -> Option<Sent> {
        match s {
            "yes" => Some(Sent::Yes),
            "no" => Some(Sent::No),
            "unknown" => Some(Sent::Unknown),
            _ => None,
        }
    }
}

fn rec(j: &Journal, kind: RecordKind) -> Action {
    let id = &j.id;
    let command = match kind {
        RecordKind::Holds => {
            format!("worklog offer record {id} holds --holds <n=予定id,...|none>")
        }
        RecordKind::Offered => format!("worklog offer record {id} offered"),
        RecordKind::Chosen => format!("worklog offer record {id} chosen --slots <n,...>"),
        RecordKind::Cancelled => format!("worklog offer record {id} cancelled --holds none"),
    };
    Action::Record { kind, command }
}

fn create_hold(j: &Journal, n: u32) -> Action {
    let s = &j.slots[(n - 1) as usize];
    Action::CreateHold {
        slot: n,
        title: hold_title().to_string(),
        description: j.marker(n),
        start: s.start.clone(),
        end: s.end.clone(),
    }
}

fn delete_hold(j: &Journal, n: u32, event_id: Option<String>) -> Action {
    Action::DeleteHold {
        slot: n,
        event_id,
        marker: j.marker(n),
    }
}

fn readback_all(j: &Journal) -> Action {
    Action::ReadBackHolds {
        search: j.search(),
        expect: j.slots.iter().map(|s| s.n).collect(),
    }
}

fn readback_none(j: &Journal) -> Action {
    Action::ReadBackHolds {
        search: j.search(),
        expect: Vec::new(),
    }
}

fn ask(reason: &str) -> Action {
    Action::AskPerson {
        reason: reason.to_string(),
    }
}

/// 欠けた押さえを作り直し、読み戻して記録するまでの手。欠けが無ければ空。
fn fill_holds(j: &Journal, holds: &BTreeMap<u32, String>) -> Vec<Action> {
    let missing: Vec<u32> = j
        .slots
        .iter()
        .map(|s| s.n)
        .filter(|n| !holds.contains_key(n))
        .collect();
    if missing.is_empty() {
        return Vec::new();
    }
    let mut a: Vec<Action> = missing.iter().map(|n| create_hold(j, *n)).collect();
    a.push(readback_all(j));
    a.push(rec(j, RecordKind::Holds));
    a
}

/// 取り消す手(押さえを消す → 残りが無いことを読み戻す → 記録)。`targets` は消す押さえ。
fn teardown(j: &Journal, targets: &BTreeMap<u32, Option<String>>, record: bool) -> Vec<Action> {
    let mut a: Vec<Action> = targets
        .iter()
        .map(|(n, e)| delete_hold(j, *n, e.clone()))
        .collect();
    a.push(readback_none(j));
    if record {
        a.push(rec(j, RecordKind::Cancelled));
    }
    a
}

/// 通常の流れの次の手(中断なし。観測は、ジャーナルが持つ最後の読み戻しの結果)。
pub fn next_steps(j: &Journal) -> Plan {
    let known = j.known_holds();
    let actions = match j.state {
        State::Intended => {
            let mut a = fill_holds(j, &known);
            if a.is_empty() {
                // 全スロットに予定 id はあるが、状態は intended のまま。読み戻して確かめ直す。
                a.push(readback_all(j));
                a.push(rec(j, RecordKind::Holds));
            }
            a
        }
        State::Held => vec![
            Action::Present {
                requires:
                    "本人の送信前の確認(画面・全文・宛先)。作った押さえの一覧も同じ確認に載せる"
                        .to_string(),
            },
            Action::ReadBackSent {},
            rec(j, RecordKind::Offered),
        ],
        State::Offered => {
            let mut a = fill_holds(j, &known);
            if a.is_empty() {
                a.push(Action::Nothing {
                    reason:
                        "相手の選択待ち。選ばれたら `worklog offer record <id> chosen` と finalize"
                            .to_string(),
                });
            }
            a
        }
        State::Chosen => vec![Action::Nothing {
            reason: "確定済み".to_string(),
        }],
        State::Cancelled => vec![Action::Nothing {
            reason: "取消済み".to_string(),
        }],
    };
    Plan {
        id: j.id.clone(),
        state: j.state,
        actions,
    }
}

/// 送信に失敗した・本人が送らないと決めたときの巻き戻し。`retract` は出てしまった提示の取り下げ(本人の判断)。
pub fn abort_plan(j: &Journal, retract: bool) -> Result<Plan, String> {
    let ev = if retract {
        Event::Retract
    } else {
        Event::Abort
    };
    apply(Some(j.state), ev)?;
    let targets: BTreeMap<u32, Option<String>> =
        j.slots.iter().map(|s| (s.n, s.event_id.clone())).collect();
    let mut actions = Vec::new();
    if retract {
        actions.push(ask(
            "出てしまった提示を取り下げる。相手への連絡(取り下げの文面を送るか)は本人が決める。押さえを消すのは、その決定のあと",
        ));
    }
    actions.extend(teardown(j, &targets, false));
    actions.push(Action::Record {
        kind: RecordKind::Cancelled,
        command: if retract {
            format!(
                "worklog offer record {} cancelled --retracted --holds none",
                j.id
            )
        } else {
            format!("worklog offer record {} cancelled --holds none", j.id)
        },
    });
    Ok(Plan {
        id: j.id.clone(),
        state: j.state,
        actions,
    })
}

/// 中断のあとの回復。`sent` は送信済みの画面の読み戻し、`holds` は押さえの読み戻し(見つかった分)。
/// 読み戻せなかったときは、勝手に消したり送ったりせず、本人に聞く手を出す。
pub fn recover(j: &Journal, sent: Sent, holds: &BTreeMap<u32, String>) -> Result<Plan, String> {
    check_holds_slots(j, holds)?;
    let observed: BTreeMap<u32, Option<String>> =
        holds.iter().map(|(n, e)| (*n, Some(e.clone()))).collect();
    let actions = match (j.state, sent) {
        // 意図だけ(または押さえが途中まで)。押さえを揃える。提示が先に出ていたら、押さえを揃えてから記録する。
        (State::Intended, Sent::Yes) => {
            let mut a = fill_holds(j, holds);
            if a.is_empty() {
                a.push(readback_all(j));
                a.push(rec(j, RecordKind::Holds));
            }
            a.push(rec(j, RecordKind::Offered));
            a
        }
        (State::Intended, Sent::No) => {
            let mut a = fill_holds(j, holds);
            if a.is_empty() {
                a.push(readback_all(j));
                a.push(rec(j, RecordKind::Holds));
            }
            a.push(ask(
                "提示は出ていない。押さえは揃えた。送るか、取り消すかを本人に聞く(送るなら送信前の確認から)",
            ));
            a
        }
        (State::Intended, Sent::Unknown) | (State::Held, Sent::Unknown) => {
            let mut a = fill_holds(j, holds);
            a.push(ask(
                "送信済みの画面を確かめられなかった。押さえは消さず、送りもしない。状態を見せて、提示が出ているかを本人に聞く",
            ));
            a
        }
        // 押さえを読み戻して確かめたあと、提示前の状態。送信済みなら提示済みにし、出ていなければ巻き戻す。
        (State::Held, Sent::Yes) => {
            let mut a = fill_holds(j, holds);
            a.push(rec(j, RecordKind::Offered));
            a
        }
        (State::Held, Sent::No) => teardown(j, &observed, true),
        // 提示済み。欠けた押さえだけを作り直す。
        (State::Offered, Sent::Yes | Sent::Unknown) => {
            let mut a = fill_holds(j, holds);
            if a.is_empty() {
                a.push(Action::Nothing {
                    reason: "提示済みで、押さえも揃っている。相手の選択待ち".to_string(),
                });
            }
            a
        }
        (State::Offered, Sent::No) => {
            let mut a = fill_holds(j, holds);
            a.push(ask(
                "提示済みと記録しているが、送信済みの画面に文面が無い。押さえは残す。取り下げる(`abort --retract`)か、送り直すかを本人に聞く",
            ));
            a
        }
        // 終端。押さえが残っていれば片付ける。
        (State::Chosen, Sent::Yes | Sent::No | Sent::Unknown) => {
            if holds.keys().any(|n| !j.chosen.contains(n)) {
                vec![Action::Finalize {
                    chosen: j.chosen.clone(),
                }]
            } else {
                vec![Action::Nothing {
                    reason: "確定済み。選ばれていない押さえは残っていない".to_string(),
                }]
            }
        }
        (State::Cancelled, Sent::Yes | Sent::No | Sent::Unknown) => {
            if holds.is_empty() {
                vec![Action::Nothing {
                    reason: "取消済み。押さえは残っていない".to_string(),
                }]
            } else {
                teardown(j, &observed, false)
            }
        }
    };
    Ok(Plan {
        id: j.id.clone(),
        state: j.state,
        actions,
    })
}

// ---------------------------------------------------------------- コマンド行

const USAGE: &str = "worklog offer begin <提案 id> --thread <識別子> --slot <開始>/<終了> [--slot ...]   (日時は YYYY-MM-DDTHH:MM。番号は指定順に 1 から)\n  \
     worklog offer plan <提案 id>\n  \
     worklog offer record <提案 id> holds --holds <n=予定id,...|none>\n  \
     worklog offer record <提案 id> offered\n  \
     worklog offer record <提案 id> chosen --slots <n,...>\n  \
     worklog offer record <提案 id> cancelled [--retracted] --holds none\n  \
     worklog offer recover <提案 id> --sent yes|no|unknown --holds <n=予定id,...|none>\n  \
     worklog offer abort <提案 id> [--retract]\n  \
     worklog offer show <提案 id>\n  \
     worklog offer list [--all]";

fn flag<'a>(it: &mut impl Iterator<Item = &'a String>, name: &str) -> Result<&'a str, String> {
    it.next()
        .map(String::as_str)
        .ok_or_else(|| format!("{name} に値が要る"))
}

/// `none` か `n=予定id,n=予定id`。
pub fn parse_holds(s: &str) -> Result<BTreeMap<u32, String>, String> {
    if s == "none" {
        return Ok(BTreeMap::new());
    }
    let mut out = BTreeMap::new();
    for part in s.split(',') {
        let (n, e) = part
            .split_once('=')
            .ok_or_else(|| format!("--holds は none か n=予定id,...: '{s}'"))?;
        let n: u32 = n
            .parse()
            .map_err(|_| format!("スロット番号が数でない: '{n}'"))?;
        check_token("押さえの予定 id", e)?;
        if out.insert(n, e.to_string()).is_some() {
            return Err(format!("スロット {n} が重複している"));
        }
    }
    Ok(out)
}

fn parse_numbers(s: &str) -> Result<Vec<u32>, String> {
    s.split(',')
        .map(|x| {
            x.parse::<u32>()
                .map_err(|_| format!("スロット番号が数でない: '{x}'"))
        })
        .collect()
}

fn parse_slot_arg(s: &str) -> Result<(String, String), String> {
    let (a, b) = s
        .split_once('/')
        .ok_or_else(|| format!("--slot は <開始>/<終了>: '{s}'"))?;
    Ok((a.to_string(), b.to_string()))
}

fn state_line(j: &Journal) -> String {
    format!("{}\t{}", j.id, j.state.name())
}

fn require_id<'a>(args: &'a [String], what: &str) -> Result<&'a str, String> {
    let id = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| format!("{what} に提案 id が要る\n  {USAGE}"))?;
    check_id(id)?;
    Ok(id)
}

/// `worklog offer <サブコマンド> ...` の本体。標準出力に出す文字列を返す。
pub fn cli(args: &[String], store: &Store) -> Result<String, String> {
    let sub = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| format!("usage:\n  {USAGE}"))?;
    let rest = &args[1..];
    match sub {
        "begin" => {
            let id = require_id(rest, "begin")?;
            let (mut thread, mut slots) = (None, Vec::new());
            let mut it = rest[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--thread" => thread = Some(flag(&mut it, "--thread")?.to_string()),
                    "--slot" => slots.push(parse_slot_arg(flag(&mut it, "--slot")?)?),
                    other => return Err(format!("begin に未知の引数: '{other}'\n  {USAGE}")),
                }
            }
            let thread = thread.ok_or("--thread が要る")?;
            match store.begin(id, &thread, &slots)? {
                Began::Created(p) => Ok(format!("意図を記録した: {}", p.display())),
                Began::Duplicate(p) => Ok(format!("既に記録済み: {}", p.display())),
            }
        }
        "plan" => {
            let id = require_id(rest, "plan")?;
            if rest.len() > 1 {
                return Err(format!("plan に未知の引数\n  {USAGE}"));
            }
            Ok(next_steps(&store.require(id)?).to_json())
        }
        "show" => {
            let id = require_id(rest, "show")?;
            let j = store.require(id)?;
            serde_json::to_string_pretty(&j).map_err(|e| e.to_string())
        }
        "list" => {
            let all = match rest {
                [] => false,
                [a] if a == "--all" => true,
                _ => return Err(format!("list の引数は --all だけ\n  {USAGE}")),
            };
            Ok(store
                .list(all)?
                .iter()
                .map(|j| format!("{}\t{}\t{}", j.id, j.state.name(), j.thread))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "abort" => {
            let id = require_id(rest, "abort")?;
            let retract = match &rest[1..] {
                [] => false,
                [a] if a == "--retract" => true,
                _ => return Err(format!("abort の引数は --retract だけ\n  {USAGE}")),
            };
            Ok(abort_plan(&store.require(id)?, retract)?.to_json())
        }
        "recover" => {
            let id = require_id(rest, "recover")?;
            let (mut sent, mut holds) = (None, None);
            let mut it = rest[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--sent" => {
                        let v = flag(&mut it, "--sent")?;
                        sent = Some(
                            Sent::parse(v)
                                .ok_or_else(|| format!("--sent は yes|no|unknown: '{v}'"))?,
                        );
                    }
                    "--holds" => holds = Some(parse_holds(flag(&mut it, "--holds")?)?),
                    other => return Err(format!("recover に未知の引数: '{other}'\n  {USAGE}")),
                }
            }
            let sent = sent.ok_or("--sent yes|no|unknown が要る(読めなかったときは unknown)")?;
            let holds = holds.ok_or("--holds が要る(押さえが 1 件も見つからなければ none)")?;
            Ok(recover(&store.require(id)?, sent, &holds)?.to_json())
        }
        "record" => {
            let id = require_id(rest, "record")?;
            let what = rest
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| format!("record に対象が要る\n  {USAGE}"))?;
            let mut holds = None;
            let (mut slots, mut retracted) = (None, false);
            let mut it = rest[2..].iter();
            while let Some(a) = it.next() {
                match (what, a.as_str()) {
                    ("holds" | "cancelled", "--holds") => {
                        holds = Some(parse_holds(flag(&mut it, "--holds")?)?);
                    }
                    ("chosen", "--slots") => {
                        slots = Some(parse_numbers(flag(&mut it, "--slots")?)?);
                    }
                    ("cancelled", "--retracted") => retracted = true,
                    (_, other) => {
                        return Err(format!("record {what} に未知の引数: '{other}'\n  {USAGE}"))
                    }
                }
            }
            let j = match what {
                "holds" => record_holds(
                    store,
                    id,
                    &holds.ok_or("--holds が要る(1 件も見つからなければ none)")?,
                )?,
                "offered" => record_offered(store, id)?,
                "chosen" => record_chosen(store, id, &slots.ok_or("--slots が要る")?)?,
                "cancelled" => record_cancelled(
                    store,
                    id,
                    retracted,
                    &holds.ok_or("--holds none が要る(押さえが残っていないことの読み戻し)")?,
                )?,
                other => {
                    return Err(format!(
                        "record の対象が閉語彙の外: '{other}'(holds|offered|chosen|cancelled)"
                    ))
                }
            };
            Ok(state_line(&j))
        }
        other => Err(format!(
            "offer のサブコマンドが閉語彙の外: '{other}'\n  {USAGE}"
        )),
    }
}
