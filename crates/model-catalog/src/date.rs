//! UTC の日付(YYYY-MM-DD)。標準ライブラリだけで、1970-01-01 からの日数を暦に直す
//! (Howard Hinnant の civil_from_days)。取得日の記録と「N 日前」の検索範囲に使う。

fn civil_from_unix_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// 1970-01-01 からの日数を `YYYY-MM-DD` にする。
pub fn format_unix_days(days: i64) -> String {
    let (y, m, d) = civil_from_unix_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn unix_days_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs / 86_400) as i64
}

pub fn today_utc() -> String {
    format_unix_days(unix_days_now())
}

pub fn days_ago_utc(n: u32) -> String {
    format_unix_days(unix_days_now() - i64::from(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates() {
        assert_eq!(format_unix_days(0), "1970-01-01");
        assert_eq!(format_unix_days(59), "1970-03-01");
        assert_eq!(format_unix_days(11_016), "2000-02-29");
        assert_eq!(format_unix_days(20_753), "2026-10-27");
        assert_eq!(format_unix_days(20_753 - 365), "2025-10-27");
    }

    #[test]
    fn today_has_date_shape_and_days_ago_is_earlier() {
        let t = today_utc();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
        assert!(days_ago_utc(30) < t);
        assert_eq!(days_ago_utc(0), t);
    }
}
