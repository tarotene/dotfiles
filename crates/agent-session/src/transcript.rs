//! transcript(JSONL)の読み取り。
//!
//! plan-scope-gate が持っていた `extract_user_text` を、wrapup-stop-gate の
//! レトロ(ユーザー発言の一覧提示)と共有するために引き上げた
//! (ADR-0035 D1「単一正本 > 複写+同期」)。挙動は移設前と同一。

use serde_json::Value;

/// transcript(JSONL)から、ユーザー自身が打った行だけを取り出す。
///
/// type=="user" かつ message.content が文字列、だけでは足りない(#290) —
/// background agent の完了通知(task-notification)も同じ形で記録される。
/// task-notification 行は origin.kind=="task-notification" かつ
/// promptSource=="system"。両フィールド不在の旧形式行は従来どおり通す。
///
/// jq と同じく、壊れた JSON に当たった時点で以降を読まない。
pub fn extract_user_text(transcript: &str) -> String {
    let mut out = String::new();
    for v in serde_json::Deserializer::from_str(transcript).into_iter::<Value>() {
        let Ok(v) = v else { break };
        let Some(obj) = v.as_object() else { continue };
        if obj.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(content) = obj
            .get("message")
            .and_then(|m| m.as_object())
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if alt_str(obj.get("promptSource")) == Some("system") {
            continue;
        }
        let kind = obj
            .get("origin")
            .and_then(|o| o.as_object())
            .and_then(|o| o.get("kind"));
        if alt_str(kind) == Some("task-notification") {
            continue;
        }
        out.push_str(content);
        out.push('\n');
    }
    out
}

/// jq の `x // ""` を文字列比較に使うための近似: null/false/欠落は ""。
fn alt_str(v: Option<&Value>) -> Option<&str> {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Some(""),
        Some(Value::String(s)) => Some(s),
        Some(_) => None,
    }
}

/// 未完了のバックグラウンド処理の ID(#854)。Stop hook が「作業の途中の待ちターン」を
/// 見分けるのに使う。hook の入力にバックグラウンドの状態は無いので、transcript から導く。
///
/// - 開始: `toolUseResult.backgroundTaskId`(背景 Bash)、または `toolUseResult.agentId` で
///   `status == "async_launched"`(非同期の Agent)。
/// - 完了: task-notification の `<task-id>ID</task-id>`。新しい形は `attachment.type ==
///   "queued_command"` の `prompt`、古い形は `type: "user"` で `origin.kind ==
///   "task-notification"` の本文。通知は処理が止まるたびに届くので、どの `status` でも終了
///   として数える。
/// - 古い開始(`stale_secs` を超えたもの)は数えない。終わらない監視(`tail -F` など)が
///   ゲートを際限なく黙らせないため。開始時刻が読めないものは新しいものとして数える。
///
/// 壊れた行は読み飛ばす(`extract_user_text` と違い、以降も読む。末尾の書きかけの行で
/// 全体が読めなくなるのを避ける)。
pub fn pending_background_tasks(transcript: &str, now_unix: u64, stale_secs: u64) -> Vec<String> {
    let mut started: Vec<(String, Option<u64>)> = Vec::new();
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in transcript.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_iso8601_utc);
        if let Some(r) = v.get("toolUseResult").and_then(Value::as_object) {
            if let Some(id) = r.get("backgroundTaskId").and_then(Value::as_str) {
                started.push((id.to_string(), ts));
            } else if r.get("status").and_then(Value::as_str) == Some("async_launched") {
                if let Some(id) = r.get("agentId").and_then(Value::as_str) {
                    started.push((id.to_string(), ts));
                }
            }
        }
        let notification = notification_text(&v);
        if let Some(text) = notification {
            let mut rest = text;
            while let Some(i) = rest.find("<task-id>") {
                rest = &rest[i + "<task-id>".len()..];
                let Some(j) = rest.find("</task-id>") else {
                    break;
                };
                done.insert(rest[..j].to_string());
                rest = &rest[j..];
            }
        }
    }
    started
        .into_iter()
        .filter(|(id, _)| !done.contains(id))
        .filter(|(_, ts)| ts.is_none_or(|t| now_unix.saturating_sub(t) <= stale_secs))
        .map(|(id, _)| id)
        .collect()
}

/// task-notification の本文(新旧 2 つの記録形式)。
fn notification_text(v: &Value) -> Option<&str> {
    if let Some(p) = v
        .get("attachment")
        .filter(|a| a.get("type").and_then(Value::as_str) == Some("queued_command"))
        .and_then(|a| a.get("prompt"))
        .and_then(Value::as_str)
    {
        return p.contains("<task-notification>").then_some(p);
    }
    if v.get("origin")
        .and_then(|o| o.get("kind"))
        .and_then(Value::as_str)
        == Some("task-notification")
    {
        return v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str);
    }
    None
}

/// `YYYY-MM-DDTHH:MM:SS(.fff)?Z` を UNIX 秒にする(UTC 以外・形式違いは `None`)。
fn parse_iso8601_utc(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // days_from_civil(Howard Hinnant)。
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_filters_injections_and_notifications() {
        let t = concat!(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"issue-index: #999 何か"}]}}"#,
            "\n",
            r#"{"type":"user","promptSource":"system","origin":{"kind":"task-notification"},"message":{"content":"--check plan.md #998"}}"#,
            "\n",
            r#"{"type":"user","promptSource":"typed","origin":{"kind":"human"},"message":{"content":"依頼: #136 をお願い"}}"#,
            "\n",
            r#"{"type":"user","message":{"content":"旧形式 #135"}}"#,
            "\n",
            "{broken\n",
            r#"{"type":"user","message":{"content":"壊れた行より後 #134"}}"#,
            "\n",
        );
        let x = extract_user_text(t);
        assert!(!x.contains("#999"));
        assert!(!x.contains("#998"));
        assert!(x.contains("#136"));
        assert!(x.contains("#135"));
        assert!(!x.contains("#134"));
    }

    // --- pending_background_tasks(#854) ---
    // 実 transcript(Claude Code)の 2 種類の形を使う:
    //  - 開始: `toolUseResult.backgroundTaskId`(背景 Bash)、
    //          `toolUseResult.agentId` + `status: "async_launched"`(非同期 Agent)
    //  - 完了: `attachment.type == "queued_command"` の prompt(新)、または
    //          `type: "user"` + `origin.kind == "task-notification"`(旧)の
    //          `<task-notification><task-id>ID</task-id>...`

    const T0: u64 = 1_792_000_000;

    fn iso(secs: u64) -> String {
        // テスト用の逆変換(UTC)。
        let days = (secs / 86_400) as i64;
        let rem = secs % 86_400;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.123Z",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        )
    }

    fn bash_started(id: &str, at: u64) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{}","toolUseResult":{{"backgroundTaskId":"{id}","stdout":"","stderr":""}},"message":{{"content":[{{"type":"tool_result","content":"Command running in background with ID: {id}."}}]}}}}"#,
            iso(at)
        ) + "\n"
    }

    fn agent_started(id: &str, at: u64) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{}","toolUseResult":{{"agentId":"{id}","status":"async_launched","isAsync":true}}}}"#,
            iso(at)
        ) + "\n"
    }

    fn done_new(id: &str, at: u64) -> String {
        format!(
            r#"{{"type":"attachment","timestamp":"{}","attachment":{{"type":"queued_command","prompt":"<task-notification>\n<task-id>{id}</task-id>\n<status>completed</status>\n</task-notification>"}}}}"#,
            iso(at)
        ) + "\n"
    }

    fn done_old(id: &str, at: u64) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{}","origin":{{"kind":"task-notification"}},"message":{{"content":"<task-notification>\n<task-id>{id}</task-id>\n<status>stopped</status>\n</task-notification>"}}}}"#,
            iso(at)
        ) + "\n"
    }

    #[test]
    fn pending_counts_started_but_not_finished() {
        let t = bash_started("b1", T0) + &agent_started("a1", T0 + 5) + &done_new("a1", T0 + 60);
        assert_eq!(pending_background_tasks(&t, T0 + 120, 3600), vec!["b1"]);
    }

    #[test]
    fn pending_is_empty_when_everything_finished_in_either_format() {
        let t = bash_started("b1", T0)
            + &agent_started("a1", T0 + 5)
            + &done_old("b1", T0 + 30)
            + &done_new("a1", T0 + 60);
        assert!(pending_background_tasks(&t, T0 + 120, 3600).is_empty());
    }

    #[test]
    fn pending_ignores_other_launches_and_text_that_only_mentions_ids() {
        // sync の Agent(status が async_launched でない)と、本文が id に言及するだけの行は数えない。
        let t = concat!(
            r#"{"type":"user","toolUseResult":{"agentId":"s1","status":"completed"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"backgroundTaskId は b9 です"}]}}"#,
            "\n",
        );
        assert!(pending_background_tasks(t, T0, 3600).is_empty());
    }

    #[test]
    fn pending_drops_tasks_older_than_the_stale_limit() {
        // 終わらない監視(`tail -F` など)で、ゲートを際限なく黙らせない。
        let t = bash_started("old", T0) + &bash_started("fresh", T0 + 3000);
        assert_eq!(pending_background_tasks(&t, T0 + 3700, 3600), vec!["fresh"]);
    }

    #[test]
    fn pending_tolerates_broken_lines_and_missing_timestamps() {
        let t = "{broken\n".to_string()
            + r#"{"type":"user","toolUseResult":{"backgroundTaskId":"nt"}}"#
            + "\n";
        assert_eq!(
            pending_background_tasks(&t, T0, 3600),
            vec!["nt"],
            "時刻が読めない開始は新しいものとして数える。壊れた行は読み飛ばす"
        );
    }

    #[test]
    fn iso8601_round_trips_with_the_test_helper() {
        for t in [0u64, 86_399, 1_792_000_000, 1_792_551_147, 4_102_444_800] {
            let text = iso(t);
            assert_eq!(parse_iso8601_utc(&text), Some(t), "{text}");
        }
        assert_eq!(parse_iso8601_utc("2026-10-21T10:29:00+09:00"), None);
        assert_eq!(parse_iso8601_utc("garbage"), None);
    }
}
