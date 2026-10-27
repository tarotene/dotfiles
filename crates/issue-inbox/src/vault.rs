//! wrap-up inbox の vault 側(ADR-1136)。
//!
//! 気づき 1 件 = vault の `inbox/<題名から作った名前>.md` 1 枚。同じ題名(同じ repo)は同じノートに
//! なるので、重複が構造的に起きず、本文の行数がそのまま再発の回数になる。
//!
//! ```text
//! ---
//! title: "<題名>"
//! repo: "owner/repo"
//! status: "open"        # open | fixed | issued | dropped
//! issue: ""             # issued にしたときの URL
//! pr: ""                # fixed にしたときの URL
//! first_seen: "YYYY-MM-DD"
//! ---
//! - YYYY-MM-DD <detail>
//! ```
//!
//! 書き換えるのは状態が変わるときだけ(一時ファイル + rename)。再発の追記は `O_APPEND`。

use serde_json::Value;
use std::io;
use vault_note::{note_name, short_hash, Locked, Note};

pub const INBOX_DIR: &str = "inbox";

pub const OPEN: &str = "open";
pub const STATUSES: [&str; 4] = [OPEN, "fixed", "issued", "dropped"];

/// `--add` の 1 行(`ts` / `title` / `detail` / 任意の `repo`)。`go` は読まない(裁定は
/// backlog-cleanup で行うので、起票前の確認を行に持たせる必要が無い)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub ts: String,
    pub title: String,
    pub detail: String,
    pub repo: String,
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
    Ok(Item {
        ts: get("ts"),
        title,
        detail: get("detail"),
        repo: get("repo"),
    })
}

// ---------------------------------------------------------------- 日付(JST)

fn civil_from_days(z: i64) -> (i64, u32, u32) {
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
        let note = Note {
            front: vec![
                ("title".into(), item.title.clone()),
                ("repo".into(), item.repo.clone()),
                ("status".into(), OPEN.into()),
                ("issue".into(), String::new()),
                ("pr".into(), String::new()),
                ("first_seen".into(), date),
            ],
            body: format!("{row}\n"),
        };
        g.replace(&path, &note.render())?;
        return Ok(Added::Created);
    };
    if note.body.lines().any(|l| l.trim_end() == row) {
        return Ok(Added::Duplicate);
    }
    let reopen = matches!(note.get("status"), Some("fixed" | "dropped"));
    if reopen {
        // 直した・捨てたはずの気づきが再発した。状態を open に戻す(書き直しはこのときだけ)。
        note.set("status", OPEN);
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
    if !STATUSES.contains(&status) {
        return Err(format!(
            "状態は {} のいずれか: {status}",
            STATUSES.join(" | ")
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
    note.set("status", status);
    match status {
        "fixed" => note.set("pr", url),
        "issued" => note.set("issue", url),
        _ => {}
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
                repo: "o/r".into()
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
}
