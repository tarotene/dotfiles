//! ルータ(`crates/hook-router`)に載る検査の、判定の結果。
//!
//! 各 hook crate は、`main` が標準入力を読んだあとにしていたことを、`pub fn hook(…)
//! -> Option<Outcome>` として lib に持つ。`main` はそれを呼んで、これまでと同じ書式で
//! 印字するだけ(bin の出力は変えない)。ルータは同じ関数をプロセス内で呼び、結果を 1 つの
//! 応答にまとめる。`None` は「判定しない(通常のフローに任せる)」。

/// 検査 1 件の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// PreToolUse の deny(理由)。
    Deny(String),
    /// `additionalContext`(PreToolUse / PostToolUse のどちらかは、呼ばれたイベントで決まる)。
    Context(String),
}

use crate::decision::{Decision, PermissionDecision};

impl Outcome {
    /// deny の `PermissionDecision` を `Outcome` にする。allow / ask は `None`: ルータに載せるのは
    /// deny だけを出す検査で、allow / ask を出す検査(`git-worktree-allow`、`gh-edit-allow` の判定役)は
    /// Claude Code 自身の `if` の評価に任せるため、ここでは扱わない。
    pub fn from_decision(d: PermissionDecision) -> Option<Outcome> {
        (d.decision == Decision::Deny).then_some(Outcome::Deny(d.reason))
    }

    pub fn deny(reason: impl Into<String>) -> Option<Outcome> {
        Some(Outcome::Deny(reason.into()))
    }

    pub fn context(text: impl Into<String>) -> Option<Outcome> {
        Some(Outcome::Context(text.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_deny_becomes_an_outcome() {
        assert_eq!(
            Outcome::from_decision(PermissionDecision::deny("no")),
            Some(Outcome::Deny("no".into()))
        );
        assert_eq!(
            Outcome::from_decision(PermissionDecision::allow("ok")),
            None
        );
        assert_eq!(Outcome::from_decision(PermissionDecision::ask("?")), None);
    }
}
