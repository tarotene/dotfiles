//! UTC の日付(YYYY-MM-DD)。暦の計算は `civil-time` が持つ(標準ライブラリだけの crate)。
//! 取得日の記録と「N 日前」の検索範囲に使う。

pub use civil_time::{days_ago_utc, today_utc};

/// 1970-01-01 からの日数を `YYYY-MM-DD` にする。
pub use civil_time::format_date as format_unix_days;
