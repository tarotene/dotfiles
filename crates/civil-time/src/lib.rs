//! civil-time — unix 秒と暦(UTC と固定の +09:00=JST)の相互変換。依存ゼロ。
//!
//! Howard Hinnant の `days_from_civil` / `civil_from_days` が、8 つの crate に手で複写されていた
//! (月日の型が `i64` と `u32` で違う複写も混ざっていた)。ここに 1 つにまとめる。`date(1)` や `TZ` に依存しない。
//! 時刻帯のデータベースは持たない(必要なのは UTC と、固定の JST だけ)。

/// 日本標準時の UTC からの差(秒)。
pub const JST_OFFSET_SECS: u64 = 9 * 3600;

/// 暦の日付 → 1970-01-01 からの日数(`civil_from_days` の逆)。`m` は 1〜12、`d` は 1〜31 を渡す
/// (範囲外は、`civil_from_days` へ戻して一致を見る `parse_day` が弾く)。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 からの日数 → 暦の日付 `(年, 月, 日)`。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 日数 → `YYYY-MM-DD`。
pub fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
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

/// 現在の unix 秒(時計が 1970 より前なら 0)。
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// unix 秒の UTC の日付 `YYYY-MM-DD`。
pub fn utc_date(unix_secs: u64) -> String {
    format_date((unix_secs / 86_400) as i64)
}

/// 今日の UTC の日付。
pub fn today_utc() -> String {
    utc_date(now_unix())
}

/// `n` 日前の UTC の日付。
pub fn days_ago_utc(n: u32) -> String {
    format_date((now_unix() / 86_400) as i64 - i64::from(n))
}

/// unix 秒の UTC の日時 `YYYY-MM-DDTHH:MM:SSZ`(`date -u +%Y-%m-%dT%H:%M:%SZ`)。
pub fn utc_timestamp(unix_secs: u64) -> String {
    let rem = unix_secs % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        utc_date(unix_secs),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// unix 秒の JST の日付 `YYYY-MM-DD`。
pub fn jst_date(unix_secs: u64) -> String {
    utc_date(unix_secs + JST_OFFSET_SECS)
}

/// 今日の JST の日付。
pub fn jst_today() -> String {
    jst_date(now_unix())
}

/// unix 秒の JST の日時 `YYYY-MM-DDTHH:MM:SS+09:00`(ISO 8601)。
pub fn jst_datetime(unix_secs: u64) -> String {
    let secs = unix_secs + JST_OFFSET_SECS;
    let rem = secs % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}+09:00",
        utc_date(secs),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// `2026-10-02T01:02:03Z` → epoch 秒。形式外は `None`。
pub fn parse_iso_epoch(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, u32, u32) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let mut t = time.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    Some(days_from_civil(y, m, day) * 86_400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates_round_trip() {
        for (days, date) in [
            (0, "1970-01-01"),
            (59, "1970-03-01"),
            (11_016, "2000-02-29"),
            (20_753, "2026-10-27"),
            (20_753 - 365, "2025-10-27"),
            (-1, "1969-12-31"),
        ] {
            assert_eq!(format_date(days), date);
            assert_eq!(parse_day(date), Some(days), "{date}");
        }
    }

    #[test]
    fn a_date_that_does_not_exist_is_refused() {
        for s in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-1-01",
            "x",
        ] {
            assert_eq!(parse_day(s), None, "{s}");
        }
        // うるう年の 2 月 29 日は、実在する年だけ。
        assert!(parse_day("2028-02-29").is_some());
        assert_eq!(parse_day("2027-02-29"), None);
    }

    #[test]
    fn jst_crosses_the_utc_boundary() {
        assert_eq!(jst_date(0), "1970-01-01");
        // 2026-10-22T15:00:00Z は JST では 2026-10-23 の 0 時。
        assert_eq!(jst_date(1_792_681_200), "2026-10-23");
        // その 1 秒前は、まだ JST でも 2026-10-22。
        assert_eq!(jst_date(1_792_681_199), "2026-10-22");
        assert_eq!(utc_date(1_792_681_200), "2026-10-22");
    }

    #[test]
    fn timestamps_have_the_iso_8601_shapes() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(1_792_681_200), "2026-10-22T15:00:00Z");
        assert_eq!(jst_datetime(1_792_681_200), "2026-10-23T00:00:00+09:00");
        assert_eq!(jst_datetime(0), "1970-01-01T09:00:00+09:00");
    }

    #[test]
    fn iso_epoch_reads_the_z_form_only() {
        assert_eq!(parse_iso_epoch("2026-10-22T15:00:00Z"), Some(1_792_681_200));
        assert_eq!(parse_iso_epoch("2026-10-22T15:00:00+09:00"), None);
        assert_eq!(parse_iso_epoch("2026-10-22 15:00:00Z"), None);
    }

    #[test]
    fn today_has_the_date_shape_and_days_ago_is_earlier() {
        let t = today_utc();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
        assert!(days_ago_utc(30) < t);
        assert_eq!(days_ago_utc(0), t);
        assert_eq!(jst_today().len(), 10);
    }
}
