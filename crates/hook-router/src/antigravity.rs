//! Antigravity CLI(`agy`)の hook の境界。`docs/claude/antigravity-hook-parity.md`。
//!
//! ツール呼び出しの検査(`hook-router pre|post --agent antigravity`)は、入力を
//! [`agent_session::antigravity::normalize`] で Claude の形に直してから、同じ規則の表を走らせる。
//! このモジュールは、規則の表に載らない別の hook(SessionStart・Stop・ExitPlanMode の個別の bin)を、
//! 同じ宣言のまま agy に登録するための橋 [`wrap`] を持つ。`hook-router compat`(Codex)と同じ型で、
//! 検査ごとの agy 版を作らない。

use agent_session::{Agent, Decision, HookEvent, PermissionDecision, Reply};
use serde_json::Value;
use std::time::Duration;

/// 正規化した入力のイベント(`hook_event_name`)。agy に無い・読めないイベントは `None`。
fn event_of(normalized: &Value) -> Option<HookEvent> {
    match normalized.get("hook_event_name")?.as_str()? {
        "PreToolUse" => Some(HookEvent::PreToolUse),
        "PostToolUse" => Some(HookEvent::PostToolUse),
        "SessionStart" => Some(HookEvent::SessionStart),
        "Stop" => Some(HookEvent::Stop),
        _ => None,
    }
}

/// Claude の形で書かれた hook の command を、agy の hook として走らせる。
///
/// agy の入力を Claude の形に直して `sh -c <command>` の標準入力に渡し、標準出力を agy の形にする。
/// 終了コード 2(Claude の「止める」)は、PreToolUse と Stop では deny・continue として扱う。
/// command が起動できない・時間を越えたときは、何も言わない(fail-open。`hook-router` の他の経路と同じ)。
/// agy が知らないイベントには、空の JSON を返す。
pub fn wrap(command: &str, raw: &str, limit: Duration) -> String {
    let normalized = agent_session::antigravity::normalize(raw);
    let value: Value = serde_json::from_str(&normalized).unwrap_or(Value::Null);
    let Some(event) = event_of(&value) else {
        return "{}\n".to_owned();
    };
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command]);
    if let Some(cwd) = value["cwd"]
        .as_str()
        .filter(|c| std::path::Path::new(c).is_dir())
    {
        cmd.current_dir(cwd);
    }
    let Some((status, stdout)) =
        process_control::output_with_timeout(&mut cmd, limit, Some(normalized.as_bytes()))
    else {
        return Reply::default().render(Agent::Antigravity, event);
    };
    let mut reply = agent_session::antigravity::reply_from_claude(&stdout);
    if status.code() == Some(2) && reply.decision.is_none() {
        reply.decision = Some(PermissionDecision::deny(
            "hook の command が終了コード 2 で止めました。",
        ));
    }
    // agy の Stop は、deny だけを「止めを阻む」に写す。ask・allow は描画が捨てる。
    if event == HookEvent::Stop
        && reply
            .decision
            .as_ref()
            .is_some_and(|d| d.decision != Decision::Deny)
    {
        reply.decision = None;
    }
    reply.render(Agent::Antigravity, event)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(10);

    fn agy_stop() -> String {
        r#"{"hookEventName":"Stop","conversationId":"c1","cwd":"/tmp","continuationActive":false}"#
            .to_owned()
    }

    #[test]
    fn a_claude_block_becomes_continue() {
        let out = wrap(
            r#"cat >/dev/null; printf '%s' '{"decision":"block","reason":"PR がありません"}'"#,
            &agy_stop(),
            LIMIT,
        );
        assert_eq!(
            out,
            "{\"decision\":\"continue\",\"reason\":\"PR がありません\"}\n"
        );
    }

    #[test]
    fn the_command_sees_a_claude_shaped_input() {
        // 入力の `session_id` と `stop_hook_active` を、そのまま理由に載せて返させる。
        let out = wrap(
            r#"jq -c '{decision:"block", reason:(.session_id + " " + (.stop_hook_active|tostring))}'"#,
            &agy_stop(),
            LIMIT,
        );
        assert_eq!(out, "{\"decision\":\"continue\",\"reason\":\"c1 false\"}\n");
    }

    #[test]
    fn session_start_context_becomes_an_inject_step() {
        let raw = r#"{"hookEventName":"SessionStart","conversationId":"c1","cwd":"/tmp"}"#;
        let out = wrap(
            r#"cat >/dev/null; printf '%s' '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"索引"}}'"#,
            raw,
            LIMIT,
        );
        assert_eq!(
            out,
            "{\"injectSteps\":[{\"systemMessage\":{\"systemMessage\":\"索引\"}}]}\n"
        );
    }

    #[test]
    fn pre_tool_use_exit_code_two_is_a_deny() {
        let raw = r#"{"hookEventName":"PreToolUse","conversationId":"c1","toolCall":{"name":"run_command","args":{"CommandLine":"ls"}}}"#;
        let out = wrap("cat >/dev/null; exit 2", raw, LIMIT);
        assert!(out.starts_with("{\"decision\":\"deny\""), "{out}");
    }

    #[test]
    fn silence_stays_silent_per_event() {
        let pre = r#"{"hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{}}}"#;
        assert_eq!(wrap("cat >/dev/null", pre, LIMIT), "");
        assert_eq!(wrap("cat >/dev/null", &agy_stop(), LIMIT), "{}\n");
    }

    #[test]
    fn an_event_agy_adds_later_gets_an_empty_object() {
        let raw = r#"{"hookEventName":"PreInvocation","conversationId":"c1"}"#;
        assert_eq!(wrap("echo never-run", raw, LIMIT), "{}\n");
    }

    #[test]
    fn a_stop_ask_is_dropped_not_turned_into_a_block() {
        let out = wrap(
            r#"cat >/dev/null; printf '%s' '{"hookSpecificOutput":{"permissionDecision":"ask","permissionDecisionReason":"?"}}'"#,
            &agy_stop(),
            LIMIT,
        );
        assert_eq!(out, "{}\n");
    }
}
