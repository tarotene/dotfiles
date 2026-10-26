//! daily-desk-reminder の純関数部分(SessionStart hook、docs/agents/daily-desk.md)。
//!
//! その日最初のセッションで「今日の朝の机は未実施」を `additionalContext` で注入する。
//! 実施済みの印は `~/.local/state/claude/desk/<JST の日付>` の空ファイル 1 つだけで、
//! 机の完了を示す台帳ではない(`daily-desk` スキルが完了時に作る)。ハブ(手順の正本を持つ
//! private リポジトリ)が未設定の機体では発火しない。I/O は main.rs だけが行う。

use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;

/// 印を置くディレクトリ。`DESK_STATE_DIR` > `$XDG_STATE_HOME/claude/desk` >
/// `$HOME/.local/state/claude/desk`。
pub fn state_dir(
    env_override: Option<OsString>,
    xdg_state: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let non_empty = |v: Option<OsString>| v.filter(|s| !s.is_empty()).map(PathBuf::from);
    if let Some(p) = non_empty(env_override) {
        return Some(p);
    }
    if let Some(x) = non_empty(xdg_state) {
        return Some(x.join("claude/desk"));
    }
    non_empty(home).map(|h| h.join(".local/state/claude/desk"))
}

/// unix 秒を JST(UTC+9)の `YYYY-MM-DD` にする。`date(1)` や `TZ` に依存しない。
pub fn jst_date(unix_secs: u64) -> String {
    let days = ((unix_secs + 9 * 3600) / 86_400) as i64;
    // Howard Hinnant の civil_from_days。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

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

/// 注入文。本人が望むときにだけ `daily-desk` を実行させる(自動では走らせない)。
pub fn build_context(today: &str, last: Option<&str>) -> String {
    let last = last.unwrap_or("なし");
    format!(
        "今日(JST {today})の朝の机は、まだ実施されていません(前回の机: {last})。\n\
         机は、その日にさばく作業(返信が要るメール、日程回答、人手タスク、未消費 PR の裁定、受信箱)を\n\
         1 か所から順に片付ける入口で、`daily-desk` スキルが実行します。\n\
         本人に一言で提案し、本人が望むときにだけ実行してください。このセッションの依頼を先に進めてかまいません。\n\
         最後の段まで終えると、実施済みの印が作られ、この注入は止まります。"
    )
}

#[derive(Serialize)]
struct Out<'a> {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: Inner<'a>,
}

#[derive(Serialize)]
struct Inner<'a> {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'a str,
    #[serde(rename = "additionalContext")]
    additional_context: &'a str,
}

pub fn session_start_json(ctx: &str) -> String {
    serde_json::to_string_pretty(&Out {
        hook_specific_output: Inner {
            hook_event_name: "SessionStart",
            additional_context: ctx,
        },
    })
    .expect("serialize session-start output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_dir_precedence() {
        let s = |x: &str| Some(OsString::from(x));
        assert_eq!(state_dir(s("/o"), s("/x"), s("/h")), Some("/o".into()));
        assert_eq!(
            state_dir(None, s("/x"), s("/h")),
            Some("/x/claude/desk".into())
        );
        assert_eq!(
            state_dir(s(""), s(""), s("/h")),
            Some("/h/.local/state/claude/desk".into())
        );
        assert_eq!(state_dir(None, None, None), None);
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
        assert!(c.contains("`daily-desk`"));
        assert!(build_context("2026-10-23", None).contains("前回の机: なし"));
    }

    #[test]
    fn json_roundtrip() {
        let j = session_start_json("x");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "x");
    }
}
