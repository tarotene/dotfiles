//! Antigravity CLI(`agy`)の hook の境界。入力を Claude の形へ直し、返答を agy の形へ描く。
//!
//! agy の hook は、camelCase の JSON を標準入力で受け(`toolCall{name,args}`・`conversationId`・
//! `hookEventName` など)、標準出力の JSON(`decision`・`reason`・`injectSteps`)で返す。検査の本体は
//! Claude の形(`tool_name`・`tool_input`・`session_id`)で書かれているので、検査の手前
//! ([`normalize`])で入力を Claude の形に直し、検査の後ろ([`render`])で返答を agy の形にする。
//! 検査ごとの別実装を作らないための 1 か所である(Codex の `apply_patch` を router の境界で
//! 直しているのと同じ型。`docs/claude/codex-hook-parity.md`)。
//!
//! 契約の出典: agy 1.5.17 に同梱の `agy-customizations/docs/hooks.md`(取得 2026-11-27)。道具名と
//! 引数の名前は、同じ版の会話記録(`brain/*/.system_generated/logs/transcript.jsonl`)から実測した。
//!
//! 未確認(実機の payload で確かめる。`docs/claude/antigravity-hook-parity.md`):
//! - MCP の道具が `toolCall.name` に載せる名前の形。今は名前をそのまま通す。
//! - PreToolUse で「意見なし」を、何も出さない(空の標準出力)で表してよいか。
//! - `Grep`・`Glob` に当たる道具の名前。

use serde_json::{json, Map, Value};

use crate::decision::{Decision, PermissionDecision};
use crate::event::HookEvent;
use crate::reply::Reply;

/// agy の道具名を、検査の語彙(Claude の名前)にする。知らない名前は、そのまま返す。
pub fn tool_name(agy: &str) -> &str {
    match agy {
        "run_command" => "Bash",
        "read_file" => "Read",
        "edit_file" => "Edit",
        "write_file" => "Write",
        "exit_plan_mode" => "ExitPlanMode",
        "invoke_subagent" => "Agent",
        other => other,
    }
}

/// agy の道具の引数を、検査の語彙(Claude の `tool_input`)にする。知らない道具の引数は、そのまま返す。
fn tool_input(agy_tool: &str, args: &Value) -> Value {
    let pick = |key: &str| args.get(key).cloned().unwrap_or(Value::Null);
    match agy_tool {
        "run_command" => json!({ "command": pick("CommandLine") }),
        "read_file" => json!({ "file_path": pick("Path") }),
        "edit_file" => json!({
            "file_path": pick("Path"),
            "old_string": pick("Target"),
            "new_string": pick("Replacement"),
        }),
        "write_file" => json!({ "file_path": pick("Path"), "content": pick("Content") }),
        "exit_plan_mode" => json!({ "plan": pick("plan") }),
        "invoke_subagent" => json!({ "prompt": pick("Prompt"), "subagent_type": pick("TypeName") }),
        _ => args.clone(),
    }
}

/// PostToolUse の `error`(`exit status 1` の形)から終了コードを読む。読めなければ -1(不明)。
fn exit_code(error: &str) -> i64 {
    error
        .trim()
        .strip_prefix("exit status ")
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(-1)
}

/// agy の hook の入力を、Claude の形の JSON にする。agy の形(`toolCall`・`conversationId`・
/// `hookEventName` のどれか)でなければ、そのまま返す(すでに Claude の形、または JSON でない)。
pub fn normalize(raw: &str) -> String {
    let Ok(Value::Object(input)) = serde_json::from_str::<Value>(raw) else {
        return raw.to_owned();
    };
    let is_agy = ["toolCall", "conversationId", "hookEventName"]
        .iter()
        .any(|k| input.contains_key(*k));
    if !is_agy {
        return raw.to_owned();
    }
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let mut out = Map::new();
    let mut put = |key: &str, value: Option<&str>| {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            out.insert(key.to_owned(), json!(v));
        }
    };
    put("session_id", text("conversationId"));
    put("hook_event_name", text("hookEventName"));
    put("transcript_path", text("transcriptPath"));
    put("model", text("modelName"));
    put("prompt", text("lastUserInput"));
    let cwd = text("cwd").or_else(|| {
        input
            .get("workspacePaths")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
    });
    put("cwd", cwd);
    if let Some(active) = input.get("continuationActive").and_then(Value::as_bool) {
        out.insert("stop_hook_active".to_owned(), json!(active));
    }
    if let Some(call) = input.get("toolCall") {
        let agy_tool = call.get("name").and_then(Value::as_str).unwrap_or_default();
        let args = call.get("args").cloned().unwrap_or(Value::Null);
        out.insert("tool_name".to_owned(), json!(tool_name(agy_tool)));
        out.insert("tool_input".to_owned(), tool_input(agy_tool, &args));
        if input.get("hookEventName").and_then(Value::as_str) == Some("PostToolUse") {
            let error = text("error").filter(|e| !e.is_empty());
            let response = match (agy_tool, error) {
                ("run_command", Some(e)) => json!({ "exitCode": exit_code(e), "error": e }),
                ("run_command", None) => json!({ "exitCode": 0 }),
                (_, Some(e)) => json!({ "error": e }),
                (_, None) => Value::Null,
            };
            if !response.is_null() {
                out.insert("tool_response".to_owned(), response);
            }
        }
    }
    Value::Object(out).to_string()
}

/// Claude の形の hook の出力を、[`Reply`] にする。読めない出力は、何も言わない返答。
///
/// 読むのは `hookSpecificOutput`(`permissionDecision`・`permissionDecisionReason`・
/// `additionalContext`)、最上位の `decision: "block"`+`reason`(Stop)、最上位の `systemMessage`。
pub fn reply_from_claude(stdout: &[u8]) -> Reply {
    let Ok(out) = serde_json::from_slice::<Value>(stdout) else {
        return Reply::default();
    };
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let specific = &out["hookSpecificOutput"];
    let mut reply = Reply {
        context: text(&specific["additionalContext"]),
        system_message: text(&out["systemMessage"]),
        ..Reply::default()
    };
    let reason = text(&specific["permissionDecisionReason"]).unwrap_or_default();
    reply.decision = match specific["permissionDecision"].as_str() {
        Some("deny") => Some(PermissionDecision::deny(reason)),
        Some("ask") => Some(PermissionDecision::ask(reason)),
        Some("allow") => Some(PermissionDecision::allow(reason)),
        _ => None,
    };
    // Stop の block は、最上位の `decision: "block"`。agy では deny として運ぶ(`render` が
    // `decision: "continue"` にする)。
    if out["decision"] == "block" {
        reply.decision = Some(PermissionDecision::deny(
            text(&out["reason"]).unwrap_or_default(),
        ));
    }
    reply
}

/// 返答を、agy の hook の出力にする。言うことが無ければ空の文字列、または空の JSON。
///
/// | イベント | 出力 |
/// |---|---|
/// | PreToolUse | 判定(allow・ask・deny)があれば `{"decision","reason"}`。無ければ何も出さない。文脈だけの返答は届け先が無いので捨てる |
/// | PostToolUse | 常に `{}`(文書が空の JSON を求める) |
/// | SessionStart | 文脈・`systemMessage` を `injectSteps` の `systemMessage` にする。無ければ `{}` |
/// | Stop | deny(Claude の block)だけ `{"decision":"continue","reason"}`(止めを阻む)。他は `{}` |
/// | PermissionRequest | 何も出さない(agy に無いイベント) |
pub fn render(reply: &Reply, event: HookEvent) -> String {
    let body = match event {
        HookEvent::PreToolUse => {
            let Some(d) = &reply.decision else {
                return String::new();
            };
            json!({ "decision": d.decision.as_str(), "reason": reason(&d.reason) })
        }
        HookEvent::PostToolUse => json!({}),
        HookEvent::SessionStart => {
            let text: Vec<&str> = [reply.context.as_deref(), reply.system_message.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            if text.is_empty() {
                json!({})
            } else {
                json!({ "injectSteps": [{ "systemMessage": { "systemMessage": text.join("\n") } }] })
            }
        }
        HookEvent::Stop => match reply
            .decision
            .as_ref()
            .filter(|d| d.decision == Decision::Deny)
        {
            Some(d) => json!({ "decision": "continue", "reason": reason(&d.reason) }),
            None => json!({}),
        },
        HookEvent::PermissionRequest => return String::new(),
    };
    format!("{body}\n")
}

/// agy は「理由の無い決定」を受けない(`a %s decision needs a reason`)。空の理由は定型文にする。
fn reason(text: &str) -> String {
    if text.is_empty() {
        "hook が止めました".to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::PermissionDecision;

    fn parsed(raw: &str) -> Value {
        serde_json::from_str(&normalize(raw)).unwrap()
    }

    #[test]
    fn a_run_command_call_becomes_a_bash_call() {
        let v = parsed(
            r#"{"toolCall":{"id":"t1","name":"run_command","args":{"CommandLine":"npm test","toolSummary":"x"}},
                "stepIdx":19,"conversationId":"c1","workspacePaths":["/w"],"cwd":"/w/sub",
                "transcriptPath":"/w/t.jsonl","modelName":"auto","lastUserInput":"fix it","hookEventName":"PreToolUse"}"#,
        );
        assert_eq!(v["tool_name"], "Bash");
        assert_eq!(v["tool_input"]["command"], "npm test");
        assert_eq!(v["session_id"], "c1");
        assert_eq!(v["cwd"], "/w/sub");
        assert_eq!(v["transcript_path"], "/w/t.jsonl");
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(v["prompt"], "fix it");
    }

    #[test]
    fn file_tools_map_to_the_shared_names_and_keys() {
        let edit = parsed(
            r#"{"hookEventName":"PreToolUse","toolCall":{"name":"edit_file","args":{"Path":"/a","Target":"x","Replacement":"y"}}}"#,
        );
        assert_eq!(edit["tool_name"], "Edit");
        assert_eq!(edit["tool_input"]["file_path"], "/a");
        assert_eq!(edit["tool_input"]["old_string"], "x");
        assert_eq!(edit["tool_input"]["new_string"], "y");
        let write = parsed(
            r#"{"hookEventName":"PreToolUse","toolCall":{"name":"write_file","args":{"Path":"/a","Content":"c","Overwrite":true}}}"#,
        );
        assert_eq!(write["tool_name"], "Write");
        assert_eq!(write["tool_input"]["content"], "c");
        let read = parsed(
            r#"{"hookEventName":"PreToolUse","toolCall":{"name":"read_file","args":{"Path":"/a","StartLine":1}}}"#,
        );
        assert_eq!(read["tool_name"], "Read");
        assert_eq!(read["tool_input"]["file_path"], "/a");
    }

    #[test]
    fn exit_plan_mode_keeps_the_plan_text() {
        let v = parsed(
            r###"{"hookEventName":"PreToolUse","toolCall":{"name":"exit_plan_mode","args":{"plan":"## 計画"}}}"###,
        );
        assert_eq!(v["tool_name"], "ExitPlanMode");
        assert_eq!(v["tool_input"]["plan"], "## 計画");
    }

    #[test]
    fn an_unknown_tool_keeps_its_name_and_arguments() {
        let v = parsed(
            r#"{"hookEventName":"PreToolUse","toolCall":{"name":"mcp_x_y","args":{"a":1}}}"#,
        );
        assert_eq!(v["tool_name"], "mcp_x_y");
        assert_eq!(v["tool_input"]["a"], 1);
    }

    #[test]
    fn post_tool_use_reads_the_exit_code_from_the_error() {
        let failed = parsed(
            r#"{"hookEventName":"PostToolUse","error":"exit status 7","toolCall":{"name":"run_command","args":{"CommandLine":"false"}}}"#,
        );
        assert_eq!(failed["tool_response"]["exitCode"], 7);
        let unknown = parsed(
            r#"{"hookEventName":"PostToolUse","error":"timed out","toolCall":{"name":"run_command","args":{}}}"#,
        );
        assert_eq!(unknown["tool_response"]["exitCode"], -1);
        let ok = parsed(
            r#"{"hookEventName":"PostToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"true"}}}"#,
        );
        assert_eq!(ok["tool_response"]["exitCode"], 0);
    }

    #[test]
    fn stop_carries_the_continuation_flag() {
        let v = parsed(
            r#"{"hookEventName":"Stop","conversationId":"c","continuationActive":true,"terminationReason":"NO_TOOL_CALL"}"#,
        );
        assert_eq!(v["stop_hook_active"], true);
        assert_eq!(v["session_id"], "c");
    }

    #[test]
    fn a_claude_shaped_or_broken_input_passes_through() {
        let claude = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        assert_eq!(normalize(claude), claude);
        assert_eq!(normalize("not json"), "not json");
    }

    #[test]
    fn the_first_workspace_is_the_cwd_when_cwd_is_missing() {
        let v = parsed(r#"{"hookEventName":"SessionStart","workspacePaths":["/w","/x"]}"#);
        assert_eq!(v["cwd"], "/w");
    }

    // ---- 出力 ----

    fn with(decision: Option<PermissionDecision>, context: Option<&str>) -> Reply {
        Reply {
            decision,
            context: context.map(str::to_owned),
            system_message: None,
        }
    }

    #[test]
    fn pre_tool_use_renders_the_decision_and_says_nothing_otherwise() {
        assert_eq!(
            render(&Reply::deny("だめ"), HookEvent::PreToolUse),
            "{\"decision\":\"deny\",\"reason\":\"だめ\"}\n"
        );
        assert_eq!(
            render(
                &with(Some(PermissionDecision::ask("確認")), None),
                HookEvent::PreToolUse
            ),
            "{\"decision\":\"ask\",\"reason\":\"確認\"}\n"
        );
        // 文脈だけの返答は、PreToolUse には届け先が無い。
        assert_eq!(render(&with(None, Some("c")), HookEvent::PreToolUse), "");
        assert_eq!(render(&Reply::default(), HookEvent::PreToolUse), "");
    }

    #[test]
    fn a_decision_always_carries_a_reason() {
        assert!(render(&Reply::deny(""), HookEvent::PreToolUse).contains("hook が止めました"));
    }

    #[test]
    fn post_tool_use_is_an_empty_object() {
        assert_eq!(render(&Reply::deny("x"), HookEvent::PostToolUse), "{}\n");
        assert_eq!(render(&Reply::default(), HookEvent::PostToolUse), "{}\n");
    }

    #[test]
    fn session_start_injects_a_system_message() {
        assert_eq!(
            render(&Reply::context("文脈"), HookEvent::SessionStart),
            "{\"injectSteps\":[{\"systemMessage\":{\"systemMessage\":\"文脈\"}}]}\n"
        );
        assert_eq!(render(&Reply::default(), HookEvent::SessionStart), "{}\n");
    }

    #[test]
    fn a_stop_block_asks_the_agent_to_continue() {
        assert_eq!(
            render(&Reply::deny("PR が無い"), HookEvent::Stop),
            "{\"decision\":\"continue\",\"reason\":\"PR が無い\"}\n"
        );
        assert_eq!(render(&Reply::default(), HookEvent::Stop), "{}\n");
        assert_eq!(
            render(
                &with(Some(PermissionDecision::ask("?")), None),
                HookEvent::Stop
            ),
            "{}\n"
        );
    }

    #[test]
    fn permission_request_has_no_antigravity_shape() {
        assert_eq!(render(&Reply::deny("r"), HookEvent::PermissionRequest), "");
    }
}
