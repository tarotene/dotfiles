//! 下書きの作成口の関所(PreToolUse)と、会話の全文の保存(PostToolUse)の判定。
//!
//! hook-router(aggregator)が規則 `reply-coverage-gate`・`reply-thread-snapshot` から呼ぶ。
//! ツール名と入力からの本文の取り出し(`ja_polish::gate::mcp_text`)は ja-polish が持つ正本を使うので、
//! この crate は ja-polish に依存しない。呼び出し側が本文を渡す。
//!
//! 対象は Gmail の MCP だけ(段2)。Slack・ブラウザの媒体・GitHub は搬送路が違い、docs/agents/reply-coverage.md の表のとおり。
//! 判定不能(状態の置き場が無い、入力の形が違う)は、拒否に変えず通す(fail-open)。止めるのは、
//! 「返信だと分かり、会話を読んでいない / 検査していない」と確かめられたときだけ。

use crate::store::{self, Store, StoredThread};
use crate::verify;
use serde_json::Value;

/// Gmail の MCP ツールか。
pub fn is_gmail(tool: &str) -> bool {
    tool.contains("Gmail")
}

fn suffix_is(tool: &str, names: &[&str]) -> bool {
    names.iter().any(|n| tool.ends_with(&format!("__{n}")))
}

/// Gmail の `get_thread` か。
pub fn is_get_thread(tool: &str) -> bool {
    is_gmail(tool) && suffix_is(tool, &["get_thread"])
}

/// 返信の本文を作る(または送る)呼び出しか。`create_draft`・`update_draft` は `replyToMessageId` を持つときだけ返信、
/// `reply` は常に返信。`send_message`・`forward` は対象にしない(新規・転送。返信は `reply` か下書きを経る)。
pub fn reply_target(tool: &str, input: &Value) -> Option<String> {
    if !is_gmail(tool) {
        return None;
    }
    if suffix_is(tool, &["create_draft", "update_draft", "reply"]) {
        return input
            .get("replyToMessageId")
            .or_else(|| input.get("messageId"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string);
    }
    None
}

/// PreToolUse の判定。`text` は、呼び出しの本文(`ja_polish::gate::mcp_text` の結果)。拒否する理由を返す。
pub fn decide(store: &Store, tool: &str, input: &Value, text: &str, now: u64) -> Option<String> {
    let target = reply_target(tool, input)?;
    let thread = store.thread_for_message(&target);
    match verify::verify(store, thread.as_ref(), text, now) {
        Ok(()) => None,
        Err(f) => {
            let id = thread
                .as_ref()
                .map(|t| t.id.clone())
                .unwrap_or_else(|| format!("(返信先 {target})"));
            Some(f.message(&id))
        }
    }
}

/// PostToolUse の記録役: Gmail の `get_thread` の応答から、会話の全文を保存する。保存した会話の id を返す。
/// 応答の形が読めなければ何もしない(`None`)。
pub fn snapshot_from_get_thread(
    store: &Store,
    input: &Value,
    response: &Value,
    now: u64,
) -> Option<String> {
    let thread_id = input.get("threadId").and_then(Value::as_str)?;
    let value = unwrap_response(response)?;
    let messages = value.get("messages")?.as_array()?;
    if messages.is_empty() {
        return None;
    }
    let mut text = format!("# thread: gmail:{thread_id}\n");
    let mut ids = Vec::new();
    for m in messages {
        if let Some(id) = m.get("id").and_then(Value::as_str) {
            ids.push(id.to_string());
        }
        let sender = m.get("sender").and_then(Value::as_str).unwrap_or("");
        let date = m.get("date").and_then(Value::as_str).unwrap_or("");
        let role = role_of(m);
        let body = m
            .get("plaintextBody")
            .and_then(Value::as_str)
            .or_else(|| m.get("snippet").and_then(Value::as_str))
            .unwrap_or("");
        text.push_str(&format!("## {role} {date} {sender}\n"));
        for line in body.lines() {
            // 本文の中の行頭の `## ` が、メッセージの見出しと混ざらないようにする。
            if line.starts_with("## ") || line.starts_with("# thread:") {
                text.push('\\');
            }
            text.push_str(line);
            text.push('\n');
        }
    }
    let t = StoredThread {
        id: format!("gmail:{thread_id}"),
        fetched_ts: now,
        format: "normalized".to_string(),
        message_ids: ids,
        text,
    };
    store.save_thread(&t).ok()?;
    Some(t.id)
}

/// MCP の応答は、`{"content":[{"type":"text","text":"<JSON>"}]}` の形か、JSON そのもの。どちらからも
/// 会話のオブジェクト(`messages` を持つもの)を取り出す。
fn unwrap_response(response: &Value) -> Option<Value> {
    let with_messages = |v: Value| v.get("messages").is_some().then_some(v);
    if let Some(s) = response.as_str() {
        return serde_json::from_str::<Value>(s)
            .ok()
            .and_then(with_messages);
    }
    if let Some(v) = with_messages(response.clone()) {
        return Some(v);
    }
    if let Some(v) = response
        .get("structuredContent")
        .cloned()
        .and_then(with_messages)
    {
        return Some(v);
    }
    let parts = response
        .get("content")
        .and_then(Value::as_array)
        .or_else(|| response.as_array())?;
    parts.iter().find_map(|p| {
        let s = p
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| p.as_str())?;
        serde_json::from_str::<Value>(s)
            .ok()
            .and_then(with_messages)
    })
}

/// 本人のメッセージか(ラベルに `SENT` を持つ)。分からなければ `them`(本人の発言を相手の依頼と誤読する側に
/// 倒れても、検出は「本人が先に答えた」を会話から読める)。
fn role_of(m: &Value) -> &'static str {
    let sent = m
        .get("labelIds")
        .and_then(Value::as_array)
        .is_some_and(|l| l.iter().any(|x| x.as_str() == Some("SENT")));
    if sent {
        "me"
    } else {
        "them"
    }
}

/// 現在時刻つきの入口(hook から呼ぶ)。
pub fn now() -> u64 {
    store::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Record, StoredThread};
    use crate::text;
    use serde_json::json;

    const CREATE: &str = "mcp__claude_ai_Gmail__create_draft";
    const GET_THREAD: &str = "mcp__claude_ai_Gmail__get_thread";

    fn response() -> Value {
        json!({"messages":[
            {"id":"m1","sender":"recruiter@example.invalid","date":"2026-11-01T10:00:00Z","labelIds":["INBOX"],
             "plaintextBody":"来週の火曜日 10 時にお電話できますか。\n## 見出しのような行"},
            {"id":"m2","sender":"me@example.invalid","date":"2026-11-01T12:00:00Z","labelIds":["SENT"],
             "plaintextBody":"火曜日で大丈夫です。"}
        ]})
    }

    #[test]
    fn get_thread_is_saved_as_a_normalized_conversation_with_roles_and_an_id_index() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let id =
            snapshot_from_get_thread(&s, &json!({"threadId":"t1"}), &response(), 1000).unwrap();
        assert_eq!(id, "gmail:t1");
        let t = s.load_thread("gmail:t1").unwrap();
        assert_eq!(t.message_ids, ["m1", "m2"]);
        assert!(t
            .text
            .starts_with("# thread: gmail:t1\n## them 2026-11-01T10:00:00Z"));
        assert!(t.text.contains("## me 2026-11-01T12:00:00Z"));
        assert!(t.text.contains("来週の火曜日 10 時にお電話できますか。"));
        // 本文の中の `## ` は、見出しと混ざらないよう退避される。
        assert!(t.text.contains("\\## 見出しのような行"));
        assert_eq!(s.thread_for_message("m2").unwrap().id, "gmail:t1");
    }

    #[test]
    fn the_mcp_envelope_forms_are_unwrapped() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let body = response().to_string();
        let wrapped = json!({"content":[{"type":"text","text": body}]});
        assert!(snapshot_from_get_thread(&s, &json!({"threadId":"t2"}), &wrapped, 1).is_some());
        let structured = json!({"structuredContent": response()});
        assert!(snapshot_from_get_thread(&s, &json!({"threadId":"t3"}), &structured, 1).is_some());
        let as_string = Value::String(response().to_string());
        assert!(snapshot_from_get_thread(&s, &json!({"threadId":"t4"}), &as_string, 1).is_some());
    }

    #[test]
    fn unreadable_responses_save_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        assert!(
            snapshot_from_get_thread(&s, &json!({"threadId":"t"}), &json!({"error":"x"}), 1)
                .is_none()
        );
        assert!(
            snapshot_from_get_thread(&s, &json!({"threadId":"t"}), &json!({"messages":[]}), 1)
                .is_none()
        );
        assert!(snapshot_from_get_thread(&s, &json!({}), &response(), 1).is_none());
        assert!(s.load_thread("gmail:t").is_none());
    }

    #[test]
    fn only_replies_are_targets() {
        let reply = json!({"replyToMessageId":"m1","body":"x"});
        assert_eq!(reply_target(CREATE, &reply).as_deref(), Some("m1"));
        assert_eq!(
            reply_target("mcp__claude_ai_Gmail__update_draft", &reply).as_deref(),
            Some("m1")
        );
        // 返信先の無い下書き(新規)・送信・転送・Gmail 以外は対象外。
        assert_eq!(reply_target(CREATE, &json!({"body":"x"})), None);
        assert_eq!(
            reply_target("mcp__claude_ai_Gmail__send_message", &reply),
            None
        );
        assert_eq!(reply_target("mcp__claude_ai_Gmail__forward", &reply), None);
        assert_eq!(reply_target("mcp__other__create_draft", &reply), None);
        assert!(is_get_thread(GET_THREAD) && !is_get_thread("mcp__claude_ai_Gmail__get_message"));
    }

    fn setup(now: u64) -> (tempfile::TempDir, Store, StoredThread) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        snapshot_from_get_thread(&s, &json!({"threadId":"t1"}), &response(), now).unwrap();
        let t = s.load_thread("gmail:t1").unwrap();
        (dir, s, t)
    }

    fn pass_record(s: &Store, t: &StoredThread, draft: &str, ts: u64) {
        s.append_record(&Record {
            ts_unix: ts,
            thread: t.id.clone(),
            thread_sha: text::sha(&t.text),
            draft_sha: text::sha(draft),
            verdict: "pass".to_string(),
            model: "m".to_string(),
            asks: 1,
            unanswered: vec![],
            apology_missing: false,
        })
        .unwrap();
    }

    #[test]
    fn a_reply_to_an_unread_thread_is_denied_and_told_to_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let input = json!({"replyToMessageId":"never-seen","body":"了解です。"});
        let reason = decide(&s, CREATE, &input, "了解です。", 1000).unwrap();
        assert!(
            reason.contains("get_thread") && reason.contains("never-seen"),
            "{reason}"
        );
    }

    #[test]
    fn a_reply_that_was_not_checked_is_denied_and_a_checked_one_passes() {
        let (_d, s, t) = setup(1000);
        let draft = "水曜日 15 時でお願いします。";
        let input = json!({"replyToMessageId":"m1","body":draft});
        let reason = decide(&s, CREATE, &input, draft, 1001).unwrap();
        assert!(reason.contains("reply-coverage check"), "{reason}");
        pass_record(&s, &t, draft, 1002);
        assert_eq!(decide(&s, CREATE, &input, draft, 1003), None);
        // 検査のあとに 1 字直した本文は通らない。
        let edited = json!({"replyToMessageId":"m1","body":"水曜日 15 時でお願いします!"});
        assert!(decide(&s, CREATE, &edited, "水曜日 15 時でお願いします!", 1003).is_some());
    }

    #[test]
    fn a_failed_check_names_the_unanswered_asks() {
        let (_d, s, t) = setup(1000);
        let draft = "前向きに検討します。";
        s.append_record(&Record {
            ts_unix: 1001,
            thread: t.id.clone(),
            thread_sha: text::sha(&t.text),
            draft_sha: text::sha(draft),
            verdict: "fail".to_string(),
            model: "m".to_string(),
            asks: 1,
            unanswered: vec!["来週の火曜日 10 時にお電話できますか。".to_string()],
            apology_missing: false,
        })
        .unwrap();
        let input = json!({"replyToMessageId":"m2","body":draft});
        let reason = decide(&s, CREATE, &input, draft, 1002).unwrap();
        assert!(reason.contains("来週の火曜日 10 時"), "{reason}");
    }

    #[test]
    fn a_stale_snapshot_is_denied_even_with_a_pass_record() {
        let (_d, s, t) = setup(1000);
        let draft = "x";
        pass_record(&s, &t, draft, 1001);
        let input = json!({"replyToMessageId":"m1","body":draft});
        let later = 1000 + verify::MAX_THREAD_AGE_SECS + 1;
        let reason = decide(&s, CREATE, &input, draft, later).unwrap();
        assert!(reason.contains("取り直"), "{reason}");
    }

    #[test]
    fn non_replies_and_other_tools_are_left_alone() {
        let (_d, s, _t) = setup(1000);
        assert_eq!(decide(&s, CREATE, &json!({"body":"新規"}), "新規", 1), None);
        assert_eq!(
            decide(&s, "mcp__x__y", &json!({"replyToMessageId":"m1"}), "x", 1),
            None
        );
    }
}
