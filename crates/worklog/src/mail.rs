//! `mail` kind の未裁定項目のライフサイクル(docs/agents/mail-lifecycle.md)。
//!
//! 項目の状態は保存せず、vault の当日ノートの `[mail]` 行を出来事として畳み込んで決める
//! (event sourcing)。状態・出来事・遷移・期限の表は
//! `config/agents/mail-lifecycle/vocabulary.json` が唯一の正本で、この crate は `include_str!` で
//! 同じ表を読む(Quint の仕様も同じ JSON から生成する)。行の形は次のとおり。
//!
//! ```text
//! - 09:12 [mail] ask: UNISEC 英語コースの募集 [gmail:19a2b/19a2c due:2026-11-28]
//! ```
//!
//! 出典の印は `[<媒体>:<スレッド>/<受信> [due:YYYY-MM-DD] [to:<移し先>]]`。鍵は
//! 「スレッド + その項目を開いた受信」で、同じ受信を読み直しても項目は増えない。
//! `[none]` は件数をまとめた行(`read` だけ)で、項目にならない。

use crate::{add as add_line, format_date, list, parse_date, Added, Entry, Kind, Row, Vault};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

/// 語彙の正本(`config/agents/mail-lifecycle/vocabulary.json`)。
const VOCABULARY_JSON: &str = include_str!("../../../config/agents/mail-lifecycle/vocabulary.json");

/// この日付より前のノートの `mail` 行は、旧い文法(PR #1256)なので畳み込まない。
pub const MAIL_SINCE: &str = "2026-10-30";
/// `mail` の出典の媒体(閉語彙)。`none` は件数をまとめた行など、特定のスレッドを指さない行。
pub const MAIL_CHANNELS: [&str; 4] = ["gmail", "linkedin", "lapras", "none"];
/// `mail` の要旨の最大文字数。
pub const MAIL_SUMMARY_MAX_CHARS: usize = 120;

// ---------------------------------------------------------------- 語彙

#[derive(Debug, Clone)]
pub struct EventDef {
    pub id: String,
    pub from: Vec<String>,
    /// 遷移先の状態。`same` は状態を変えない(remind)。
    pub to: String,
    /// `agent` | `person` | `machine`。強制するのは `machine`(`sweep` だけが書く)。
    pub by: String,
}

impl EventDef {
    /// 項目をまだ開いていない状態からだけ書ける出来事(read / draft / ask)。読み直しで重ならない。
    pub fn is_pure_opening(&self) -> bool {
        self.from == ["none"]
    }
}

#[derive(Debug)]
pub struct Vocabulary {
    pub events: Vec<EventDef>,
    pub states: Vec<(String, bool)>,
    pub remind_after_days: i64,
    pub stale_after_remind_days: i64,
    pub remind_before_due_days: i64,
}

/// 状態 `(id, 終端か)` の並びと、出来事の並び。
pub(crate) type StatesEvents = (Vec<(String, bool)>, Vec<EventDef>);

/// 語彙 JSON の状態・出来事の部分を読み、from / to が状態にあることを検査する。
/// `mail` と `slot` が同じ形の語彙を読むので共有する(タイマーは `mail` だけが持つ)。
pub(crate) fn parse_states_events(v: &Value) -> Result<StatesEvents, String> {
    let strs = |x: &Value| -> Result<Vec<String>, String> {
        x.as_array()
            .ok_or("配列でない")?
            .iter()
            .map(|s| {
                s.as_str()
                    .map(str::to_string)
                    .ok_or("文字列でない".to_string())
            })
            .collect()
    };
    let field = |o: &Value, k: &str| -> Result<String, String> {
        o.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or(format!("'{k}' が無い"))
    };
    let mut states = Vec::new();
    for s in v["states"].as_array().ok_or("states が無い")? {
        let terminal = s["terminal"].as_bool().ok_or("terminal が無い")?;
        states.push((field(s, "id")?, terminal));
    }
    let mut events = Vec::new();
    for e in v["events"].as_array().ok_or("events が無い")? {
        events.push(EventDef {
            id: field(e, "id")?,
            from: strs(&e["from"])?,
            to: field(e, "to")?,
            by: field(e, "by")?,
        });
    }
    for e in &events {
        for f in &e.from {
            if f != "none" && !states.iter().any(|(s, _)| s == f) {
                return Err(format!("出来事 {} の from '{f}' が状態にない", e.id));
            }
        }
        if e.to != "same" && !states.iter().any(|(s, _)| *s == e.to) {
            return Err(format!("出来事 {} の to '{}' が状態にない", e.id, e.to));
        }
    }
    Ok((states, events))
}

impl Vocabulary {
    pub fn parse(json: &str) -> Result<Vocabulary, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("語彙が JSON でない: {e}"))?;
        let (states, events) = parse_states_events(&v)?;
        let t = |k: &str| -> Result<i64, String> {
            v["timers"][k].as_i64().ok_or(format!("timers.{k} が無い"))
        };
        Ok(Vocabulary {
            events,
            states,
            remind_after_days: t("remind_after_days")?,
            stale_after_remind_days: t("stale_after_remind_days")?,
            remind_before_due_days: t("remind_before_due_days")?,
        })
    }

    pub fn get() -> &'static Vocabulary {
        static V: OnceLock<Vocabulary> = OnceLock::new();
        V.get_or_init(|| {
            Vocabulary::parse(VOCABULARY_JSON).expect("mail-lifecycle の語彙が壊れている")
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

/// 項目の鍵の部品。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId {
    pub thread: String,
    pub receipt: String,
}

/// `move` の移し先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// `owner/repo#N`
    Issue(String),
    /// `cal:YYYY-MM-DD`(カレンダーの【期限】の予定)
    Cal(i64),
    /// `mail:<鍵>`(同じスレッドの新しい項目)
    Mail(String),
}

impl std::fmt::Display for Dest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dest::Issue(r) => f.write_str(r),
            Dest::Cal(d) => write!(f, "cal:{}", format_date(*d)),
            Dest::Mail(k) => write!(f, "mail:{k}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub channel: String,
    pub item: Option<ItemId>,
    pub due: Option<i64>,
    pub to: Option<Dest>,
}

impl Tag {
    /// 項目の鍵 `<媒体>:<スレッド>/<受信>`。`[none]` の行には無い。
    pub fn key(&self) -> Option<String> {
        self.item.as_ref().map(|i| key_of(&self.channel, i))
    }

    pub fn thread_key(&self) -> Option<String> {
        self.item
            .as_ref()
            .map(|i| format!("{}:{}", self.channel, i.thread))
    }
}

fn key_of(channel: &str, i: &ItemId) -> String {
    format!("{channel}:{}/{}", i.thread, i.receipt)
}

fn format_tag(t: &Tag) -> String {
    let mut s = match &t.item {
        Some(i) => key_of(&t.channel, i),
        None => t.channel.clone(),
    };
    if let Some(d) = t.due {
        s.push_str(&format!(" due:{}", format_date(d)));
    }
    if let Some(to) = &t.to {
        s.push_str(&format!(" to:{to}"));
    }
    s
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailLine {
    pub event: String,
    pub summary: String,
    pub tag: Tag,
}

const FORM: &str = "`<出来事>: <要旨> [<媒体>:<スレッド>/<受信> [due:YYYY-MM-DD] [to:<移し先>]]`";

fn id_chars_ok(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '=' | '-'))
}

/// `YYYYMMDDTHHMM`。
fn stamp_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 13
        && b[8] == b'T'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 8 || c.is_ascii_digit())
}

fn parse_key(k: &str) -> Result<(String, ItemId), String> {
    let (channel, rest) = k.split_once(':').ok_or(format!("鍵の形が違う: '{k}'"))?;
    let (thread, receipt) = rest.split_once('/').ok_or(format!("鍵の形が違う: '{k}'"))?;
    check_item(channel, thread, receipt)?;
    Ok((
        channel.to_string(),
        ItemId {
            thread: thread.into(),
            receipt: receipt.into(),
        },
    ))
}

fn check_item(channel: &str, thread: &str, receipt: &str) -> Result<(), String> {
    if !id_chars_ok(thread) || !id_chars_ok(receipt) {
        return Err(format!(
            "スレッドと受信は [A-Za-z0-9_=-] だけ: '{thread}/{receipt}'"
        ));
    }
    if channel != "gmail" && !stamp_ok(receipt) {
        return Err(format!(
            "媒体 {channel} の受信は画面に出る受信日時を分まで `YYYYMMDDTHHMM` で書く: '{receipt}'"
        ));
    }
    Ok(())
}

fn parse_dest(s: &str) -> Result<Dest, String> {
    if let Some(d) = s.strip_prefix("cal:") {
        return parse_date(d)
            .map(Dest::Cal)
            .ok_or(format!("to:cal: が日付でない: '{d}'"));
    }
    if let Some(k) = s.strip_prefix("mail:") {
        parse_key(k)?;
        return Ok(Dest::Mail(k.to_string()));
    }
    match crate::Reference::parse(s) {
        Some(r) if r.number.is_some() => Ok(Dest::Issue(s.to_string())),
        _ => Err(format!(
            "to: は `owner/repo#N` | `cal:YYYY-MM-DD` | `mail:<鍵>` のいずれか: '{s}'"
        )),
    }
}

fn parse_tag(tag: &str) -> Result<Tag, String> {
    let mut tokens = tag.split(' ');
    let first = tokens.next().unwrap_or("");
    let (channel, item) = match first.split_once(':') {
        None => (first, None),
        Some((c, rest)) => {
            let (thread, receipt) = rest.split_once('/').ok_or(format!(
                "出典の印は `{c}:<スレッド>/<受信>` の形: '{first}'"
            ))?;
            check_item(c, thread, receipt)?;
            (
                c,
                Some(ItemId {
                    thread: thread.into(),
                    receipt: receipt.into(),
                }),
            )
        }
    };
    if !MAIL_CHANNELS.contains(&channel) {
        return Err(format!(
            "mail の媒体が閉語彙の外: '{channel}'(使えるのは: {})",
            MAIL_CHANNELS.join("|")
        ));
    }
    match (channel, &item) {
        ("none", Some(_)) => return Err("媒体 none には id を付けない(`[none]`)".into()),
        (c, None) if c != "none" => {
            return Err(format!("媒体 {c} には `{c}:<スレッド>/<受信>` が要る"))
        }
        _ => {}
    }
    let (mut due, mut to) = (None, None);
    for t in tokens {
        if let Some(d) = t.strip_prefix("due:") {
            if due
                .replace(parse_date(d).ok_or(format!("due: が日付でない: '{d}'"))?)
                .is_some()
            {
                return Err("due: は 1 つだけ".into());
            }
        } else if let Some(x) = t.strip_prefix("to:") {
            if to.replace(parse_dest(x)?).is_some() {
                return Err("to: は 1 つだけ".into());
            }
        } else {
            return Err(format!(
                "出典の印に未知の項目: '{t}'(使えるのは due: と to:)"
            ));
        }
    }
    if channel == "none" && (due.is_some() || to.is_some()) {
        return Err("媒体 none には due: も to: も付けない".into());
    }
    Ok(Tag {
        channel: channel.to_string(),
        item,
        due,
        to,
    })
}

/// `mail` の本文を解析し、文法と閉語彙を検査する。
///
/// 記録するのは本人の裁定の結論と出典の印だけで、メールの中身や相手の名前は書かない。
/// 要旨に `@`・`http`・角括弧を許さないのは、アドレスや URL の転記を書けなくするため。
pub fn parse_mail(text: &str) -> Result<MailLine, String> {
    let vocab = Vocabulary::get();
    let open = text
        .rfind('[')
        .filter(|_| text.ends_with(']'))
        .filter(|&i| text[..i].ends_with(' '))
        .ok_or_else(|| format!("mail の末尾に出典の印が要る。形は {FORM}"))?;
    let head = text[..open].trim_end();
    let tag = parse_tag(&text[open + 1..text.len() - 1])?;

    let (event, summary) = head
        .split_once(": ")
        .ok_or_else(|| format!("mail は出来事と要旨が要る。形は {FORM}"))?;
    let def = vocab.event(event).ok_or_else(|| {
        format!(
            "mail の出来事が閉語彙の外: '{event}'(使えるのは: {})",
            vocab.event_names()
        )
    })?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err("mail の要旨が空".into());
    }
    if summary.contains(['@', '＠']) || summary.to_ascii_lowercase().contains("http") {
        return Err("mail の要旨に '@' か 'http' を含めない(アドレス・URL は書かない)".into());
    }
    if summary.contains(['[', ']']) {
        return Err("mail の要旨に角括弧を含めない(出典の印と区別できなくなる)".into());
    }
    let n = summary.chars().count();
    if n > MAIL_SUMMARY_MAX_CHARS {
        return Err(format!(
            "mail の要旨が長い({n} 文字。上限 {MAIL_SUMMARY_MAX_CHARS})。結論だけにする"
        ));
    }
    if tag.channel == "none" && event != "read" {
        return Err("媒体 none は件数をまとめた `read` の行だけ".into());
    }
    match (event, &tag.to) {
        ("move", None) => return Err("move には移し先 `to:<移し先>` が要る".into()),
        (e, Some(_)) if e != "move" => return Err("to: を付けられるのは move だけ".into()),
        _ => {}
    }
    let _ = def;
    Ok(MailLine {
        event: event.to_string(),
        summary: summary.to_string(),
        tag,
    })
}

/// 語彙の外・文法違反を `Err` にする(`lib.rs` の `add` から呼ぶ)。
pub fn validate_mail(text: &str) -> Result<(), String> {
    parse_mail(text).map(|_| ())
}

// ---------------------------------------------------------------- 畳み込み

#[derive(Debug, Clone)]
pub struct Ev {
    pub day: i64,
    pub time: Option<String>,
    pub text: String,
    pub line: MailLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discrepancy {
    pub key: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub thread_key: String,
    pub state: String,
    pub opened_seq: usize,
    pub last_day: i64,
    /// 最後の実質的な出来事より後に書かれた remind の日。実質的な出来事で消える。
    pub remind_day: Option<i64>,
    pub due: Option<i64>,
    pub summary: String,
    pub channel: String,
    pub id: ItemId,
}

impl Item {
    pub fn tag(&self) -> Tag {
        Tag {
            channel: self.channel.clone(),
            item: Some(self.id.clone()),
            due: self.due,
            to: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct Folded {
    pub items: BTreeMap<String, Item>,
    pub discrepancies: Vec<Discrepancy>,
}

/// `[mail]` の行を、日付・時刻・ノート内の並びの順に並べて解析する。解析できない行は食い違い。
pub fn collect(rows: &[Row]) -> (Vec<Ev>, Vec<Discrepancy>) {
    let mut evs = Vec::new();
    let mut bad = Vec::new();
    for (idx, r) in rows.iter().enumerate() {
        if r.entry.kind != Some(Kind::Mail) {
            continue;
        }
        let day = match parse_date(&r.date) {
            Some(d) => d,
            None => continue,
        };
        match parse_mail(&r.entry.text) {
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

/// 受信 `a` が `b` より新しいか。画面の受信日時(固定幅 `YYYYMMDDTHHMM`)は字句順、
/// Gmail のメッセージ id は長さ、字句順(16 進で増える id の想定。単調に増えることは未確認)。
pub fn receipt_newer(a: &str, b: &str) -> bool {
    (a.len(), a) > (b.len(), b)
}

pub fn fold(events: &[Ev]) -> Folded {
    let vocab = Vocabulary::get();
    let mut out = Folded::default();
    for (seq, ev) in events.iter().enumerate() {
        let (Some(key), Some(thread_key)) = (ev.line.tag.key(), ev.line.tag.thread_key()) else {
            continue;
        };
        let def = vocab
            .event(&ev.line.event)
            .expect("parse_mail が語彙を検査済み");
        match out.items.get_mut(&key) {
            None => {
                if !def.from.iter().any(|f| f == "none") {
                    out.discrepancies.push(Discrepancy {
                        key,
                        message: format!("開いていない項目への {}", def.id),
                    });
                    continue;
                }
                let state = def.to.clone();
                let mut item = Item {
                    key: key.clone(),
                    thread_key: thread_key.clone(),
                    state: state.clone(),
                    opened_seq: seq,
                    last_day: ev.day,
                    remind_day: None,
                    due: ev.line.tag.due,
                    summary: ev.line.summary.clone(),
                    channel: ev.line.tag.channel.clone(),
                    id: ev.line.tag.item.clone().expect("key があれば item がある"),
                };
                // 新しい受信を `read`・`draft`・`ask` で開くと、同じスレッドの古い未了は移したとみなす
                // (`read` などで即座に終端になる受信でも同じ。未記録の鍵への `pass` は含めない: 本人が新しい
                // 受信を見送っても、古い未了の判断は残す)。add_mail は先に move を書くので、これは
                // 合流した 2 機体の行の備え。開いた項目が未了で、より新しい受信の未了がすでにあれば、
                // 遅れて届いた古い方の自分が移される。
                if def.is_pure_opening() {
                    for other in out.items.values_mut() {
                        if other.thread_key == thread_key
                            && !vocab.is_terminal(&other.state)
                            && receipt_newer(&item.id.receipt, &other.id.receipt)
                        {
                            other.state = "superseded".into();
                        }
                    }
                    if !vocab.is_terminal(&item.state)
                        && out.items.values().any(|o| {
                            o.thread_key == thread_key
                                && !vocab.is_terminal(&o.state)
                                && receipt_newer(&o.id.receipt, &item.id.receipt)
                        })
                    {
                        item.state = "superseded".into();
                    }
                }
                out.items.insert(key, item);
            }
            Some(item) => {
                if vocab.is_terminal(&item.state) {
                    if !def.is_pure_opening() {
                        out.discrepancies.push(Discrepancy {
                            key,
                            message: format!("終端 {} の後の {}", item.state, def.id),
                        });
                    }
                } else if def.from.contains(&item.state) {
                    item.state = if def.to == "same" {
                        item.state.clone()
                    } else {
                        def.to.clone()
                    };
                    item.last_day = ev.day;
                    if def.id == "remind" {
                        item.remind_day = Some(ev.day);
                    } else {
                        item.remind_day = None;
                        item.summary = ev.line.summary.clone();
                    }
                    if ev.line.tag.due.is_some() {
                        item.due = ev.line.tag.due;
                    }
                } else if !def.is_pure_opening() {
                    out.discrepancies.push(Discrepancy {
                        key,
                        message: format!("{} から {} は許されない", item.state, def.id),
                    });
                }
            }
        }
    }
    out
}

/// 今日までの `[mail]` 行を読んで畳み込む。
pub fn load(vault: &Vault, today: i64) -> Result<(Vec<Ev>, Folded), String> {
    let since = parse_date(MAIL_SINCE).expect("MAIL_SINCE").min(today);
    let rows = list(vault, since, today)?;
    let (evs, bad) = collect(&rows);
    let mut folded = fold(&evs);
    folded.discrepancies.extend(bad);
    Ok((evs, folded))
}

// ---------------------------------------------------------------- 終端の控え

/// 機体ごとの終端の控え(追記専用の JSON Lines)。ノートの終端の行が手で消されたことを
/// 検出するためだけに使い、状態の正本はノートのまま。
pub struct Ledger {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub state: String,
    pub text: String,
}

impl Ledger {
    /// `WORKLOG_STATE_DIR`、無ければ `~/.local/state/claude/worklog`。
    pub fn from_env() -> Result<Ledger, String> {
        let dir = match std::env::var("WORKLOG_STATE_DIR") {
            Ok(d) if !d.is_empty() => PathBuf::from(d),
            _ => PathBuf::from(
                std::env::var("HOME").map_err(|_| "HOME が無く、終端の控えの置き場が決まらない")?,
            )
            .join(".local/state/claude/worklog"),
        };
        Ok(Ledger {
            path: dir.join("mail-terminal"),
        })
    }

    pub fn at(path: PathBuf) -> Ledger {
        Ledger { path }
    }

    fn append(&self, v: &Value) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{} を作れない: {e}", dir.display()))?;
        }
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .map_err(|e| format!("{} を開けない: {e}", self.path.display()))?;
        f.lock()
            .map_err(|e| format!("{} をロックできない: {e}", self.path.display()))?;
        writeln!(f, "{v}").map_err(|e| format!("{} に書けない: {e}", self.path.display()))
    }

    pub fn record(&self, key: &str, state: &str, text: &str) -> Result<(), String> {
        self.append(&json!({"key": key, "state": state, "text": text}))
    }

    pub fn forget(&self, key: &str) -> Result<(), String> {
        self.append(&json!({"key": key, "forgot": true}))
    }

    /// 捨てられていない控え。
    pub fn entries(&self) -> Result<BTreeMap<String, LedgerEntry>, String> {
        let mut s = String::new();
        match fs::File::open(&self.path) {
            Ok(mut f) => {
                f.read_to_string(&mut s)
                    .map_err(|e| format!("{} を読めない: {e}", self.path.display()))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(format!("{} を開けない: {e}", self.path.display())),
        }
        let mut out = BTreeMap::new();
        for line in s.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line)
                .map_err(|e| format!("{} の行が JSON でない: {e}", self.path.display()))?;
            let key = v["key"]
                .as_str()
                .ok_or("控えの行に key が無い")?
                .to_string();
            if v["forgot"].as_bool() == Some(true) {
                out.remove(&key);
            } else {
                out.insert(
                    key,
                    LedgerEntry {
                        state: v["state"]
                            .as_str()
                            .ok_or("控えの行に state が無い")?
                            .to_string(),
                        text: v["text"]
                            .as_str()
                            .ok_or("控えの行に text が無い")?
                            .to_string(),
                    },
                );
            }
        }
        Ok(out)
    }
}

/// 控えにあるのにノートの畳み込みに同じ終端が無い鍵(終端の行が消えた)。`(鍵, 控えの状態, 今の状態)`。
pub fn deleted_terminals(
    folded: &Folded,
    ledger: &BTreeMap<String, LedgerEntry>,
) -> Vec<(String, String, String)> {
    ledger
        .iter()
        .filter_map(|(k, e)| {
            let now = folded.items.get(k).map(|i| i.state.as_str());
            (now != Some(e.state.as_str())).then(|| {
                (
                    k.clone(),
                    e.state.clone(),
                    now.unwrap_or("なし").to_string(),
                )
            })
        })
        .collect()
}

// ---------------------------------------------------------------- 追記

/// 行を書く主体。`machine` の出来事(remind / stale)は `sweep` だけが書ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Agent,
    Machine,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MailAdded {
    pub path: Option<PathBuf>,
    /// 同じ命令の中で先に移した古い項目の鍵。
    pub superseded: Vec<String>,
    /// 同じ出来事が既に記録済み。
    pub duplicate: bool,
}

/// 畳み込んだ今の状態から許される出来事だけを書く。許されなければ書かずに止める。
pub fn add_mail(
    vault: &Vault,
    ledger: &Ledger,
    days: i64,
    time: &str,
    text: &str,
    actor: Actor,
) -> Result<MailAdded, String> {
    let vocab = Vocabulary::get();
    let text = text.trim();
    let line = parse_mail(text)?;
    let def = vocab
        .event(&line.event)
        .expect("parse_mail が語彙を検査済み");
    if def.by == "machine" && actor != Actor::Machine {
        return Err(format!(
            "{} は `worklog mail sweep` だけが書く(機械の出来事)",
            def.id
        ));
    }
    let write =
        |t: &str| -> Result<Added, String> { add_line(vault, days, time, Kind::Mail, t, None) };
    let path_of = |a: Added| match a {
        Added::Appended(p) | Added::Duplicate(p) => p,
    };

    let Some(key) = line.tag.key() else {
        // `[none]` の件数の行は項目にならない。
        let added = write(text)?;
        return Ok(MailAdded {
            duplicate: matches!(added, Added::Duplicate(_)),
            path: Some(path_of(added)),
            superseded: vec![],
        });
    };
    let (evs, folded) = load(vault, days)?;
    // 同じ行が既にある(読み直し・再実行)なら、状態にかかわらず記録済み。
    if evs.iter().any(|e| e.text == text) {
        return Ok(MailAdded {
            path: None,
            superseded: vec![],
            duplicate: true,
        });
    }
    let mut superseded = Vec::new();
    match folded.items.get(&key) {
        Some(item) => {
            if vocab.is_terminal(&item.state) {
                if def.is_pure_opening() {
                    return Ok(MailAdded {
                        path: None,
                        superseded,
                        duplicate: true,
                    });
                }
                return Err(format!(
                    "{key} は終端 {} に入っていて、{} は書けない",
                    item.state, def.id
                ));
            }
            if !def.from.contains(&item.state) {
                if def.is_pure_opening() {
                    return Ok(MailAdded {
                        path: None,
                        superseded,
                        duplicate: true,
                    });
                }
                return Err(format!("{key} は {} で、{} は書けない", item.state, def.id));
            }
        }
        None => {
            if !def.from.iter().any(|f| f == "none") {
                return Err(format!(
                    "{key} はまだ開いていない。先に {} で開く",
                    open_events(vocab)
                ));
            }
            // 終端の行が手で消された項目は、控えがあるので「記録済み」とみなす。読み直した受信を
            // 未記録と見て開き直すと、消された終端が通常の未了に戻ってしまう(仕様 `handled`)。
            if def.is_pure_opening() && ledger.entries()?.contains_key(&key) {
                return Ok(MailAdded {
                    path: None,
                    superseded,
                    duplicate: true,
                });
            }
            let thread_key = line.tag.thread_key().expect("key があれば thread がある");
            let new_receipt = line
                .tag
                .item
                .as_ref()
                .map(|i| i.receipt.as_str())
                .expect("key があれば item がある");
            // 同じスレッドの未了は常に 1 つ。新しい受信を `read`・`draft`・`ask` で開くとき(`read` などで
            // 即座に終端になる受信でも)、古い項目を先に移す。未記録の鍵への `pass` では移さない。新しい受信の未了があれば移さない(開いた項目の方が、畳み込みで移される)。
            for old in folded.items.values().filter(|i| {
                def.is_pure_opening()
                    && i.thread_key == thread_key
                    && !vocab.is_terminal(&i.state)
                    && receipt_newer(new_receipt, &i.id.receipt)
            }) {
                let mv = format!(
                    "move: {} [{} to:mail:{key}]",
                    old.summary,
                    format_tag(&Tag {
                        to: None,
                        ..old.tag()
                    })
                );
                write(&mv)?;
                ledger.record(&old.key, "superseded", &mv)?;
                superseded.push(old.key.clone());
            }
        }
    }
    let added = write(text)?;
    let next = match folded.items.get(&key) {
        Some(item) if def.to == "same" => item.state.clone(),
        _ => def.to.clone(),
    };
    if vocab.is_terminal(&next) && matches!(added, Added::Appended(_)) {
        ledger.record(&key, &next, text)?;
    }
    Ok(MailAdded {
        duplicate: matches!(added, Added::Duplicate(_)),
        path: Some(path_of(added)),
        superseded,
    })
}

fn open_events(v: &Vocabulary) -> String {
    v.events
        .iter()
        .filter(|e| e.is_pure_opening())
        .map(|e| e.id.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

// ---------------------------------------------------------------- sweep

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepAction {
    /// `remind` | `stale`
    pub event: &'static str,
    pub key: String,
    pub text: String,
}

/// 期限と経過日数から、今日書く remind / stale を決める(純関数)。
///
/// - remind まだ: 最後の出来事から `remind_after_days` 日たった、または期限の `remind_before_due_days` 日前。
/// - remind 済み: remind が今日より前で、さらに `stale_after_remind_days` 日たった、または期限を過ぎた。
///
/// stale の前には必ず(実質的な出来事より後の、今日より前の)remind がある。
/// 終端の行が消えた項目は、再通知せずに机で見せるので対象にしない。
pub fn sweep_plan(folded: &Folded, skip: &BTreeSet<String>, today: i64) -> Vec<SweepAction> {
    let v = Vocabulary::get();
    let mut out = Vec::new();
    for item in folded.items.values() {
        if v.is_terminal(&item.state) || skip.contains(&item.key) {
            continue;
        }
        let near_due = item
            .due
            .is_some_and(|d| today >= d - v.remind_before_due_days);
        let past_due = item.due.is_some_and(|d| today > d);
        let event = match item.remind_day {
            None if today - item.last_day >= v.remind_after_days || near_due => "remind",
            Some(r) if r < today && (today - r >= v.stale_after_remind_days || past_due) => "stale",
            _ => continue,
        };
        out.push(SweepAction {
            event: if event == "remind" { "remind" } else { "stale" },
            key: item.key.clone(),
            text: format!("{event}: {} [{}]", item.summary, format_tag(&item.tag())),
        });
    }
    out
}

/// `sweep` の実行。書いた(`dry_run` なら書くはずの)行を返す。
pub fn sweep(
    vault: &Vault,
    ledger: &Ledger,
    days: i64,
    time: &str,
    dry_run: bool,
) -> Result<Vec<SweepAction>, String> {
    let (_, folded) = load(vault, days)?;
    let skip: BTreeSet<String> = deleted_terminals(&folded, &ledger.entries()?)
        .into_iter()
        .map(|(k, _, _)| k)
        .collect();
    let plan = sweep_plan(&folded, &skip, days);
    if !dry_run {
        for a in &plan {
            add_mail(vault, ledger, days, time, &a.text, Actor::Machine)?;
        }
    }
    Ok(plan)
}

/// `Entry` から mail の行を取り出す(試験用)。
pub fn mail_entries(rows: &[Row]) -> impl Iterator<Item = &Entry> {
    rows.iter()
        .map(|r| &r.entry)
        .filter(|e| e.kind == Some(Kind::Mail))
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: i64 = 20_757; // 2026-10-31

    fn ev(day: i64, text: &str) -> Ev {
        Ev {
            day,
            time: None,
            text: text.into(),
            line: parse_mail(text).unwrap(),
        }
    }

    fn state_of(f: &Folded, k: &str) -> String {
        f.items[k].state.clone()
    }

    #[test]
    fn the_embedded_vocabulary_parses_and_matches_the_plan() {
        let v = Vocabulary::get();
        assert_eq!(
            v.event_names(),
            "read|draft|ask|decide|apply|pass|remind|move|stale"
        );
        assert_eq!(
            (
                v.remind_after_days,
                v.stale_after_remind_days,
                v.remind_before_due_days
            ),
            (7, 7, 3)
        );
        for s in ["closed", "dismissed", "stale", "superseded"] {
            assert!(v.is_terminal(s), "{s}");
        }
        for s in ["awaiting", "decided"] {
            assert!(!v.is_terminal(s), "{s}");
        }
        assert!(
            v.event("remind").unwrap().by == "machine" && v.event("stale").unwrap().by == "machine"
        );
        assert!(Vocabulary::parse(r#"{"states":[],"events":[{"id":"x","from":["nope"],"to":"same","by":"agent"}],"timers":{"remind_after_days":1,"stale_after_remind_days":1,"remind_before_due_days":1}}"#).is_err());
    }

    #[test]
    fn grammar_accepts_the_forms_and_rejects_the_rest() {
        let l = parse_mail("ask: 判断の表に載せた [gmail:19a2b/19a2c due:2026-11-28]").unwrap();
        assert_eq!(l.tag.key().as_deref(), Some("gmail:19a2b/19a2c"));
        assert_eq!(l.tag.due, parse_date("2026-11-28"));
        assert!(parse_mail("draft: 返信を用意 [linkedin:2-abc=/20261029T2307]").is_ok());
        assert!(parse_mail("read: 約 490 件を読んだ [none]").is_ok());
        let mv = parse_mail("move: 予算の方針へ [gmail:t/m to:example/repo#564]").unwrap();
        assert_eq!(mv.tag.to, Some(Dest::Issue("example/repo#564".into())));
        assert!(parse_mail("move: 期限へ [gmail:t/m to:cal:2026-11-28]").is_ok());
        assert!(parse_mail("move: 新しい受信へ [gmail:t/m to:mail:gmail:t/n]").is_ok());
        for bad in [
            "read: a@b.c [gmail:t/m]",
            "skip: 読んだ [gmail:t/m]",
            "read: 出典なし",
            "read: 旧い文法 [gmail:abc123]",
            "read: 受信が無い [gmail:t/]",
            "draft: 受信日時が違う [linkedin:abc/2026-10-29]",
            "ask: 日付が違う [gmail:t/m due:2026-02-30]",
            "ask: to は move だけ [gmail:t/m to:example/repo#1]",
            "move: 移し先が無い [gmail:t/m]",
            "apply: none は read だけ [none]",
            "read: none に印は付けない [none due:2026-11-01]",
            "ask: 未知の項目 [gmail:t/m x:1]",
        ] {
            assert!(parse_mail(bad).is_err(), "{bad}");
        }
        let long = "あ".repeat(MAIL_SUMMARY_MAX_CHARS + 1);
        assert!(parse_mail(&format!("ask: {long} [gmail:t/m]")).is_err());
    }

    #[test]
    fn fold_follows_the_vocabulary_and_terminal_absorbs() {
        let f = fold(&[
            ev(D, "ask: 判断の表 [gmail:t1/m1]"),
            ev(D, "decide: やると決めた [gmail:t1/m1]"),
            ev(D + 1, "apply: 設定した [gmail:t1/m1]"),
            ev(D + 2, "pass: 後から断る [gmail:t1/m1]"),
            ev(D + 2, "apply: もう一度 [gmail:t1/m1]"),
        ]);
        assert_eq!(state_of(&f, "gmail:t1/m1"), "closed");
        assert_eq!(f.discrepancies.len(), 2, "{:?}", f.discrepancies);
    }

    #[test]
    fn re_reading_the_same_receipt_does_not_duplicate_or_change_state() {
        let f = fold(&[
            ev(D, "ask: 判断の表 [gmail:t1/m1]"),
            ev(D, "decide: 決めた [gmail:t1/m1]"),
            ev(D + 1, "ask: 読み直し [gmail:t1/m1]"),
            ev(D + 1, "draft: 読み直し [gmail:t1/m1]"),
        ]);
        assert_eq!(f.items.len(), 1);
        assert_eq!(state_of(&f, "gmail:t1/m1"), "decided");
        assert!(f.discrepancies.is_empty(), "{:?}", f.discrepancies);
    }

    #[test]
    fn a_non_opening_event_on_an_unknown_key_is_a_discrepancy() {
        let f = fold(&[ev(D, "apply: 開いていない [gmail:t1/m1]")]);
        assert!(f.items.is_empty());
        assert_eq!(f.discrepancies.len(), 1);
    }

    #[test]
    fn two_unresolved_items_in_one_thread_leave_only_the_later_one() {
        // 2 機体が同じスレッドの別の受信を、それぞれ開いて合流した。
        let f = fold(&[
            ev(D, "ask: 古い [gmail:t1/m1]"),
            ev(D, "ask: 新しい [gmail:t1/m2]"),
        ]);
        assert_eq!(state_of(&f, "gmail:t1/m1"), "superseded");
        assert_eq!(state_of(&f, "gmail:t1/m2"), "awaiting");
        // 遅れて届いた古い受信は、開いた直後に自分が移される(新しい受信が残る)。
        let late = fold(&[
            ev(D, "ask: 新しい [gmail:t1/m2]"),
            ev(D, "ask: 遅れて届いた古い [gmail:t1/m1]"),
        ]);
        assert_eq!(state_of(&late, "gmail:t1/m1"), "superseded");
        assert_eq!(state_of(&late, "gmail:t1/m2"), "awaiting");
        assert!(receipt_newer("20261031T1000", "20261029T2307"));
        assert!(receipt_newer("19a2c", "19a2b") && receipt_newer("10000", "ffff"));
        // 別のスレッドは影響しない。
        let g = fold(&[ev(D, "ask: a [gmail:t1/m1]"), ev(D, "ask: b [gmail:t2/m1]")]);
        assert_eq!(
            g.items.values().filter(|i| i.state == "awaiting").count(),
            2
        );
    }

    #[test]
    fn sweep_reminds_then_stales_and_every_event_resets_the_clock() {
        let base = [ev(D, "ask: 方針 [gmail:t1/m1]")];
        let plan = |evs: &[Ev], today| sweep_plan(&fold(evs), &BTreeSet::new(), today);
        assert!(plan(&base, D + 6).is_empty());
        let p = plan(&base, D + 7);
        assert_eq!((p.len(), p[0].event), (1, "remind"));
        // remind が無いうちは、いくら日がたっても stale にならない(先に remind)。
        assert_eq!(plan(&base, D + 30)[0].event, "remind");
        let reminded = [
            ev(D, "ask: 方針 [gmail:t1/m1]"),
            ev(D + 7, "remind: 方針 [gmail:t1/m1]"),
        ];
        assert!(plan(&reminded, D + 13).is_empty());
        assert_eq!(plan(&reminded, D + 14)[0].event, "stale");
        // 同じ日の再実行では stale にならない。
        assert!(plan(&reminded, D + 7).is_empty());
        // 実質的な出来事は時計と remind を数え直す。
        let decided = [
            ev(D, "ask: 方針 [gmail:t1/m1]"),
            ev(D + 7, "remind: 方針 [gmail:t1/m1]"),
            ev(D + 8, "decide: やる [gmail:t1/m1]"),
        ];
        assert!(plan(&decided, D + 14).is_empty());
        assert_eq!(plan(&decided, D + 15)[0].event, "remind");
    }

    #[test]
    fn sweep_uses_the_due_date_for_an_early_remind_and_a_stale_after_it() {
        let due = D + 20;
        let line = format!("ask: 設定 [gmail:t1/m1 due:{}]", format_date(due));
        let evs = [ev(D, &line)];
        let plan = |evs: &[Ev], today| sweep_plan(&fold(evs), &BTreeSet::new(), today);
        // 期限の 4 日前に開いた項目は、期限の 3 日前までは何も出ない。
        let fresh = [ev(due - 4, &line)];
        assert!(plan(&fresh, due - 4).is_empty());
        let early = plan(&fresh, due - 3);
        assert_eq!(early[0].event, "remind");
        assert!(early[0].text.contains(&format!("due:{}", format_date(due))));
        let reminded = [
            ev(D, &line),
            ev(
                due - 3,
                &format!("remind: 設定 [gmail:t1/m1 due:{}]", format_date(due)),
            ),
        ];
        let _ = &evs;
        assert!(plan(&reminded, due).is_empty());
        assert_eq!(plan(&reminded, due + 1)[0].event, "stale");
        // 期限を過ぎてから開いた項目も、先に remind。
        assert_eq!(
            plan(
                &[ev(
                    due + 5,
                    &format!("ask: 遅れた [gmail:t2/m1 due:{}]", format_date(due))
                )],
                due + 5
            )[0]
            .event,
            "remind"
        );
    }

    #[test]
    fn items_whose_terminal_line_vanished_are_not_reminded() {
        let f = fold(&[ev(D, "ask: 方針 [gmail:t1/m1]")]);
        let skip: BTreeSet<String> = ["gmail:t1/m1".to_string()].into();
        assert!(sweep_plan(&f, &skip, D + 30).is_empty());
        let mut ledger = BTreeMap::new();
        ledger.insert(
            "gmail:t1/m1".to_string(),
            LedgerEntry {
                state: "closed".into(),
                text: "apply: 済み [gmail:t1/m1]".into(),
            },
        );
        assert_eq!(
            deleted_terminals(&f, &ledger),
            [(
                "gmail:t1/m1".to_string(),
                "closed".to_string(),
                "awaiting".to_string()
            )]
        );
        let closed = fold(&[
            ev(D, "ask: 方針 [gmail:t1/m1]"),
            ev(D, "apply: 済み [gmail:t1/m1]"),
        ]);
        assert!(deleted_terminals(&closed, &ledger).is_empty());
    }
}
