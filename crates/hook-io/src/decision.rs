//! クラスタ A: `permissionDecision` の emit。
//!
//! 吸収元(10 個の独立実装のうち代表):
//! - `config/claude/hooks/git-worktree-allow.sh:97-103`(allow)
//! - `config/claude/hooks/stack-base-guard.sh:480-485`(deny)
//! - `config/codex/hooks/attribution-guard.sh:17-19`(Codex: Claude と同形)
//! - `config/copilot/hooks/attribution-guard.sh:48-53`(Copilot: ラップ無しの直下)

use crate::input::Agent;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

/// PreToolUse の判定と理由。「判定しない(通常フローに任せる)」は
/// この型を作らず、何も出力しないことで表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub decision: Decision,
    pub reason: String,
}

impl PermissionDecision {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Allow,
            reason: reason.into(),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Deny,
            reason: reason.into(),
        }
    }

    pub fn ask(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Ask,
            reason: reason.into(),
        }
    }

    /// エージェントが読む形の JSON。
    pub fn to_json(&self, agent: Agent) -> Value {
        let body = json!({
            "permissionDecision": self.decision.as_str(),
            "permissionDecisionReason": self.reason,
        });
        match agent {
            Agent::Copilot => body,
            Agent::Claude | Agent::Codex => {
                let mut wrapped = json!({ "hookEventName": "PreToolUse" });
                wrapped
                    .as_object_mut()
                    .expect("object literal")
                    .extend(body.as_object().expect("object literal").clone());
                json!({ "hookSpecificOutput": wrapped })
            }
        }
    }

    /// stdout に 1 行で書き出す。
    pub fn emit(&self, agent: Agent) {
        println!("{}", self.to_json(agent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_and_codex_are_wrapped() {
        let v = PermissionDecision::allow("ok").to_json(Agent::Claude);
        assert_eq!(
            v,
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"ok"}})
        );
        assert_eq!(v, PermissionDecision::allow("ok").to_json(Agent::Codex));
    }

    #[test]
    fn copilot_is_flat() {
        let v = PermissionDecision::deny("no").to_json(Agent::Copilot);
        assert_eq!(
            v,
            json!({"permissionDecision":"deny","permissionDecisionReason":"no"})
        );
    }

    #[test]
    fn ask_value() {
        let v = PermissionDecision::ask("?").to_json(Agent::Copilot);
        assert_eq!(v["permissionDecision"], "ask");
    }
}
