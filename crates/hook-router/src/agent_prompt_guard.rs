//! Agent(サブエージェント)の prompt に、未置換の雛形の穴(`{{RANGE}}` など)が残っていないかを、送る前に止める。
//!
//! 2026-11-15、サブエージェントへの指示の雛形に `{{RANGE}}` を置いたまま送り、担当が範囲を知らずに 1 件も書かずに
//! 止まった(補足の連絡で再開した。F55)。穴を埋め忘れる誤りは、送る前に文字列として検出できる。
//!
//! 穴は、`{{` と `}}` の間が、大文字の英字で始まる大文字・数字・`_` だけの名前(`{{RANGE}}`・`{{ TARGET_FILE }}`)のものに
//! 限る。`{{ .Values.x }}`(Helm)や `{{name}}`(小文字)は、依頼の対象の本文(テンプレートの例)として正当に書かれうるので、
//! 止めない。逃げ道は設けない: 意図した文字列なら、波括弧の間に空白を入れて(`{ {RANGE} }`)書けば、穴の形でなくなる。

use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};

/// 名前の長さの上限。長い文字列は、穴でなく本文とみなす。
const MAX_NAME: usize = 40;

/// `{{NAME}}` の NAME として許す名前か: 大文字の英字で始まり、大文字・数字・`_` だけ。
fn is_slot_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && s.len() >= 2
        && s.len() <= MAX_NAME
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// prompt にある未置換の穴の名前(重複なし、出現順)。
pub fn unfilled_slots(prompt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = prompt;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        if let Some(j) = after.find("}}") {
            let name = after[..j].trim();
            if is_slot_name(name) && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
        rest = after;
    }
    out
}

fn deny_message(slots: &[String]) -> String {
    let shown: Vec<String> = slots
        .iter()
        .take(5)
        .map(|s| format!("{{{{{s}}}}}"))
        .collect();
    format!(
        "Agent の prompt に、未置換の雛形の穴が残っています: {}。サブエージェントは、穴の値(範囲・対象ファイルなど)を知らずに \
         動き始め、何も書かずに止まります(2026-11-15 に実例)。雛形の穴を実際の値に置き換えてから送ってください。\
         意図した文字列(テンプレートの例など)なら、波括弧の間に空白を入れて `{{ {{NAME}} }}` のように書き、穴の形にしないでください。",
        shown.join("、")
    )
}

/// hook 本体。`hook-router` の規則(`Tools::Named(&["Agent", "Task"])`)が呼ぶ。対象外・読めないときは `None`。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    if !matches!(agent, Agent::Claude | Agent::Antigravity) {
        return None;
    }
    let call = ToolCall::parse(agent, raw)?;
    if !matches!(call.tool.as_str(), "Agent" | "Task") {
        return None;
    }
    let prompt = call.tool_input_alt(&["prompt"])?;
    let slots = unfilled_slots(&prompt);
    (!slots.is_empty()).then(|| Outcome::Deny(deny_message(&slots)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, prompt: &str) -> String {
        serde_json::json!({"tool_name": tool, "tool_input": {"prompt": prompt, "description": "d"}})
            .to_string()
    }

    #[test]
    fn finds_uppercase_slots_in_order_without_duplicates() {
        assert_eq!(
            unfilled_slots("範囲は {{RANGE}}、対象は {{ TARGET_FILE }}、もう一度 {{RANGE}}。"),
            ["RANGE", "TARGET_FILE"]
        );
        assert_eq!(unfilled_slots("{{A1}}"), ["A1"]);
    }

    #[test]
    fn leaves_template_examples_and_other_braces_alone() {
        for p in [
            "Helm の {{ .Values.image }} を直す",
            "小文字の {{name}} は穴とみなさない",
            "1 字の {{X}} は穴とみなさない",
            "{{ 1ST }} は数字で始まる",
            "{ {RANGE} } は空白を入れた書き方",
            "波括弧 {{ だけ",
            "閉じない {{RANGE",
            "JSON: {\"a\": {\"b\": 1}}",
            "",
        ] {
            assert!(unfilled_slots(p).is_empty(), "{p}");
        }
        let long = format!("{{{{{}}}}}", "A".repeat(MAX_NAME + 1));
        assert!(unfilled_slots(&long).is_empty());
    }

    #[test]
    fn denies_an_agent_prompt_with_an_unfilled_slot_and_names_it() {
        for tool in ["Agent", "Task"] {
            let out = hook(Agent::Claude, &call(tool, "範囲は {{RANGE}} です")).unwrap();
            let Outcome::Deny(m) = out else {
                panic!("deny でない")
            };
            assert!(m.contains("{{RANGE}}"), "{m}");
            assert!(m.contains("置き換えてから送って"), "{m}");
        }
    }

    #[test]
    fn passes_other_tools_other_agents_and_clean_prompts() {
        assert!(hook(Agent::Claude, &call("Agent", "穴の無い指示")).is_none());
        assert!(hook(Agent::Claude, &call("Bash", "{{RANGE}}")).is_none());
        assert!(hook(Agent::Codex, &call("Agent", "{{RANGE}}")).is_none());
        assert!(hook(Agent::Claude, "not json").is_none());
        assert!(hook(
            Agent::Claude,
            r#"{"tool_name":"Agent","tool_input":{"description":"d"}}"#
        )
        .is_none());
    }
}
