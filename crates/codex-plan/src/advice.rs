//! Read-only observations of native failures; never infer quota from message text.
//!
//! OpenAI, Codex rust-v0.164.0, retrieved 2026-10-26:
//! - `protocol/src/error.rs:491-499` maps capacity and quota separately.
//! - `app-server-protocol/src/protocol/v2/shared.rs:72-128` defines camelCase tags.
//! - `app-server-protocol/src/protocol/v2/notification.rs:62-69` defines willRetry.
//! - `app-server-protocol/src/protocol/v2/thread_data.rs:386-441` defines turn errors.
//!
//! Source base: <https://github.com/openai/codex/tree/rust-v0.164.0/codex-rs>.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Capacity,
    UsageExhausted,
    ContextExceeded,
    OtherUnknown,
}

impl StopReason {
    pub fn from_error_info(info: &Value) -> Self {
        match info.as_str() {
            Some("serverOverloaded") => Self::Capacity,
            Some("usageLimitExceeded") => Self::UsageExhausted,
            Some("contextWindowExceeded") => Self::ContextExceeded,
            // RateLimitExceeded can be a short-term request-rate limit, not
            // subscription exhaustion. HTTP 429 and free text prove neither.
            _ => Self::OtherUnknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedFailure {
    pub thread_id: String,
    pub turn_id: String,
    pub reason: StopReason,
}

/// An error remains relevant until its turn succeeds or a new turn starts on
/// that thread. Unrelated threads cannot clear each other's failures.
#[derive(Debug, Default)]
pub struct AdviceObserver {
    failures: BTreeMap<String, ObservedFailure>,
    active_turns: BTreeMap<String, String>,
}

impl AdviceObserver {
    /// Does not retain native prose, prompts, tool output or approval requests.
    pub fn observe(&mut self, message: &Value) {
        // Notifications have no request ID. Responses and server requests are
        // not terminal failures merely because they contain an error field.
        if message.get("id").is_some() {
            return;
        }
        let params = &message["params"];
        let Some(thread) = params["threadId"].as_str() else {
            return;
        };
        match message["method"].as_str() {
            Some("turn/started") => {
                if let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str) {
                    self.active_turns.insert(thread.into(), turn.into());
                    self.failures.remove(thread);
                }
            }
            Some("error") if params["willRetry"] == false => {
                if let Some(turn) = params["turnId"].as_str() {
                    self.record(thread, turn, &params["error"]["codexErrorInfo"]);
                }
            }
            Some("turn/completed") => {
                let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str) else {
                    return;
                };
                if self.is_stale(thread, turn) {
                    return;
                }
                match params.pointer("/turn/status").and_then(Value::as_str) {
                    Some("failed") => {
                        self.record(thread, turn, &params["turn"]["error"]["codexErrorInfo"]);
                    }
                    Some("completed")
                        if self.failures.get(thread).is_some_and(|f| f.turn_id == turn) =>
                    {
                        self.failures.remove(thread);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn is_stale(&self, thread: &str, turn: &str) -> bool {
        self.active_turns.get(thread).is_some_and(|id| id != turn)
    }

    fn record(&mut self, thread: &str, turn: &str, info: &Value) {
        if !self.is_stale(thread, turn) {
            // Some completion snapshots omit the error info already observed
            // on the same turn. Absence is not a new unknown classification.
            if info.is_null()
                && self
                    .failures
                    .get(thread)
                    .is_some_and(|failure| failure.turn_id == turn)
            {
                return;
            }
            self.failures.insert(
                thread.into(),
                ObservedFailure {
                    thread_id: thread.into(),
                    turn_id: turn.into(),
                    reason: StopReason::from_error_info(info),
                },
            );
        }
    }

    pub fn failures(&self) -> impl Iterator<Item = &ObservedFailure> {
        self.failures.values()
    }

    /// Call after native TUI exit and terminal restoration. The catalog is the
    /// live native model/list result, never the bundled public pricing list.
    pub fn render(&self, native_models: &[Value]) -> Option<String> {
        if self.failures.is_empty() {
            return None;
        }
        let mut output = String::from("\ncodex-plan: native の停止理由\n");
        for failure in self.failures.values() {
            let reason = match failure.reason {
                StopReason::Capacity => {
                    "capacity（供給混雑 / serverOverloaded）。利用枠枯渇とは別です。"
                }
                StopReason::UsageExhausted => {
                    "利用枠枯渇（usageLimitExceeded）。残枠・リセットは compare で確認してください。"
                }
                StopReason::ContextExceeded => {
                    "コンテキスト不足（contextWindowExceeded）。会話量の削減も検討してください。"
                }
                StopReason::OtherUnknown => {
                    "その他／不明。native の診断を確認してください（短期の rate limit もここに含みます）。"
                }
            };
            output.push_str(reason);
            output.push('\n');
            output.push_str("手動変更: codex-plan configure → 保存 → ");
            output.push_str("codex-plan resume ");
            output.push_str(&shell_quote(&failure.thread_id));
            output.push('\n');
        }
        let mut models: Vec<&str> = native_models
            .iter()
            .filter_map(|model| model["model"].as_str())
            .filter(|model| !model.is_empty() && model.chars().all(|c| !c.is_control()))
            .collect();
        models.sort_unstable();
        models.dedup();
        if models.is_empty() {
            output.push_str("代替候補は未確認（native カタログ未取得）。\n");
        } else {
            output.push_str("native カタログの選択候補: ");
            output.push_str(&models.join(", "));
            output.push('\n');
        }
        output.push_str("候補への掲載は推論成功や追加課金なしの継続を保証しません。中継による自動再送・自動切替は行いません。\n");
        Some(output)
    }
}

fn shell_quote(value: &str) -> String {
    // Session IDs are normally UUIDs, but do not trust future protocol values
    // as shell syntax or emit terminal control characters from an identifier.
    let printable: String = value.chars().filter(|c| !c.is_control()).collect();
    format!("'{}'", printable.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn error(info: Value, retry: bool) -> Value {
        json!({"method":"error","params":{"threadId":"thread-a","turnId":"turn-a",
            "willRetry":retry,"error":{"message":"native prose","codexErrorInfo":info}}})
    }

    #[test]
    fn capacity_with_remaining_quota_is_not_exhaustion() {
        let mut observer = AdviceObserver::default();
        observer.observe(&json!({"method":"account/rateLimits/updated","params":{
            "rateLimits":{"primary":{"usedPercent":11}}}}));
        observer.observe(&error(json!("serverOverloaded"), false));
        assert_eq!(
            observer.failures().next().unwrap().reason,
            StopReason::Capacity
        );
        let text = observer.render(&[json!({"model":"gpt-6.1-sol"})]).unwrap();
        assert!(text.contains("利用枠枯渇とは別"));
        assert!(text.contains("codex-plan resume 'thread-a'"));
        assert!(text.contains("gpt-6.1-sol"));
        assert!(!text.contains("native prose"));
    }

    #[test]
    fn structured_known_codes_and_unknown_are_distinct() {
        for (code, expected) in [
            (json!("usageLimitExceeded"), StopReason::UsageExhausted),
            (json!("contextWindowExceeded"), StopReason::ContextExceeded),
            (json!("rateLimitExceeded"), StopReason::OtherUnknown),
            (json!("futureError"), StopReason::OtherUnknown),
            (
                json!({"httpConnectionFailed":{"httpStatusCode":429}}),
                StopReason::OtherUnknown,
            ),
            (Value::Null, StopReason::OtherUnknown),
        ] {
            let mut observer = AdviceObserver::default();
            observer.observe(&error(code, false));
            assert_eq!(observer.failures().next().unwrap().reason, expected);
        }
    }

    #[test]
    fn retry_success_and_new_turn_do_not_leave_stale_stop_reason() {
        let mut observer = AdviceObserver::default();
        observer.observe(&error(json!("serverOverloaded"), true));
        assert!(observer.render(&[]).is_none());
        observer.observe(&error(json!("serverOverloaded"), false));
        observer.observe(
            &json!({"method":"turn/completed","params":{"threadId":"thread-a",
            "turn":{"id":"turn-a","status":"completed","error":null}}}),
        );
        assert!(observer.render(&[]).is_none());
        observer.observe(&error(json!("serverOverloaded"), false));
        observer.observe(
            &json!({"method":"turn/started","params":{"threadId":"thread-a",
            "turn":{"id":"turn-b","status":"inProgress"}}}),
        );
        observer.observe(&error(json!("serverOverloaded"), false));
        assert!(observer.render(&[]).is_none());
    }

    #[test]
    fn failed_turn_without_error_code_is_unknown_and_other_threads_cannot_clear_it() {
        let mut observer = AdviceObserver::default();
        observer.observe(
            &json!({"method":"turn/completed","params":{"threadId":"thread-a",
            "turn":{"id":"turn-a","status":"failed","error":null}}}),
        );
        observer.observe(
            &json!({"method":"turn/completed","params":{"threadId":"thread-b",
            "turn":{"id":"turn-b","status":"completed"}}}),
        );
        assert_eq!(
            observer.failures().next().unwrap().reason,
            StopReason::OtherUnknown
        );
        assert!(observer.render(&[]).unwrap().contains("未確認"));
    }

    #[test]
    fn ordinary_rpc_error_and_native_text_never_classify_subscription_usage() {
        let mut observer = AdviceObserver::default();
        observer.observe(&json!({"id":1,"error":{"code":429,"message":"usage limit"}}));
        observer.observe(&json!({"method":"warning","params":{"threadId":"thread-a",
            "message":"serverOverloaded usageLimitExceeded"}}));
        assert!(observer.render(&[]).is_none());
    }

    #[test]
    fn completion_after_retries_records_terminal_error_and_preserves_known_code() {
        let mut observer = AdviceObserver::default();
        observer.observe(&error(json!("serverOverloaded"), true));
        assert_eq!(observer.failures().count(), 0);
        observer.observe(&json!({"method":"turn/completed","params":{"threadId":"thread-a",
            "turn":{"id":"turn-a","status":"failed","error":{"codexErrorInfo":"serverOverloaded"}}}}));
        assert_eq!(
            observer.failures().next().unwrap().reason,
            StopReason::Capacity
        );
        observer.observe(
            &json!({"method":"turn/completed","params":{"threadId":"thread-a",
            "turn":{"id":"turn-a","status":"failed","error":null}}}),
        );
        assert_eq!(
            observer.failures().next().unwrap().reason,
            StopReason::Capacity
        );
    }

    #[test]
    fn output_quotes_identifiers_and_filters_terminal_control_codes() {
        assert_eq!(shell_quote("a'b\u{1b}"), "'a'\\''b'");
        let mut observer = AdviceObserver::default();
        observer.observe(&error(json!("other"), false));
        let text = observer
            .render(&[json!({"model":"gpt-6-sol\u{1b}"})])
            .unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("未確認"));
    }
}
