//! PreToolUse の判定(allow・ask・deny)と理由の型。
//!
//! 出力の形(Claude・Codex の `hookSpecificOutput` の封筒、Copilot の平らな形)は、
//! [`crate::reply::Reply::render`] が持つ。この型は、判定を値として運ぶだけで、JSON を作らない。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

impl Decision {
    /// 出力の `permissionDecision` に書く語。
    pub fn as_str(self) -> &'static str {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_carry_the_decision_and_the_reason() {
        for (d, word) in [
            (PermissionDecision::allow("r"), "allow"),
            (PermissionDecision::deny("r"), "deny"),
            (PermissionDecision::ask("r"), "ask"),
        ] {
            assert_eq!(d.decision.as_str(), word);
            assert_eq!(d.reason, "r");
        }
    }
}
