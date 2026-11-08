//! wrap-up inbox の vault 側(ADR-1136)。
//!
//! 気づき 1 件 = vault の `inbox/<題名から作った名前>.md` 1 枚。同じ題名(同じ repo)は同じノートに
//! なるので、重複が構造的に起きず、本文の行数がそのまま再発の回数になる。
//!
//! ```text
//! ---
//! title: "<題名>"
//! repo: "owner/repo"
//! status: "open"        # open | fixed | issued | dropped | lapsed
//! issue: ""             # issued にしたときの URL
//! pr: ""                # fixed にしたときの URL
//! first_seen: "YYYY-MM-DD"
//! signal: "..."         # 任意。as-code 化の候補のとき(兆候。codify-signals の語彙、ADR-1276)
//! mechanism: "..."      # 同上(次から防ぐ手段)
//! existing: "..."       # 同上(採用|拡張|自前)
//! drop_reason: "..."    # 任意。signal を持つノートを dropped にしたときの閉じた理由
//! ---
//! - YYYY-MM-DD <detail>
//! ```
//!
//! 書き換えるのは状態が変わるときだけ(一時ファイル + rename)。再発の追記は `O_APPEND`。

use crate::codify::Vocabulary;
use serde_json::Value;
use std::io;
use vault_note::{note_name, short_hash, Locked, Note};

pub const INBOX_DIR: &str = "inbox";

pub const OPEN: &str = "open";
/// `open` のまま放置された気づき(`sweep` だけが書く)。**非終端**: 再発すると `open` に戻り、
/// `--set-status` で `fixed`・`dropped` にも進める。
pub const LAPSED: &str = "lapsed";
pub const STATUSES: [&str; 5] = [OPEN, "fixed", "issued", "dropped", LAPSED];

/// `open` が、最後の行(初出か再発)からこの日数たつと `lapsed` になる(語彙 JSON の
/// `timers.lapse_after_days`。`vocabulary_matches_vault_rules` が一致を確かめる)。
pub const LAPSE_AFTER_DAYS: i64 = 14;

/// `--add` の 1 行(`ts` / `title` / `detail` / 任意の `repo`)。`go` は読まない(裁定は
/// my-cleanup で行うので、起票前の確認を行に持たせる必要が無い)。
///
/// as-code 化の候補(retro の `codify` など)は、`signal`(兆候)・`mechanism`(段)・`existing`
/// (採用|拡張|自前)も構造で持つ。空は「無い」。後から「どの兆候の候補が採用・却下されたか」を
/// 数えるための欄で、`detail` の文には溶かさない(ADR-1276)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub ts: String,
    pub title: String,
    pub detail: String,
    pub repo: String,
    pub signal: String,
    pub mechanism: String,
    pub existing: String,
}

pub fn parse_item(line: &str) -> Result<Item, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("JSON ではない: {e}"))?;
    let get = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let title = get("title");
    if title.is_empty() {
        return Err("title が無い".into());
    }
    let signal = get("signal");
    if !signal.is_empty() && Vocabulary::get().signal(&signal).is_none() {
        return Err(format!(
            "signal は語彙(codify-signals)の id のいずれか: {signal}"
        ));
    }
    Ok(Item {
        ts: get("ts"),
        title,
        detail: get("detail"),
        repo: get("repo"),
        signal,
        mechanism: get("mechanism"),
        existing: get("existing"),
    })
}

// ---------------------------------------------------------------- 日付(JST)

/// 1970-01-01 からの日数 → 暦の日付 `(年, 月, 日)`。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 暦の日付 → 1970-01-01 からの日数(`civil_from_days` の逆)。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD`(暦に実在するもの)の、1970-01-01 からの日数。読めなければ `None`。
pub fn parse_day(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let shaped = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
    if !shaped {
        return None;
    }
    let (y, m, d) = (
        s[..4].parse::<i64>().ok()?,
        s[5..7].parse::<u32>().ok()?,
        s[8..].parse::<u32>().ok()?,
    );
    let days = days_from_civil(y, m, d);
    // 2026-02-30 のような実在しない日付は、往復すると別の日になる。
    (civil_from_days(days) == (y, m, d)).then_some(days)
}

/// unix 秒の JST の日付 `YYYY-MM-DD`。
pub fn jst_date(unix_secs: u64) -> String {
    let (y, m, d) = civil_from_days(((unix_secs + 9 * 3600) / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn jst_today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    jst_date(now)
}

/// `ts`(ISO 8601)の日付部分。読めなければ `today`。
fn row_date(ts: &str, today: &str) -> String {
    let b = ts.as_bytes();
    let ok = b.len() >= 10
        && b[..10].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
    if ok {
        ts[..10].to_string()
    } else {
        today.to_string()
    }
}

fn one_line(detail: &str) -> String {
    detail
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn rel(name: &str) -> String {
    format!("{INBOX_DIR}/{name}.md")
}

// ---------------------------------------------------------------- 書く

#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Created,
    Appended,
    /// 同じ日に同じ内容の行が既にある。
    Duplicate,
}

fn locate(g: &Locked, item: &Item) -> io::Result<(String, Option<Note>)> {
    let base = note_name(&item.title);
    let alt = format!(
        "{base} ~{}",
        short_hash(&format!("{}\0{}", item.repo, item.title))
    );
    for name in [base, alt] {
        match g.read(&rel(&name))? {
            None => return Ok((name, None)),
            Some(text) => {
                let note = Note::parse(&text);
                if note.get("title") == Some(&item.title) && note.get("repo") == Some(&item.repo) {
                    return Ok((name, Some(note)));
                }
            }
        }
    }
    Err(io::Error::other(format!(
        "ノート名が衝突している: {}",
        item.title
    )))
}

/// 気づきを 1 件、ノートに足す。`today` は `ts` が読めないときの日付(JST)。
pub fn add_item(g: &Locked, item: &Item, today: &str) -> io::Result<Added> {
    let date = row_date(&item.ts, today);
    let detail = one_line(&item.detail);
    let row = if detail.is_empty() {
        format!("- {date}")
    } else {
        format!("- {date} {detail}")
    };
    let (name, existing) = locate(g, item)?;
    let path = rel(&name);
    let Some(mut note) = existing else {
        let mut front = vec![
            ("title".into(), item.title.clone()),
            ("repo".into(), item.repo.clone()),
            ("status".into(), OPEN.into()),
            ("issue".into(), String::new()),
            ("pr".into(), String::new()),
            ("first_seen".into(), date),
        ];
        // 空の欄は書かない: 既存のノートの形は変わらず、構造を持つのは as-code 化の候補だけ。
        for (k, v) in [
            ("signal", &item.signal),
            ("mechanism", &item.mechanism),
            ("existing", &item.existing),
        ] {
            if !v.is_empty() {
                front.push((k.into(), v.clone()));
            }
        }
        let note = Note {
            front,
            body: format!("{row}\n"),
        };
        g.replace(&path, &note.render())?;
        return Ok(Added::Created);
    };
    if note.body.lines().any(|l| l.trim_end() == row) {
        return Ok(Added::Duplicate);
    }
    let reopen = matches!(note.get("status"), Some("fixed" | "dropped" | LAPSED));
    if reopen {
        // 直した・捨てた・失効したはずの気づきが再発した。状態を open に戻す(書き直しはこのときだけ)。
        // 失効の時計(`sweep`)は、足したこの行の日付から数え直す。
        note.set("status", OPEN);
        if note.get("drop_reason").is_some() {
            // 捨てた理由は、捨てた判断に付くもの。再発して開き直したら消す。
            note.set("drop_reason", "");
        }
        if !note.body.ends_with('\n') && !note.body.is_empty() {
            note.body.push('\n');
        }
        note.body.push_str(&row);
        note.body.push('\n');
        g.replace(&path, &note.render())?;
    } else {
        g.append_line(&path, &row)?;
    }
    Ok(Added::Appended)
}

// ---------------------------------------------------------------- 読む

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenNote {
    /// vault からの相対パス(`inbox/<名前>.md`)。
    pub note: String,
    pub title: String,
    pub repo: String,
    pub first_seen: String,
    pub rows: Vec<String>,
    pub signal: String,
    pub mechanism: String,
    pub existing: String,
}

fn rows_of(body: &str) -> Vec<String> {
    body.lines()
        .filter(|l| l.starts_with("- "))
        .map(|l| l.trim_end().to_string())
        .collect()
}

fn all_notes(g: &Locked) -> io::Result<Vec<(String, Note)>> {
    let mut out = Vec::new();
    for file in g.list_md(INBOX_DIR)? {
        let path = format!("{INBOX_DIR}/{file}");
        if let Some(text) = g.read(&path)? {
            out.push((path, Note::parse(&text)));
        }
    }
    Ok(out)
}

/// 状態が open のノートを、再発の行数の多い順(同数なら初出が古い順、さらに名前順)に返す。
/// `repo` が `Some` ならその repo のノートだけ。
pub fn list_open(g: &Locked, repo: Option<&str>) -> io::Result<Vec<OpenNote>> {
    let mut v: Vec<OpenNote> = all_notes(g)?
        .into_iter()
        .filter(|(_, n)| n.get("status") == Some(OPEN))
        .filter(|(_, n)| repo.is_none_or(|r| n.get("repo") == Some(r)))
        .map(|(path, n)| OpenNote {
            title: n.get("title").unwrap_or("").to_string(),
            repo: n.get("repo").unwrap_or("").to_string(),
            first_seen: n.get("first_seen").unwrap_or("").to_string(),
            rows: rows_of(&n.body),
            signal: n.get("signal").unwrap_or("").to_string(),
            mechanism: n.get("mechanism").unwrap_or("").to_string(),
            existing: n.get("existing").unwrap_or("").to_string(),
            note: path,
        })
        .collect();
    v.sort_by(|a, b| {
        b.rows
            .len()
            .cmp(&a.rows.len())
            .then_with(|| a.first_seen.cmp(&b.first_seen))
            .then_with(|| a.note.cmp(&b.note))
    });
    Ok(v)
}

/// as-code 化の候補(`signal` を持つノート)の、較正に要る欄だけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateNote {
    pub signal: String,
    pub status: String,
    pub drop_reason: String,
    pub first_seen: String,
}

/// `signal` を持つノートを、状態によらず全部返す。日次の較正(`promotion-detect`)が、兆候ごとの
/// 採用・却下と理由を数えるために読む。
pub fn candidate_notes(g: &Locked) -> io::Result<Vec<CandidateNote>> {
    Ok(all_notes(g)?
        .into_iter()
        .filter_map(|(_, n)| {
            let signal = n.get("signal").filter(|s| !s.is_empty())?.to_string();
            Some(CandidateNote {
                signal,
                status: n.get("status").unwrap_or("").to_string(),
                drop_reason: n.get("drop_reason").unwrap_or("").to_string(),
                first_seen: n.get("first_seen").unwrap_or("").to_string(),
            })
        })
        .collect())
}

// ---------------------------------------------------------------- 失効(open の放置)

/// `sweep` が `lapsed` にした(dry-run なら、する)気づき 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lapsed {
    /// vault からの相対パス(`inbox/<名前>.md`)。
    pub note: String,
    pub title: String,
    pub repo: String,
    /// 最後の行(初出か再発)の日付。
    pub last_seen: String,
    /// `today` から `last_seen` までの日数。
    pub age_days: i64,
}

/// ノートの最後の行(初出か再発)の日付。行の日付のうち最も新しいもの。行に日付が無ければ `first_seen`。
fn last_seen(note: &Note) -> String {
    rows_of(&note.body)
        .iter()
        .filter_map(|r| r.get(2..12))
        .filter(|d| parse_day(d).is_some())
        .max()
        .map(str::to_string)
        .or_else(|| note.get("first_seen").map(str::to_string))
        .unwrap_or_default()
}

/// ノートの最後の行からの経過日数と、その最後の行の日付。`sweep`(失効)と
/// [`oldest_open_age_days`](片付けの提案)が同じ時計を使うための 1 か所。日付を読めなければ `None`。
fn age_since_last_seen(note: &Note, today_days: i64) -> Option<(String, i64)> {
    let seen = last_seen(note);
    parse_day(&seen).map(|d| (seen, today_days - d))
}

/// 状態が `open` のノート(`repo` が `Some` ならその repo だけ)のうち、最後の行(初出か再発)から
/// いちばん日数がたっているものの経過日数。`open` が無い、または日付を読めるノートが無ければ `None`。
///
/// 起動時の片付けの提案が使う。時計は `sweep` の失効と同じ(最後の行の日付)で、初出の日ではない。
/// 再発を続ける気づきは、初出が古くても、まだ生きているものとして数える。
pub fn oldest_open_age_days(
    g: &Locked,
    repo: Option<&str>,
    today: &str,
) -> io::Result<Option<i64>> {
    let Some(today_days) = parse_day(today) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("今日の日付を読めない: {today}"),
        ));
    };
    Ok(all_notes(g)?
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.get("status") == Some(OPEN))
        .filter(|n| repo.is_none_or(|r| n.get("repo") == Some(r)))
        .filter_map(|n| age_since_last_seen(n, today_days).map(|(_, age)| age))
        .max())
}

/// 状態が `open` のまま、最後の行から [`LAPSE_AFTER_DAYS`] 日以上たったノートを `lapsed` にする
/// (全 repo のノートが対象。`today` は JST の `YYYY-MM-DD`)。frontmatter の `status` だけを書き換え、
/// 本文はそのまま残す。`dry_run` なら何も書かず、なるノートだけを返す。
///
/// 冪等: 一度 `lapsed` にしたノートは `open` でないので、繰り返しても二重に書かない。日付を読めない
/// ノートは、判断できないので `open` のまま残す。`fixed`・`dropped`・`issued` には触れない。
/// 返す順は、ノートのパスの昇順。
pub fn sweep(g: &Locked, today: &str, dry_run: bool) -> io::Result<Vec<Lapsed>> {
    let Some(today_days) = parse_day(today) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("今日の日付を読めない: {today}"),
        ));
    };
    let mut out = Vec::new();
    for (path, mut note) in all_notes(g)? {
        if note.get("status") != Some(OPEN) {
            continue;
        }
        let Some((seen, age_days)) = age_since_last_seen(&note, today_days) else {
            continue;
        };
        if age_days < LAPSE_AFTER_DAYS {
            continue;
        }
        let lapsed = Lapsed {
            title: note.get("title").unwrap_or("").to_string(),
            repo: note.get("repo").unwrap_or("").to_string(),
            note: path.clone(),
            last_seen: seen,
            age_days,
        };
        if !dry_run {
            note.set("status", LAPSED);
            g.replace(&path, &note.render())?;
        }
        out.push(lapsed);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 状態を変える

/// 題名(と、あれば repo)で 1 枚を選び、状態を変える。`url` は fixed なら `pr`、issued なら
/// `issue` に書く。frontmatter だけを書き換え、本文はそのまま残す。成功すればノートの相対パス。
pub fn set_status(
    g: &Locked,
    title: &str,
    repo: Option<&str>,
    status: &str,
    url: &str,
) -> Result<String, String> {
    set_status_with_reason(g, title, repo, status, url, None)
}

/// [`set_status`] に、却下の理由(`reason`)を足したもの。`signal` を持つノート(as-code 化の候補)を
/// `dropped` にするときは、閉じた理由(`codify-signals` の `drop_reasons`)が必須になる。後から
/// 「どの兆候の候補が、なぜ却下されたか」を数えるため(ADR-1276)。`signal` を持たない旧い気づきは
/// 理由なしで捨てられる。理由は `dropped` のときだけ付けられる。
pub fn set_status_with_reason(
    g: &Locked,
    title: &str,
    repo: Option<&str>,
    status: &str,
    url: &str,
    reason: Option<&str>,
) -> Result<String, String> {
    let vocab = Vocabulary::get();
    if let Some(r) = reason {
        if status != "dropped" {
            return Err(format!(
                "--reason は dropped のときだけ付けられる: {status}"
            ));
        }
        if !vocab.is_drop_reason(r) {
            return Err(format!(
                "却下の理由は {} のいずれか: {r}",
                vocab.drop_reasons.join(" | ")
            ));
        }
    }
    if status == LAPSED {
        return Err(format!(
            "{LAPSED} は --sweep だけが書く(open の放置の失効。再発で open に戻る): {status}"
        ));
    }
    if !STATUSES.contains(&status) {
        let settable: Vec<&str> = STATUSES.into_iter().filter(|s| *s != LAPSED).collect();
        return Err(format!(
            "状態は {} のいずれか: {status}",
            settable.join(" | ")
        ));
    }
    let mut hits: Vec<(String, Note)> = all_notes(g)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|(_, n)| n.get("title") == Some(title))
        .filter(|(_, n)| repo.is_none_or(|r| n.get("repo") == Some(r)))
        .collect();
    match hits.len() {
        0 => return Err(format!("該当するノートが無い: {title}")),
        1 => {}
        n => {
            return Err(format!(
                "同じ題名のノートが {n} 枚ある。--repo で絞る: {title}"
            ))
        }
    }
    let (path, mut note) = hits.remove(0);
    let is_candidate = note.get("signal").is_some_and(|s| !s.is_empty());
    if status == "dropped" && is_candidate && reason.is_none() {
        return Err(format!(
            "as-code 化の候補(signal を持つノート)を捨てるには --reason が要る: {}",
            vocab.drop_reasons.join(" | ")
        ));
    }
    note.set("status", status);
    match status {
        "fixed" => note.set("pr", url),
        "issued" => note.set("issue", url),
        _ => {}
    }
    if let Some(r) = reason {
        note.set("drop_reason", r);
    } else if note.get("drop_reason").is_some() {
        note.set("drop_reason", "");
    }
    g.replace(&path, &note.render())
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_note::VaultNotes;

    fn vault() -> (tempfile::TempDir, VaultNotes) {
        let d = tempfile::tempdir().unwrap();
        let v = VaultNotes::at(&d.path().join("vault"), &d.path().join("state"));
        (d, v)
    }

    fn item(title: &str, detail: &str, ts: &str) -> Item {
        Item {
            ts: ts.into(),
            title: title.into(),
            detail: detail.into(),
            repo: "o/r".into(),
            ..Item::default()
        }
    }

    /// as-code 化の候補(兆候・手段・既存手段を構造で持つ)。
    fn candidate(title: &str) -> Item {
        Item {
            signal: "copy-drift".into(),
            mechanism: "gate".into(),
            existing: "拡張:pr-gate".into(),
            ..item(title, "d", "2026-10-01")
        }
    }

    #[test]
    fn parse_item_requires_a_title_and_ignores_go() {
        let i =
            parse_item(r#"{"ts":"t","title":" T ","detail":"d","go":"ask","repo":"o/r"}"#).unwrap();
        assert_eq!(
            i,
            Item {
                ts: "t".into(),
                title: "T".into(),
                detail: "d".into(),
                repo: "o/r".into(),
                ..Item::default()
            }
        );
        assert!(parse_item(r#"{"detail":"d"}"#).is_err());
        assert!(parse_item("not json").is_err());
    }

    #[test]
    fn jst_date_crosses_the_utc_boundary() {
        // 2026-10-27T15:30:00Z は JST では 10-28。
        assert_eq!(jst_date(1_793_115_000), "2026-10-28");
        assert_eq!(jst_date(0), "1970-01-01");
    }

    #[test]
    fn the_same_title_makes_one_note_and_recurrences_are_rows() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        let i1 = item("T / x", "first\nsecond line", "2026-10-01T09:00:00+09:00");
        assert_eq!(add_item(&g, &i1, "2026-10-27").unwrap(), Added::Created);
        let i2 = item("T / x", "again", "2026-10-02T09:00:00+09:00");
        assert_eq!(add_item(&g, &i2, "2026-10-27").unwrap(), Added::Appended);
        // 同じ日の同じ内容は足さない(drain の再実行に対して冪等)。
        assert_eq!(add_item(&g, &i2, "2026-10-27").unwrap(), Added::Duplicate);
        assert_eq!(g.list_md(INBOX_DIR).unwrap(), vec!["T - x.md".to_string()]);
        let text = g.read("inbox/T - x.md").unwrap().unwrap();
        assert_eq!(
            text,
            "---\ntitle: \"T / x\"\nrepo: \"o/r\"\nstatus: \"open\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"2026-10-01\"\n---\n\
             - 2026-10-01 first / second line\n- 2026-10-02 again\n"
        );
    }

    #[test]
    fn a_missing_ts_uses_today() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("T", "", ""), "2026-10-27").unwrap();
        assert!(g
            .read("inbox/T.md")
            .unwrap()
            .unwrap()
            .ends_with("---\n- 2026-10-27\n"));
    }

    #[test]
    fn the_same_title_in_another_repo_is_another_note() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("T", "a", "2026-10-01"), "x").unwrap();
        let mut other = item("T", "b", "2026-10-01");
        other.repo = "o/other".into();
        add_item(&g, &other, "x").unwrap();
        assert_eq!(g.list_md(INBOX_DIR).unwrap().len(), 2);
        assert_eq!(list_open(&g, Some("o/other")).unwrap().len(), 1);
        assert_eq!(list_open(&g, None).unwrap().len(), 2);
    }

    #[test]
    fn fixed_and_dropped_reopen_on_recurrence_but_issued_stays() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        for (status, reopened) in [("fixed", true), ("dropped", true), ("issued", false)] {
            let title = format!("T-{status}");
            add_item(&g, &item(&title, "a", "2026-10-01"), "x").unwrap();
            set_status(&g, &title, None, status, "https://example.test/1").unwrap();
            add_item(&g, &item(&title, "b", "2026-10-02"), "x").unwrap();
            let n = Note::parse(&g.read(&format!("inbox/{title}.md")).unwrap().unwrap());
            assert_eq!(n.get("status") == Some(OPEN), reopened, "{status}");
            assert_eq!(rows_of(&n.body).len(), 2, "{status}");
        }
    }

    #[test]
    fn set_status_keeps_the_body_bytes_and_records_the_url() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("T", "a", "2026-10-01"), "x").unwrap();
        g.append_line("inbox/T.md", "手書きの追記").unwrap();
        let before = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        set_status(&g, "T", Some("o/r"), "fixed", "https://example.test/pull/1").unwrap();
        let after = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        assert_eq!(after.body, before.body);
        assert_eq!(after.get("status"), Some("fixed"));
        assert_eq!(after.get("pr"), Some("https://example.test/pull/1"));
        set_status(&g, "T", None, "issued", "https://example.test/issues/2").unwrap();
        let after = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        assert_eq!(after.get("issue"), Some("https://example.test/issues/2"));
        assert!(set_status(&g, "T", None, "weird", "").is_err());
        assert!(set_status(&g, "missing", None, "fixed", "").is_err());
    }

    #[test]
    fn set_status_refuses_an_ambiguous_title() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("T", "a", "2026-10-01"), "x").unwrap();
        let mut other = item("T", "b", "2026-10-01");
        other.repo = "o/other".into();
        add_item(&g, &other, "x").unwrap();
        assert!(set_status(&g, "T", None, "dropped", "")
            .unwrap_err()
            .contains("--repo"));
        set_status(&g, "T", Some("o/other"), "dropped", "").unwrap();
        assert_eq!(list_open(&g, None).unwrap().len(), 1);
    }

    #[test]
    fn list_open_orders_by_recurrence_then_age() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("old once", "a", "2026-09-01"), "x").unwrap();
        add_item(&g, &item("new twice", "a", "2026-10-01"), "x").unwrap();
        add_item(&g, &item("new twice", "b", "2026-10-02"), "x").unwrap();
        add_item(&g, &item("newer once", "a", "2026-10-05"), "x").unwrap();
        add_item(&g, &item("done", "a", "2026-08-01"), "x").unwrap();
        set_status(&g, "done", None, "dropped", "").unwrap();
        let titles: Vec<_> = list_open(&g, None)
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, ["new twice", "old once", "newer once"]);
    }

    #[test]
    fn a_hand_edited_note_with_unquoted_properties_is_still_found() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        g.replace(
            "inbox/hand.md",
            "---\ntitle: hand\nrepo: o/r\nstatus: open\n---\n- 2026-10-01 x\n",
        )
        .unwrap();
        let open = list_open(&g, Some("o/r")).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].rows, ["- 2026-10-01 x"]);
        // 同じ題名の再発は、手書きのノートに足される。
        add_item(&g, &item("hand", "y", "2026-10-02"), "x").unwrap();
        assert_eq!(list_open(&g, None).unwrap()[0].rows.len(), 2);
    }
    #[test]
    fn a_candidates_signal_mechanism_and_existing_reach_the_frontmatter() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &candidate("C"), "x").unwrap();
        let text = g.read("inbox/C.md").unwrap().unwrap();
        let note = Note::parse(&text);
        assert_eq!(note.get("signal"), Some("copy-drift"));
        assert_eq!(note.get("mechanism"), Some("gate"));
        assert_eq!(note.get("existing"), Some("拡張:pr-gate"));
        let open = list_open(&g, None).unwrap();
        assert_eq!(
            (
                open[0].signal.as_str(),
                open[0].mechanism.as_str(),
                open[0].existing.as_str()
            ),
            ("copy-drift", "gate", "拡張:pr-gate")
        );
        // 再発しても、最初の値を保つ。
        let mut again = candidate("C");
        again.signal = "form-check".into();
        again.detail = "second".into();
        add_item(&g, &again, "x").unwrap();
        let text = g.read("inbox/C.md").unwrap().unwrap();
        assert_eq!(Note::parse(&text).get("signal"), Some("copy-drift"));
    }

    #[test]
    fn a_plain_item_leaves_the_frontmatter_unchanged() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("P", "d", "2026-10-01"), "x").unwrap();
        let text = g.read("inbox/P.md").unwrap().unwrap();
        for k in ["signal:", "mechanism:", "existing:", "drop_reason:"] {
            assert!(!text.contains(k), "{text}");
        }
    }

    #[test]
    fn an_unknown_signal_is_refused_when_parsing() {
        let e = parse_item(r#"{"title":"T","signal":"vibes"}"#).unwrap_err();
        assert!(e.contains("signal"), "{e}");
        let ok = parse_item(
            r#"{"title":"T","signal":"copy-drift","mechanism":"gate","existing":"自前"}"#,
        )
        .unwrap();
        assert_eq!(
            (ok.signal.as_str(), ok.existing.as_str()),
            ("copy-drift", "自前")
        );
    }

    #[test]
    fn dropping_a_candidate_needs_a_closed_reason() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &candidate("C"), "x").unwrap();
        // 理由なし・語彙外の理由は拒否。
        let e = set_status(&g, "C", None, "dropped", "").unwrap_err();
        assert!(e.contains("--reason"), "{e}");
        let e = set_status_with_reason(&g, "C", None, "dropped", "", Some("boring")).unwrap_err();
        assert!(e.contains("semantic-judgment"), "{e}");
        assert_eq!(list_open(&g, None).unwrap().len(), 1);
        // 閉じた理由なら捨てられ、ノートに残る。
        set_status_with_reason(&g, "C", None, "dropped", "", Some("one-off")).unwrap();
        let note = Note::parse(&g.read("inbox/C.md").unwrap().unwrap());
        assert_eq!(note.get("status"), Some("dropped"));
        assert_eq!(note.get("drop_reason"), Some("one-off"));
    }

    #[test]
    fn a_legacy_note_can_be_dropped_without_a_reason() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &item("L", "d", "2026-10-01"), "x").unwrap();
        set_status(&g, "L", None, "dropped", "").unwrap();
        let text = g.read("inbox/L.md").unwrap().unwrap();
        assert!(!text.contains("drop_reason"), "{text}");
    }

    #[test]
    fn a_reason_is_only_for_dropped() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &candidate("C"), "x").unwrap();
        let e = set_status_with_reason(&g, "C", None, "fixed", "u", Some("one-off")).unwrap_err();
        assert!(e.contains("dropped"), "{e}");
        assert_eq!(list_open(&g, None).unwrap().len(), 1);
    }

    #[test]
    fn a_recurrence_reopens_a_dropped_candidate_and_forgets_the_reason() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        add_item(&g, &candidate("C"), "x").unwrap();
        set_status_with_reason(&g, "C", None, "dropped", "", Some("preference")).unwrap();
        let mut again = candidate("C");
        again.detail = "back again".into();
        add_item(&g, &again, "x").unwrap();
        let note = Note::parse(&g.read("inbox/C.md").unwrap().unwrap());
        assert_eq!(note.get("status"), Some("open"));
        assert_eq!(note.get("drop_reason"), Some(""));
    }

    // ------------------------------------------------------------ 失効(sweep)

    /// 最後の行が `last` の日付の、open なノートを 1 枚作る。
    fn aged(g: &Locked, title: &str, last: &str) {
        add_item(g, &item(title, "d", last), "x").unwrap();
    }

    fn status_of(g: &Locked, title: &str) -> String {
        let n = Note::parse(&g.read(&format!("inbox/{title}.md")).unwrap().unwrap());
        n.get("status").unwrap().to_string()
    }

    #[test]
    fn parse_day_round_trips_and_refuses_a_date_that_does_not_exist() {
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(
            parse_day("2026-11-08").unwrap() - parse_day("2026-10-25").unwrap(),
            14
        );
        // 月をまたぐ・年をまたぐ・うるう日。
        assert_eq!(
            parse_day("2026-03-01").unwrap() - parse_day("2026-02-28").unwrap(),
            1
        );
        assert_eq!(
            parse_day("2024-03-01").unwrap() - parse_day("2024-02-28").unwrap(),
            2
        );
        assert_eq!(
            parse_day("2027-01-01").unwrap() - parse_day("2026-12-31").unwrap(),
            1
        );
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-1-01",
            "x",
            "",
            "2026-11-08T",
        ] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_open_note_lapses_at_exactly_14_days_not_at_13() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "d13", "2026-10-26");
        aged(&g, "d14", "2026-10-25");
        aged(&g, "d15", "2026-10-24");
        let lapsed = sweep(&g, "2026-11-08", false).unwrap();
        let titles: Vec<_> = lapsed
            .iter()
            .map(|l| (l.title.as_str(), l.age_days))
            .collect();
        assert_eq!(titles, [("d14", 14), ("d15", 15)]);
        assert_eq!(status_of(&g, "d13"), "open");
        assert_eq!(status_of(&g, "d14"), LAPSED);
        assert_eq!(status_of(&g, "d15"), LAPSED);
    }

    #[test]
    fn the_clock_runs_from_the_latest_row_not_from_first_seen() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "old but recurring", "2026-09-01");
        add_item(&g, &item("old but recurring", "again", "2026-11-01"), "x").unwrap();
        assert!(sweep(&g, "2026-11-08", false).unwrap().is_empty());
        assert_eq!(status_of(&g, "old but recurring"), "open");
        // 最後の行から 14 日たてば失効する。
        assert_eq!(sweep(&g, "2026-11-15", false).unwrap().len(), 1);
    }

    #[test]
    fn the_oldest_open_age_uses_the_latest_row_and_skips_closed_notes() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        assert_eq!(oldest_open_age_days(&g, None, "2026-11-08").unwrap(), None);
        // 初出は古いが、再発が新しいノートは、再発の日から数える。
        aged(&g, "recurring", "2026-09-01");
        add_item(&g, &item("recurring", "again", "2026-11-05"), "x").unwrap();
        assert_eq!(
            oldest_open_age_days(&g, None, "2026-11-08").unwrap(),
            Some(3)
        );
        aged(&g, "stale", "2026-11-01");
        assert_eq!(
            oldest_open_age_days(&g, None, "2026-11-08").unwrap(),
            Some(7)
        );
        // 裁定済みのノートは数えない。
        set_status(&g, "stale", None, "dropped", "").unwrap();
        assert_eq!(
            oldest_open_age_days(&g, None, "2026-11-08").unwrap(),
            Some(3)
        );
    }

    #[test]
    fn the_oldest_open_age_can_be_limited_to_one_repo() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "mine", "2026-11-07");
        let mut other = item("other", "d", "2026-10-01");
        other.repo = "o/other".into();
        add_item(&g, &other, "x").unwrap();
        assert_eq!(
            oldest_open_age_days(&g, Some("o/r"), "2026-11-08").unwrap(),
            Some(1)
        );
        assert_eq!(
            oldest_open_age_days(&g, Some("o/other"), "2026-11-08").unwrap(),
            Some(38)
        );
        assert_eq!(
            oldest_open_age_days(&g, None, "2026-11-08").unwrap(),
            Some(38)
        );
    }

    #[test]
    fn the_oldest_open_age_refuses_an_unreadable_today() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        assert!(oldest_open_age_days(&g, None, "not-a-date").is_err());
    }

    #[test]
    fn sweeping_twice_writes_nothing_the_second_time() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "T", "2026-10-01");
        assert_eq!(sweep(&g, "2026-11-08", false).unwrap().len(), 1);
        let once = g.read("inbox/T.md").unwrap().unwrap();
        assert!(sweep(&g, "2026-11-08", false).unwrap().is_empty());
        assert!(sweep(&g, "2026-12-31", false).unwrap().is_empty());
        assert_eq!(g.read("inbox/T.md").unwrap().unwrap(), once);
    }

    #[test]
    fn a_dry_run_reports_without_writing() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "T", "2026-10-01");
        let before = g.read("inbox/T.md").unwrap().unwrap();
        let would = sweep(&g, "2026-11-08", true).unwrap();
        assert_eq!(would.len(), 1);
        assert_eq!(would[0].last_seen, "2026-10-01");
        assert_eq!(g.read("inbox/T.md").unwrap().unwrap(), before);
        assert_eq!(status_of(&g, "T"), "open");
    }

    #[test]
    fn lapsing_changes_only_the_status_and_leaves_the_body_bytes() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "T", "2026-10-01");
        g.append_line("inbox/T.md", "手書きの追記").unwrap();
        let before = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        sweep(&g, "2026-11-08", false).unwrap();
        let after = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        assert_eq!(after.body, before.body);
        assert_eq!(after.get("status"), Some(LAPSED));
        let keys = |n: &Note| n.front.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&after), keys(&before));
    }

    #[test]
    fn sweep_only_touches_open_notes_in_every_repo() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        for status in ["fixed", "issued", "dropped"] {
            aged(&g, status, "2026-10-01");
            set_status(&g, status, None, status, "https://example.test/1").unwrap();
        }
        let mut other = item("other repo", "d", "2026-10-01");
        other.repo = "o/other".into();
        add_item(&g, &other, "x").unwrap();
        aged(&g, "this repo", "2026-10-01");
        let lapsed: Vec<_> = sweep(&g, "2026-11-08", false)
            .unwrap()
            .into_iter()
            .map(|l| (l.repo, l.title))
            .collect();
        assert_eq!(
            lapsed,
            [
                ("o/other".to_string(), "other repo".to_string()),
                ("o/r".to_string(), "this repo".to_string())
            ]
        );
        for status in ["fixed", "issued", "dropped"] {
            assert_eq!(status_of(&g, status), status);
        }
    }

    #[test]
    fn a_note_whose_dates_cannot_be_read_stays_open() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        g.replace(
            "inbox/odd.md",
            "---\ntitle: odd\nrepo: o/r\nstatus: open\nfirst_seen: yesterday\n---\n- no date here\n",
        )
        .unwrap();
        assert!(sweep(&g, "2026-11-08", false).unwrap().is_empty());
        assert!(sweep(&g, "not a date", false).is_err());
    }

    #[test]
    fn a_recurrence_reopens_a_lapsed_note_and_restarts_the_clock() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "T", "2026-10-01");
        sweep(&g, "2026-11-08", false).unwrap();
        assert_eq!(status_of(&g, "T"), LAPSED);
        // list_open にも pending にも出ない(再通知の対象でない)。
        assert!(list_open(&g, None).unwrap().is_empty());
        // 同じ題の再発 → open に戻り、行が足される(別の項目にはしない)。
        assert_eq!(
            add_item(&g, &item("T", "back", "2026-11-10"), "x").unwrap(),
            Added::Appended
        );
        assert_eq!(status_of(&g, "T"), "open");
        let n = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
        assert_eq!(rows_of(&n.body).len(), 2);
        // 時計は再発の行から数え直す: 13 日後はまだ、14 日後に失効。
        assert!(sweep(&g, "2026-11-23", false).unwrap().is_empty());
        assert_eq!(sweep(&g, "2026-11-24", false).unwrap().len(), 1);
    }

    #[test]
    fn a_lapsed_note_can_be_settled_by_the_adjudication_command() {
        for (to, url) in [("fixed", "https://example.test/pull/1"), ("dropped", "")] {
            let (_d, v) = vault();
            let g = v.lock().unwrap();
            aged(&g, "T", "2026-10-01");
            sweep(&g, "2026-11-08", false).unwrap();
            set_status(&g, "T", None, to, url).unwrap();
            assert_eq!(status_of(&g, "T"), to);
            // settled になったので、もう掃除の対象でない。
            assert!(sweep(&g, "2026-12-31", false).unwrap().is_empty());
        }
    }

    #[test]
    fn lapsed_is_written_only_by_the_sweep() {
        let (_d, v) = vault();
        let g = v.lock().unwrap();
        aged(&g, "T", "2026-11-08");
        let e = set_status(&g, "T", None, LAPSED, "").unwrap_err();
        assert!(e.contains("--sweep"), "{e}");
        assert_eq!(status_of(&g, "T"), "open");
    }

    /// 気づきの語彙(`config/agents/wrapup-inbox/vocabulary.json`)は実装の記述で、実装を駆動しない。
    /// 語彙が実装と食い違わないことを、実装の動きから導いた集合と突き合わせて確かめる:
    /// 状態は `STATUSES`、`set_status` が行き着く先(`lapsed` は `sweep` だけが書く)は fix・file・drop の
    /// `to`、`sweep` が行き着く先と、その入口の状態は `lapse` の `to` と `from`、時計の日数は
    /// `timers.lapse_after_days`、再発で open に戻る状態は `reopen` の `from`、戻らず事象も持たない状態
    /// (終端)は `issued` だけ。
    #[test]
    fn vocabulary_matches_vault_rules() {
        const VOCABULARY_JSON: &str =
            include_str!("../../../config/agents/wrapup-inbox/vocabulary.json");
        let vocab: Value = serde_json::from_str(VOCABULARY_JSON).unwrap();
        let strings = |v: &Value| -> Vec<String> {
            let mut out: Vec<String> = v
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect();
            out.sort();
            out
        };
        let event = |id: &str| -> Value {
            vocab["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["id"] == id)
                .unwrap_or_else(|| panic!("事象 {id} が語彙に無い"))
                .clone()
        };

        // 状態
        let mut statuses: Vec<String> = STATUSES.iter().map(|s| s.to_string()).collect();
        statuses.sort();
        let ids: Vec<Value> = vocab["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].clone())
            .collect();
        assert_eq!(strings(&Value::Array(ids)), statuses);

        // set_status の行き先(open と lapsed 以外)は、fix・file・drop の to
        let mut targets: Vec<String> = ["fix", "file", "drop"]
            .iter()
            .flat_map(|id| strings(&event(id)["to"]))
            .collect();
        targets.sort();
        let mut non_open: Vec<String> = statuses.iter().filter(|s| *s != OPEN).cloned().collect();
        non_open.sort();
        let mut settable: Vec<String> = non_open.iter().filter(|s| *s != LAPSED).cloned().collect();
        settable.sort();
        assert_eq!(targets, settable);

        // 失効(sweep の実際の動き): open だけが lapsed になり、ほかの状態は動かない。時計の日数は語彙の timers と一致する。
        assert_eq!(
            vocab["timers"]["lapse_after_days"].as_i64(),
            Some(LAPSE_AFTER_DAYS)
        );
        let mut lapse_from = Vec::new();
        for status in STATUSES {
            let (_d, v) = vault();
            let g = v.lock().unwrap();
            let title = format!("T-{status}");
            add_item(&g, &item(&title, "a", "2026-10-01"), "x").unwrap();
            if status != OPEN && status != LAPSED {
                set_status(&g, &title, None, status, "https://example.test/1").unwrap();
            }
            if status == LAPSED {
                sweep(&g, "2026-11-08", false).unwrap();
            }
            let before = Note::parse(&g.read(&format!("inbox/{title}.md")).unwrap().unwrap());
            sweep(&g, "2026-11-30", false).unwrap();
            let after = Note::parse(&g.read(&format!("inbox/{title}.md")).unwrap().unwrap());
            if before.get("status") != after.get("status") {
                assert_eq!(after.get("status"), Some(LAPSED));
                lapse_from.push(status.to_string());
            }
        }
        lapse_from.sort();
        let lapse = event("lapse");
        assert_eq!(strings(&lapse["from"]), lapse_from);
        assert_eq!(strings(&lapse["to"]), vec![LAPSED.to_string()]);
        assert_eq!(lapse["by"], "machine");

        // 裁定の手順(set_status)で fixed・dropped に進める状態は、open と lapsed。fix・drop の from がそれ。
        let mut settle_from = vec![OPEN.to_string(), LAPSED.to_string()];
        settle_from.sort();
        for (id, to) in [("fix", "fixed"), ("drop", "dropped")] {
            for from in [OPEN, LAPSED] {
                let (_d, v) = vault();
                let g = v.lock().unwrap();
                add_item(&g, &item("T", "a", "2026-10-01"), "x").unwrap();
                if from == LAPSED {
                    sweep(&g, "2026-11-08", false).unwrap();
                }
                set_status(&g, "T", None, to, "").unwrap();
                let n = Note::parse(&g.read("inbox/T.md").unwrap().unwrap());
                assert_eq!(n.get("status"), Some(to), "{from} -> {to}");
            }
            assert_eq!(strings(&event(id)["from"]), settle_from, "{id}");
        }

        // 再発で open に戻る状態(add_item の実際の動き)
        let mut reopened = Vec::new();
        for status in STATUSES.iter().filter(|s| **s != OPEN) {
            let (_d, v) = vault();
            let g = v.lock().unwrap();
            let title = format!("T-{status}");
            add_item(&g, &item(&title, "a", "2026-10-01"), "x").unwrap();
            if *status == LAPSED {
                sweep(&g, "2026-11-08", false).unwrap();
            } else {
                set_status(&g, &title, None, status, "https://example.test/1").unwrap();
            }
            add_item(&g, &item(&title, "b", "2026-10-02"), "x").unwrap();
            let n = Note::parse(&g.read(&format!("inbox/{title}.md")).unwrap().unwrap());
            if n.get("status") == Some(OPEN) {
                reopened.push(status.to_string());
            }
        }
        reopened.sort();
        let reopen = event("reopen");
        assert_eq!(strings(&reopen["from"]), reopened);
        assert_eq!(strings(&reopen["to"]), vec![OPEN.to_string()]);

        // 終端は、再発で戻らない(open でも reopened でもない)状態だけ
        let mut terminal: Vec<String> = vocab["states"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["terminal"] == true)
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect();
        terminal.sort();
        let mut expect: Vec<String> = non_open
            .into_iter()
            .filter(|s| !reopened.contains(s))
            .collect();
        expect.sort();
        assert_eq!(terminal, expect);
        assert_eq!(terminal, vec!["issued".to_string()]);
    }
}
