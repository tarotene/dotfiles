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
}
