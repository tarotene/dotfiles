//! daily-desk-reminder の純関数部分(SessionStart hook、docs/agents/my-desk.md)。
//!
//! その日最初のセッションで「今日の朝の机は未実施」を `additionalContext` で注入する。
//! 実施済みの印は `~/.local/state/claude/desk/<JST の日付>` の空ファイル 1 つだけで、
//! 机の完了を示す台帳ではない(`my-desk` スキルが完了時に作る)。ハブ(手順の正本を持つ
//! private リポジトリ)が未設定の機体では発火しない。I/O は main.rs だけが行う。

use std::ffi::OsString;
use std::path::PathBuf;

/// 印を置くディレクトリ。`DESK_STATE_DIR` > `$XDG_STATE_HOME/claude/desk` >
/// `$HOME/.local/state/claude/desk`。
pub fn state_dir(env_override: Option<OsString>, state_base: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = env_override.filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(p));
    }
    state_base.map(|b| b.join("claude/desk"))
}

/// unix 秒を JST(UTC+9)の `YYYY-MM-DD` にする。暦の計算は `civil-time` が持つ。
pub use civil_time::jst_date;

/// `YYYY-MM-DD` の形(数字 4-2-2)か。印のファイル名だけを数える。
pub fn is_date_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// 印のファイル名のうち、最新の日付。
pub fn latest_date<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    names
        .into_iter()
        .filter(|n| is_date_name(n))
        .max()
        .map(str::to_string)
}

/// 注入文。本人が望むときにだけ `my-desk` を実行させる(自動では走らせない)。
pub fn build_context(today: &str, last: Option<&str>) -> String {
    let last = last.unwrap_or("なし");
    format!(
        "今日(JST {today})の朝の机は、まだ実施されていません(前回の机: {last})。\n\
         机は、その日にさばく作業(返信が要るメール、日程回答、人手タスク、未消費 PR の裁定、受信箱)を\n\
         1 か所から順に片付ける入口で、`my-desk` スキルが実行します。\n\
         本人に一言で提案し、本人が望むときにだけ実行してください。このセッションの依頼を先に進めてかまいません。\n\
         最後の段まで終えると、実施済みの印が作られ、この注入は止まります。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_dir_precedence() {
        let s = |x: &str| Some(OsString::from(x));
        let b = || Some(PathBuf::from("/x"));
        assert_eq!(state_dir(s("/o"), b()), Some("/o".into()));
        assert_eq!(state_dir(None, b()), Some("/x/claude/desk".into()));
        assert_eq!(state_dir(s(""), b()), Some("/x/claude/desk".into()));
        assert_eq!(state_dir(None, None), None);
    }

    #[test]
    fn jst_date_crosses_the_utc_boundary() {
        assert_eq!(jst_date(0), "1970-01-01");
        // 2026-10-22T15:00:00Z は JST では 2026-10-23 の 0 時。
        assert_eq!(jst_date(1_792_681_200), "2026-10-23");
        // その 1 秒前は、まだ JST でも 2026-10-22。
        assert_eq!(jst_date(1_792_681_199), "2026-10-22");
        // うるう日。2028-02-29T00:00:00Z。
        assert_eq!(jst_date(1_835_395_200), "2028-02-29");
    }

    #[test]
    fn date_names_only() {
        assert!(is_date_name("2026-10-23"));
        assert!(!is_date_name("2026-10-23.tmp"));
        assert!(!is_date_name("2026-1-23"));
        assert!(!is_date_name("abcd-ef-gh"));
        assert_eq!(
            latest_date(["2026-10-21", "x", "2026-10-22", "2026-10-22.bak"]),
            Some("2026-10-22".into())
        );
        assert_eq!(latest_date(["x"]), None);
    }

    #[test]
    fn context_names_the_skill_and_the_dates() {
        let c = build_context("2026-10-23", Some("2026-10-22"));
        assert!(c.contains("2026-10-23"));
        assert!(c.contains("前回の机: 2026-10-22"));
        assert!(c.contains("`my-desk`"));
        assert!(build_context("2026-10-23", None).contains("前回の机: なし"));
    }
}
