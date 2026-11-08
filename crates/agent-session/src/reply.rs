//! hook の返答の型と、その描画。出力の形は、ここ 1 か所が持つ。
//!
//! 返答は [`Reply`] で表し、[`Reply::render`] がエージェントとイベントごとの JSON にする。
//! 形式は 1 つに決めている: jq の `jq .` と同じ整形(2 空白インデント、`"k": v`、キーは挿入順、
//! 末尾に改行)。これは `jq_json_compat::jqfmt::J` が持つ。何も言うことが無い返答は、空の文字列。
//!
//! 描画の表(Claude と Codex は同じ。Copilot は PreToolUse の判定だけが平らな形):
//!
//! | イベント | Claude / Codex | Copilot |
//! |---|---|---|
//! | PreToolUse | `hookSpecificOutput{hookEventName, permissionDecision, permissionDecisionReason, additionalContext}` | 判定は平ら `{permissionDecision, permissionDecisionReason}`。文脈だけなら入れ子 |
//! | PostToolUse | `hookSpecificOutput{hookEventName, additionalContext}`(判定は捨てる) | 入れ子(従来の adr-number と同じ) |
//! | SessionStart | `hookSpecificOutput{hookEventName, additionalContext}`(判定は捨てる) | 平ら `{additionalContext}` |
//! | PermissionRequest | deny だけ `hookSpecificOutput{hookEventName, decision{behavior, message}}` | 捨てる |
//!
//! Copilot の SessionStart が最上位の `additionalContext` だけを読むことの出典: GitHub Docs,
//! "GitHub Copilot hooks reference" の `sessionStart`(取得 2026-10-28)。
//!
//! `systemMessage` はどのイベントでも、最上位に付く。Copilot の PostToolUse・systemMessage・
//! PermissionRequest の形は、このリポジトリに実測の記録が無い(未確認)。従来の挙動を保つ。

use jq_json_compat::jqfmt::J;

use crate::decision::{Decision, PermissionDecision};
use crate::event::HookEvent;
use crate::input::Agent;
use crate::outcome::Outcome;

/// hook の返答。`render` が JSON にする。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reply {
    /// PreToolUse の判定(allow・ask・deny)。他のイベントでは、描画のときに捨てる。
    pub decision: Option<PermissionDecision>,
    /// `additionalContext`。
    pub context: Option<String>,
    /// 最上位の `systemMessage`。
    pub system_message: Option<String>,
}

impl Reply {
    /// deny だけの返答。
    pub fn deny(reason: impl Into<String>) -> Reply {
        Reply {
            decision: Some(PermissionDecision::deny(reason)),
            ..Reply::default()
        }
    }

    /// 文脈だけの返答。
    pub fn context(text: impl Into<String>) -> Reply {
        Reply {
            context: Some(text.into()),
            ..Reply::default()
        }
    }

    /// `systemMessage` だけの返答。
    pub fn system_message(message: impl Into<String>) -> Reply {
        Reply {
            system_message: Some(message.into()),
            ..Reply::default()
        }
    }

    /// 検査 1 件の結果から。`None` は「言うことが無い」。
    pub fn from_outcome(outcome: Option<Outcome>) -> Reply {
        match outcome {
            None => Reply::default(),
            Some(o) => Reply::from_outcomes(&[o]),
        }
    }

    /// 複数の検査の結果を 1 つにまとめる。判定は deny > ask > allow の順で最初に空でないものを採り、
    /// 同じ判定の理由は空行(`\n\n`)で、文脈は改行(`\n`)でつなぐ。ルータの併合と同じ規則。
    pub fn from_outcomes(outcomes: &[Outcome]) -> Reply {
        let join = |pick: fn(&Outcome) -> Option<&str>, sep: &str| -> Option<String> {
            let parts: Vec<&str> = outcomes.iter().filter_map(pick).collect();
            (!parts.is_empty()).then(|| parts.join(sep))
        };
        let deny = join(
            |o| match o {
                Outcome::Deny(r) => Some(r),
                _ => None,
            },
            "\n\n",
        );
        let ask = join(
            |o| match o {
                Outcome::Ask(r) => Some(r),
                _ => None,
            },
            "\n\n",
        );
        let allow = join(
            |o| match o {
                Outcome::Allow(r) => Some(r),
                _ => None,
            },
            "\n\n",
        );
        let decision = deny
            .map(PermissionDecision::deny)
            .or_else(|| ask.map(PermissionDecision::ask))
            .or_else(|| allow.map(PermissionDecision::allow));
        let context = join(
            |o| match o {
                Outcome::Context(t) => Some(t),
                _ => None,
            },
            "\n",
        );
        Reply {
            decision,
            context,
            system_message: None,
        }
    }

    /// 返答を、エージェントとイベントの形の JSON にする。言うことが無ければ空の文字列。
    pub fn render(&self, agent: Agent, event: HookEvent) -> String {
        let mut top: Vec<(&str, J)> = Vec::new();
        match agent {
            Agent::Claude | Agent::Codex => {
                if let Some(specific) = self.specific(event) {
                    top.push(("hookSpecificOutput", specific));
                }
            }
            Agent::Copilot => self.copilot_fields(event, &mut top),
        }
        if let Some(message) = &self.system_message {
            top.push(("systemMessage", J::str(message.clone())));
        }
        if top.is_empty() {
            return String::new();
        }
        format!("{}\n", J::obj(top).pretty())
    }

    /// `hookSpecificOutput` の中身。`hookEventName` のほかに言うことが無ければ `None`。
    fn specific(&self, event: HookEvent) -> Option<J> {
        let mut pairs: Vec<(&str, J)> = vec![("hookEventName", J::str(event.name()))];
        match event {
            HookEvent::PreToolUse => {
                if let Some(d) = &self.decision {
                    pairs.push(("permissionDecision", J::str(d.decision.as_str())));
                    pairs.push(("permissionDecisionReason", J::str(d.reason.clone())));
                }
                if let Some(c) = &self.context {
                    pairs.push(("additionalContext", J::str(c.clone())));
                }
            }
            HookEvent::PostToolUse | HookEvent::SessionStart => {
                if let Some(c) = &self.context {
                    pairs.push(("additionalContext", J::str(c.clone())));
                }
            }
            HookEvent::PermissionRequest => {
                if let Some(d) = self
                    .decision
                    .as_ref()
                    .filter(|d| d.decision == Decision::Deny)
                {
                    pairs.push((
                        "decision",
                        J::obj(vec![
                            ("behavior", J::str("deny")),
                            ("message", J::str(d.reason.clone())),
                        ]),
                    ));
                }
            }
        }
        (pairs.len() > 1).then(|| J::obj(pairs))
    }

    fn copilot_fields(&self, event: HookEvent, top: &mut Vec<(&'static str, J)>) {
        match event {
            HookEvent::PreToolUse => {
                if let Some(d) = &self.decision {
                    top.push(("permissionDecision", J::str(d.decision.as_str())));
                    top.push(("permissionDecisionReason", J::str(d.reason.clone())));
                }
                if self.context.is_some() {
                    // 判定が無く文脈だけのときも、入れ子の形にする(従来と同じ)。
                    let context_only = Reply {
                        decision: None,
                        ..self.clone()
                    };
                    if let Some(specific) = context_only.specific(event) {
                        top.push(("hookSpecificOutput", specific));
                    }
                }
            }
            HookEvent::PostToolUse => {
                if let Some(specific) = self.specific(event) {
                    top.push(("hookSpecificOutput", specific));
                }
            }
            HookEvent::SessionStart => {
                if let Some(c) = &self.context {
                    top.push(("additionalContext", J::str(c.clone())));
                }
            }
            HookEvent::PermissionRequest => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny(reason: &str) -> Reply {
        Reply::deny(reason)
    }

    // ---- 従来の jq の整形出力と、1 バイトも変わらないこと(固定値) ----
    //
    // 値は、従来の描画関数(`jqfmt::deny_for_event`・`jqfmt::system_message`)の出力から取った。
    // DEL と C0 は `\u00xx`、`/` は書き換えない。関数は消えたので、この表が唯一の記録である。

    /// (理由, PreToolUse の出力, PermissionRequest の出力)。
    const DENY_GOLDENS: &[(&str, &str, &str)] = &[
        (
            "plain",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"plain\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"plain\"\n    }\n  }\n}\n",
        ),
        (
            "line1\nline2",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"line1\\nline2\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"line1\\nline2\"\n    }\n  }\n}\n",
        ),
        (
            "quote \" backslash \\ slash / tab \t",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"quote \\\" backslash \\\\ slash / tab \\t\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"quote \\\" backslash \\\\ slash / tab \\t\"\n    }\n  }\n}\n",
        ),
        (
            "ctl \u{1} \u{1b} del \u{7f}",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"ctl \\u0001 \\u001b del \\u007f\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"ctl \\u0001 \\u001b del \\u007f\"\n    }\n  }\n}\n",
        ),
        (
            "日本語と é",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"日本語と é\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"日本語と é\"\n    }\n  }\n}\n",
        ),
        (
            "",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"\"\n  }\n}\n",
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"\"\n    }\n  }\n}\n",
        ),
    ];

    /// (メッセージ, 出力)。
    const SYSTEM_MESSAGE_GOLDENS: &[(&str, &str)] = &[
        ("m", "{\n  \"systemMessage\": \"m\"\n}\n"),
        ("a\nb", "{\n  \"systemMessage\": \"a\\nb\"\n}\n"),
        (
            "日本語 \" \\ \u{7f}",
            "{\n  \"systemMessage\": \"日本語 \\\" \\\\ \\u007f\"\n}\n",
        ),
    ];

    #[test]
    fn deny_renders_the_exact_bytes() {
        for (reason, pre, perm) in DENY_GOLDENS {
            for agent in [Agent::Claude, Agent::Codex] {
                assert_eq!(
                    deny(reason).render(agent, HookEvent::PreToolUse),
                    *pre,
                    "{agent:?} {reason:?}"
                );
            }
            assert_eq!(
                deny(reason).render(Agent::Claude, HookEvent::PermissionRequest),
                *perm,
                "{reason:?}"
            );
            // 従来の Copilot の平らな形(hook::deny_output の旧実装)を、そのまま組み立てて比べる
            let legacy_copilot = format!(
                "{}\n",
                J::obj(vec![
                    ("permissionDecision", J::str("deny")),
                    ("permissionDecisionReason", J::str(*reason)),
                ])
                .pretty()
            );
            assert_eq!(
                deny(reason).render(Agent::Copilot, HookEvent::PreToolUse),
                legacy_copilot,
                "{reason:?}"
            );
        }
    }

    #[test]
    fn system_message_renders_the_exact_bytes() {
        for (message, expected) in SYSTEM_MESSAGE_GOLDENS {
            assert_eq!(
                Reply::system_message(*message).render(Agent::Claude, HookEvent::PreToolUse),
                *expected,
                "{message:?}"
            );
        }
    }

    // ---- 描画の表(exact bytes) ----

    #[test]
    fn claude_and_codex_render_the_same_bytes() {
        let r = Reply {
            decision: Some(PermissionDecision::ask("確認")),
            context: Some("文脈".into()),
            system_message: Some("注意".into()),
        };
        for event in [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::SessionStart,
            HookEvent::PermissionRequest,
        ] {
            assert_eq!(
                r.render(Agent::Claude, event),
                r.render(Agent::Codex, event),
                "{event:?}"
            );
        }
    }

    #[test]
    fn pre_tool_use_with_decision_and_context() {
        let r = Reply {
            decision: Some(PermissionDecision::deny("no")),
            context: Some("ctx".into()),
            system_message: None,
        };
        assert_eq!(
            r.render(Agent::Claude, HookEvent::PreToolUse),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"no\",\n    \"additionalContext\": \"ctx\"\n  }\n}\n"
        );
    }

    #[test]
    fn ask_and_allow_use_their_own_words() {
        assert_eq!(
            Reply {
                decision: Some(PermissionDecision::ask("?")),
                ..Reply::default()
            }
            .render(Agent::Claude, HookEvent::PreToolUse),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"ask\",\n    \"permissionDecisionReason\": \"?\"\n  }\n}\n"
        );
        assert!(Reply {
            decision: Some(PermissionDecision::allow("ok")),
            ..Reply::default()
        }
        .render(Agent::Claude, HookEvent::PreToolUse)
        .contains("\"permissionDecision\": \"allow\""));
    }

    #[test]
    fn post_tool_use_and_session_start_carry_context_and_drop_decisions() {
        let r = Reply {
            decision: Some(PermissionDecision::deny("捨てる")),
            context: Some("c".into()),
            system_message: None,
        };
        for event in [HookEvent::PostToolUse, HookEvent::SessionStart] {
            let out = r.render(Agent::Claude, event);
            assert!(!out.contains("permissionDecision"), "{event:?}: {out}");
            assert!(out.contains("\"additionalContext\": \"c\""), "{event:?}");
            assert!(out.contains(&format!("\"hookEventName\": \"{}\"", event.name())));
        }
        // 判定だけの返答は、PostToolUse・SessionStart では何も言わない
        assert_eq!(deny("x").render(Agent::Claude, HookEvent::PostToolUse), "");
        assert_eq!(deny("x").render(Agent::Claude, HookEvent::SessionStart), "");
    }

    #[test]
    fn permission_request_keeps_only_deny() {
        assert_eq!(
            deny("r").render(Agent::Claude, HookEvent::PermissionRequest),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"r\"\n    }\n  }\n}\n"
        );
        for d in [
            PermissionDecision::ask("?"),
            PermissionDecision::allow("ok"),
        ] {
            assert_eq!(
                Reply {
                    decision: Some(d),
                    ..Reply::default()
                }
                .render(Agent::Claude, HookEvent::PermissionRequest),
                ""
            );
        }
    }

    #[test]
    fn system_message_alone_has_no_hook_specific_output() {
        assert_eq!(
            Reply::system_message("s").render(Agent::Claude, HookEvent::PostToolUse),
            "{\n  \"systemMessage\": \"s\"\n}\n"
        );
    }

    #[test]
    fn system_message_follows_hook_specific_output() {
        let r = Reply {
            context: Some("c".into()),
            system_message: Some("s".into()),
            ..Reply::default()
        };
        assert_eq!(
            r.render(Agent::Claude, HookEvent::PostToolUse),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PostToolUse\",\n    \"additionalContext\": \"c\"\n  },\n  \"systemMessage\": \"s\"\n}\n"
        );
    }

    #[test]
    fn an_empty_reply_says_nothing() {
        for agent in [Agent::Claude, Agent::Codex, Agent::Copilot] {
            for event in [
                HookEvent::PreToolUse,
                HookEvent::PostToolUse,
                HookEvent::SessionStart,
                HookEvent::PermissionRequest,
            ] {
                assert_eq!(
                    Reply::default().render(agent, event),
                    "",
                    "{agent:?} {event:?}"
                );
            }
        }
    }

    // ---- Copilot ----

    #[test]
    fn copilot_pre_tool_use_decision_is_flat() {
        assert_eq!(
            deny("no").render(Agent::Copilot, HookEvent::PreToolUse),
            "{\n  \"permissionDecision\": \"deny\",\n  \"permissionDecisionReason\": \"no\"\n}\n"
        );
    }

    #[test]
    fn copilot_session_start_context_is_flat() {
        assert_eq!(
            Reply::context("c").render(Agent::Copilot, HookEvent::SessionStart),
            "{\n  \"additionalContext\": \"c\"\n}\n"
        );
    }

    #[test]
    fn copilot_post_tool_use_context_stays_nested() {
        assert_eq!(
            Reply::context("c").render(Agent::Copilot, HookEvent::PostToolUse),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PostToolUse\",\n    \"additionalContext\": \"c\"\n  }\n}\n"
        );
    }

    #[test]
    fn copilot_permission_request_says_nothing() {
        assert_eq!(
            deny("r").render(Agent::Copilot, HookEvent::PermissionRequest),
            ""
        );
    }

    // ---- 併合(ルータと同じ規則) ----

    #[test]
    fn from_outcomes_picks_deny_over_ask_over_allow() {
        let r = Reply::from_outcomes(&[
            Outcome::Allow("a".into()),
            Outcome::Ask("k".into()),
            Outcome::Deny("d1".into()),
            Outcome::Deny("d2".into()),
        ]);
        assert_eq!(r.decision, Some(PermissionDecision::deny("d1\n\nd2")));
        let r = Reply::from_outcomes(&[Outcome::Allow("a".into()), Outcome::Ask("k".into())]);
        assert_eq!(r.decision, Some(PermissionDecision::ask("k")));
        let r = Reply::from_outcomes(&[Outcome::Allow("a".into())]);
        assert_eq!(r.decision, Some(PermissionDecision::allow("a")));
    }

    #[test]
    fn from_outcomes_joins_contexts_with_a_newline() {
        let r = Reply::from_outcomes(&[
            Outcome::Context("c1".into()),
            Outcome::Deny("d".into()),
            Outcome::Context("c2".into()),
        ]);
        assert_eq!(r.context.as_deref(), Some("c1\nc2"));
        assert_eq!(r.decision, Some(PermissionDecision::deny("d")));
    }

    #[test]
    fn from_outcome_none_is_an_empty_reply() {
        assert_eq!(Reply::from_outcome(None), Reply::default());
    }
}
