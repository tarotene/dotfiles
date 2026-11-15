//! hook-router の規則 `reply-coverage-gate`(PreToolUse)と `reply-thread-snapshot`(PostToolUse)の入口。
//!
//! 有効にした機体(`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/reply-coverage` の目印がある機体。
//! `home/modules/reply-coverage.nix` が置く)だけで働く。目印が無ければ何もしない。
//! ツール名と入力からの本文の取り出し(`ja_polish::gate::mcp_text`)は ja-polish が持つ正本で、
//! 呼び出し側(hook-router)が `text_of` として渡す。

use crate::gate;
use crate::store::{self, Store};
use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use serde_json::Value;
use std::path::PathBuf;

/// ツール名と入力から、呼び出しの本文を取り出す関数。
pub type TextOf = fn(&str, &Value) -> Option<String>;

fn marker_path() -> Option<PathBuf> {
    Some(
        process_env::Xdg::Config
            .resolve()?
            .join("dotfiles")
            .join("reply-coverage"),
    )
}

/// この機体で、関所を有効にしているか。
pub fn enabled() -> bool {
    marker_path().is_some_and(|p| p.exists())
}

fn tool_input(call: &ToolCall) -> Option<&Value> {
    let key = match call.agent {
        Agent::Copilot => "toolArgs",
        Agent::Claude | Agent::Codex => "tool_input",
    };
    call.raw.get(key)
}

/// PreToolUse: 返信の下書き・送信で、会話を読んでいない / 検査していない呼び出しを拒否する。
pub fn pre(agent: Agent, raw: &str, text_of: TextOf) -> Option<Outcome> {
    if agent_session::escape::REPLY_COVERAGE_GATE
        .active(None, None)
        .is_some()
    {
        return None;
    }
    if !enabled() {
        return None;
    }
    let call = ToolCall::parse(agent, raw)?;
    if !call.tool.starts_with("mcp__") {
        return None;
    }
    let input = tool_input(&call)?;
    let text = text_of(&call.tool, input)?;
    let store = Store::from_env()?;
    let reason = gate::decide(&store, &call.tool, input, &text, store::now())?;
    Outcome::deny(reason)
}

/// PostToolUse(記録役): Gmail の `get_thread` の応答から、会話の全文を保存する。出力は無い。
pub fn post(agent: Agent, raw: &str) -> Option<Outcome> {
    if agent == Agent::Copilot || !enabled() {
        return None;
    }
    let call = ToolCall::parse(agent, raw)?;
    if !gate::is_get_thread(&call.tool) {
        return None;
    }
    let input = tool_input(&call)?;
    let response = call.raw.get("tool_response")?;
    let store = Store::from_env()?;
    gate::snapshot_from_get_thread(&store, input, response, store::now());
    None
}
