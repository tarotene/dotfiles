//! due-remind のコアロジック(純粋関数のみ)。
//!
//! `${XDG_STATE_HOME:-~/.local/state}/claude/<domain>/<repo-slug>/due.jsonl` という
//! 契約(別の private リポジトリが持つ `docs/adr/333-due-index-contract.md`。
//! `docs/adr/528-due-remind-timer.md` 参照)を読む側。このクレート自身は
//! 「行が2キー(`slug`/`due`)を持つ JSON Lines」という形しか知らず、どの domain が
//! どんな record を持つかは一切知らない — 新しい domain が増えてもここは変更不要
//! (同 ADR 決定1)。ファイル探索・プロセス起動・exit code は main.rs が担い、
//! ここはパースと組み立てだけを持つ(herdr-issue-counts と同じ分離)。
use serde_json::Value;

/// due-index の1行。`id` を優先し、無ければ古い `followup_id`(contacts/ の既存書き手)
/// にフォールバックする — 契約が定める通り(同 ADR 決定2)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub slug: String,
    pub id: Option<String>,
    pub due: String,
}

/// 1行を解釈する。`slug` と `due` が両方非空でなければ `None`(壊れた行は無視して
/// 続行する — 契約は「壊れた行の扱い」を規定していないので、その行1件を諦めるのが
/// 最も安全な既定)。
pub fn parse_row(line: &str) -> Option<Row> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let slug = v.get("slug")?.as_str()?.to_string();
    let due = v.get("due").and_then(|d| d.as_str())?.to_string();
    if slug.is_empty() || due.is_empty() {
        return None;
    }
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("followup_id").and_then(|x| x.as_str()))
        .map(str::to_string);
    Some(Row { slug, id, due })
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// `YYYY-MM-DD`(厳密に4桁-2桁-2桁、実在する日付)を 1970-01-01 起点の日数
/// (エポック日)に変換する。Howard Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" <http://howardhinnant.github.io/date_algorithms.html>
/// (取得 2026-09-28)の `days_from_civil` をそのまま実装したもの — 西暦の日付を
/// 単純な整数の日数差に変換するだけの計算に、日付ライブラリを1つ増やす理由は
/// ない(このクレートが日付を扱うのはここだけ)。
pub fn civil_to_epoch_day(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let [ys, ms, ds] = [parts[0], parts[1], parts[2]];
    if ys.len() != 4 || ms.len() != 2 || ds.len() != 2 {
        return None;
    }
    let y: i64 = ys.parse().ok()?;
    let m: u32 = ms.parse().ok()?;
    let d: u32 = ds.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    let y_ = if m <= 2 { y - 1 } else { y };
    let era = if y_ >= 0 { y_ } else { y_ - 399 } / 400;
    let yoe = y_ - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146097 + doe - 719468)
}

/// 1行を「今日から何日後(負なら超過何日)」に分類する。ウィンドウは
/// 上限のみ(7日先まで) — 超過側は無期限(グリル Q5 決定 (a): 「超過後も毎日」)。
/// ウィンドウ外(8日以上先、または `due` が不正な日付)は `None`(この回の通知には
/// 出さない)。
pub fn days_until(due: &str, today: &str) -> Option<i64> {
    let due_day = civil_to_epoch_day(due)?;
    let today_day = civil_to_epoch_day(today)?;
    let diff = due_day - today_day;
    if diff <= 7 {
        Some(diff)
    } else {
        None
    }
}

/// 表示用の1行。`domain/slug id — due(あと n日 | 本日 | 超過 n日)`。
pub fn format_row(domain: &str, row: &Row, days: i64) -> String {
    let when = match days.cmp(&0) {
        std::cmp::Ordering::Greater => format!("あと{days}日"),
        std::cmp::Ordering::Equal => "本日".to_string(),
        std::cmp::Ordering::Less => format!("超過{}日", -days),
    };
    match &row.id {
        Some(id) => format!("{domain}/{} {id} — {}({when})", row.slug, row.due),
        None => format!("{domain}/{} — {}({when})", row.slug, row.due),
    }
}

/// 表示行の一覧から通知本文を組み立てる。230文字を超える分は末尾を切り、
/// 「…ほかN件」に置き換える(gpg-subkey / git-audit-worktrees の230文字切りと
/// 同じ上限)。空なら `None`(呼び出し側は herdr を叩かない)。
pub fn build_message(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let full = lines.join("、");
    const LIMIT: usize = 230;
    if full.chars().count() <= LIMIT {
        return Some(full);
    }
    // 何行までなら切り詰め後も LIMIT に収まるかを、末尾に足す
    // 「…ほかN件」の長さを差し引きながら後ろから探す。
    for shown in (1..lines.len()).rev() {
        let head = lines[..shown].join("、");
        let suffix = format!("…ほか{}件", lines.len() - shown);
        let candidate_len = head.chars().count() + suffix.chars().count();
        if candidate_len <= LIMIT {
            return Some(format!("{head}{suffix}"));
        }
    }
    // 1行目すら収まらない極端なケースは、そのまま文字数で切る。
    Some(full.chars().take(LIMIT).collect())
}

/// `herdr notification show` の JSON 応答から `shown`/`reason` を取り出す。
/// bash 側(git-audit-worktrees / gpg-subkey)の
/// `jq -e '.. | objects | select(has("shown")) | ...'` と同じく、深さを問わず
/// 最初に見つかった `shown` キーを持つオブジェクトを採用する — herdr の応答が
/// `{"result":{...}}` でラップされているかどうかにこの関数を依存させないため。
pub fn find_shown_reason(v: &Value) -> (Option<bool>, Option<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::Bool(b)) = map.get("shown") {
                let reason = map.get("reason").and_then(|r| r.as_str()).map(String::from);
                return (Some(*b), reason);
            }
            for val in map.values() {
                let found = find_shown_reason(val);
                if found.0.is_some() {
                    return found;
                }
            }
            (None, None)
        }
        Value::Array(arr) => {
            for val in arr {
                let found = find_shown_reason(val);
                if found.0.is_some() {
                    return found;
                }
            }
            (None, None)
        }
        _ => (None, None),
    }
}

/// 1回の実行の結末。exit code は「配信の成否」だけを表す
/// (crates/herdr-issue-counts / crates/detect-drift と同じ考え方、#442)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// ウィンドウ内の行が1件も無い。
    NothingDue,
    /// 今日すでに `shown: true` を得ている(state ファイルの日付が今日)。
    AlreadyShownToday,
    /// 今回 `shown: true` を得た — state を今日の日付に更新する。
    Shown,
    /// herdr が前面に無い等、一過性の理由で出せなかった。次の時刻に再試行する
    /// (state は更新しない)。herdr バイナリ自体が無い場合もここに含める —
    /// 恒久状態になり得るが、`--failed` に張り付かせる理由が無い一時的な
    /// 環境不足として扱う。
    NotShownRetry,
    /// herdr 自身の通知配信が設定で無効化されている(`reason: "disabled"`)。
    /// 一過性ではないので `systemctl --user --failed` に出す。
    Disabled,
}

pub fn exit_code(o: Outcome) -> u8 {
    match o {
        Outcome::Disabled => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_row_prefers_id_over_followup_id() {
        let row = parse_row(r#"{"slug":"2026-09b","id":"report","due":"2026-10-01"}"#).unwrap();
        assert_eq!(row.id, Some("report".into()));
        assert_eq!(row.due, "2026-10-01");
    }

    #[test]
    fn parse_row_falls_back_to_followup_id() {
        let row =
            parse_row(r#"{"slug":"2026-09a","followup_id":"f1","due":"2026-10-02"}"#).unwrap();
        assert_eq!(row.id, Some("f1".into()));
    }

    #[test]
    fn parse_row_allows_missing_id() {
        let row = parse_row(r#"{"slug":"2026-09a","due":"2026-10-02"}"#).unwrap();
        assert_eq!(row.id, None);
    }

    #[test]
    fn parse_row_rejects_missing_due_or_slug() {
        assert!(parse_row(r#"{"slug":"2026-09a"}"#).is_none());
        assert!(parse_row(r#"{"due":"2026-10-02"}"#).is_none());
        assert!(parse_row(r#"{"slug":"","due":"2026-10-02"}"#).is_none());
    }

    #[test]
    fn parse_row_rejects_malformed_json_and_blank_lines() {
        assert!(parse_row("not json").is_none());
        assert!(parse_row("").is_none());
        assert!(parse_row("   ").is_none());
    }

    #[test]
    fn civil_to_epoch_day_matches_known_values() {
        // python3 -c "from datetime import date; print((date(Y,M,D)-date(1970,1,1)).days)"
        // で取得したクロスチェック値(2026-09-28)。
        assert_eq!(civil_to_epoch_day("1970-01-01"), Some(0));
        assert_eq!(civil_to_epoch_day("2000-01-01"), Some(10957));
        assert_eq!(civil_to_epoch_day("2026-09-24"), Some(20720));
        assert_eq!(civil_to_epoch_day("2026-09-28"), Some(20724));
        assert_eq!(civil_to_epoch_day("2026-10-01"), Some(20727));
        assert_eq!(civil_to_epoch_day("2026-10-05"), Some(20731));
        assert_eq!(civil_to_epoch_day("2027-01-06"), Some(20824));
    }

    #[test]
    fn civil_to_epoch_day_rejects_invalid_dates_and_formats() {
        for s in [
            "2026-13-01", // 月が範囲外
            "2026-02-30", // 2月に存在しない日
            "2025-02-29", // 平年の2/29
            "2026-9-1",   // ゼロパディング無し
            "2026/09/01", // 区切りが違う
            "not-a-date",
        ] {
            assert!(civil_to_epoch_day(s).is_none(), "{s}");
        }
        assert!(civil_to_epoch_day("2024-02-29").is_some()); // 閏年は通る
    }

    #[test]
    fn days_until_reports_signed_difference_and_window() {
        assert_eq!(days_until("2026-10-01", "2026-09-28"), Some(3));
        assert_eq!(days_until("2026-09-28", "2026-09-28"), Some(0));
        assert_eq!(days_until("2026-09-24", "2026-09-28"), Some(-4));
        // 8日先はウィンドウ外。
        assert_eq!(days_until("2026-10-06", "2026-09-28"), None);
        // 超過側には上限が無い。
        assert_eq!(days_until("2020-01-01", "2026-09-28"), Some(-2462));
    }

    #[test]
    fn format_row_shows_upcoming_today_and_overdue() {
        let row = Row {
            slug: "2026-09b".into(),
            id: Some("report".into()),
            due: "2026-10-01".into(),
        };
        assert_eq!(
            format_row("travel", &row, 3),
            "travel/2026-09b report — 2026-10-01(あと3日)"
        );
        assert_eq!(
            format_row("travel", &row, 0),
            "travel/2026-09b report — 2026-10-01(本日)"
        );
        assert_eq!(
            format_row("travel", &row, -4),
            "travel/2026-09b report — 2026-10-01(超過4日)"
        );
    }

    #[test]
    fn format_row_omits_id_when_absent() {
        let row = Row {
            slug: "2026-09a".into(),
            id: None,
            due: "2026-10-01".into(),
        };
        assert_eq!(
            format_row("contacts", &row, 1),
            "contacts/2026-09a — 2026-10-01(あと1日)"
        );
    }

    #[test]
    fn build_message_returns_none_for_empty() {
        assert_eq!(build_message(&[]), None);
    }

    #[test]
    fn build_message_joins_short_list_untruncated() {
        let lines = vec!["a".to_string(), "b".to_string()];
        assert_eq!(build_message(&lines), Some("a、b".to_string()));
    }

    #[test]
    fn build_message_truncates_long_list_with_count_suffix() {
        let long_line = "x".repeat(100);
        let lines: Vec<String> = (0..5).map(|_| long_line.clone()).collect();
        let msg = build_message(&lines).unwrap();
        assert!(msg.chars().count() <= 230, "len={}", msg.chars().count());
        assert!(msg.ends_with("件)") || msg.contains("…ほか"), "{msg}");
    }

    #[test]
    fn find_shown_reason_locates_nested_shown() {
        let v: Value =
            serde_json::from_str(r#"{"id":"x","result":{"shown":true,"reason":null}}"#).unwrap();
        assert_eq!(find_shown_reason(&v), (Some(true), None));
    }

    #[test]
    fn find_shown_reason_reports_disabled_reason() {
        let v: Value =
            serde_json::from_str(r#"{"result":{"shown":false,"reason":"disabled"}}"#).unwrap();
        assert_eq!(
            find_shown_reason(&v),
            (Some(false), Some("disabled".into()))
        );
    }

    #[test]
    fn find_shown_reason_absent_when_no_shown_key() {
        let v: Value = serde_json::from_str(r#"{"error":{"message":"x"}}"#).unwrap();
        assert_eq!(find_shown_reason(&v), (None, None));
    }

    #[test]
    fn exit_code_reflects_delivery_only() {
        assert_eq!(exit_code(Outcome::NothingDue), 0);
        assert_eq!(exit_code(Outcome::AlreadyShownToday), 0);
        assert_eq!(exit_code(Outcome::Shown), 0);
        assert_eq!(exit_code(Outcome::NotShownRetry), 0);
        assert_eq!(exit_code(Outcome::Disabled), 1);
    }
}
