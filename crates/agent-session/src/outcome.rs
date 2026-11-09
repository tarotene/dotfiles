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
    /// 一回の本人承認を求める。恒久的な allow は追加しない。
    Ask(String),
    /// PreToolUse の限定的な allow。
    Allow(String),
    /// `additionalContext`(PreToolUse / PostToolUse のどちらかは、呼ばれたイベントで決まる)。
    Context(String),
}

use crate::decision::{Decision, PermissionDecision};

/// Stop hook の結果。「何も言わない」は `Option` の `None` で表す。ルータの [`Outcome`] とは別の型で、
/// ルータ(PreToolUse・PostToolUse だけ)には載せられない。pr-gate は CI を最大 300 秒待つので、
/// ルータの予算に収まらない(`docs/agents/hook-router.md` の条件 3)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// 終了させず、続けさせる。`reason` はモデルへ、`notice` は利用者向けの `systemMessage`。
    Block {
        reason: String,
        notice: Option<String>,
    },
    /// 止めない。利用者向けの注意書きだけ。
    Notice(String),
}

impl StopOutcome {
    pub fn block(reason: impl Into<String>) -> StopOutcome {
        StopOutcome::Block {
            reason: reason.into(),
            notice: None,
        }
    }

    pub fn block_with_notice(reason: impl Into<String>, notice: impl Into<String>) -> StopOutcome {
        StopOutcome::Block {
            reason: reason.into(),
            notice: Some(notice.into()),
        }
    }

    pub fn notice(text: impl Into<String>) -> StopOutcome {
        StopOutcome::Notice(text.into())
    }
}

/// 検査が `stop_hook_active`(直前の block で続けさせられたターンの再呼び出し)をどう扱うか。
/// 検査ごとに 1 つ宣言する。`stop_hook_active` の読み自体は [`crate::stop::StopInput`] だけが持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reentry {
    /// 立っていたら検査せず、黙る。
    Quiet,
    /// 見ない。block の直後の再呼び出しでも検査する。無限に block しないための上限は、検査自身が持つ
    /// (pr-gate・codex-plan-gate・レトロ。型では強制できない)。
    Recheck,
}

/// ブロックしない hook エラー(終了コード 0 と 2 は作れない)。終了コード 2 は Claude Code で「止める」の
/// 意味なので、block は [`StopOutcome::Block`] だけが運ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault(u8);

impl Fault {
    /// 処理に失敗した(終了コード 1)。
    pub const FAILED: Fault = Fault(1);
    /// hook の入力を読めなかった(終了コード 5)。
    pub const MALFORMED_INPUT: Fault = Fault(5);

    pub fn code(self) -> u8 {
        self.0
    }
}

/// Stop の検査の戻り値。`Ok(None)` は沈黙。
pub type StopResult = Result<Option<StopOutcome>, Fault>;

/// 判定を、そのまま検査の結果にする(allow・ask・deny のどれでも)。`from_decision` は、ルータに載せる
/// 検査のために deny だけを拾う別の関数で、こちらは全てを運ぶ。
impl From<PermissionDecision> for Outcome {
    fn from(d: PermissionDecision) -> Outcome {
        match d.decision {
            Decision::Deny => Outcome::Deny(d.reason),
            Decision::Ask => Outcome::Ask(d.reason),
            Decision::Allow => Outcome::Allow(d.reason),
        }
    }
}

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

    #[test]
    fn a_fault_can_never_be_a_block_or_a_pass() {
        for f in [Fault::FAILED, Fault::MALFORMED_INPUT] {
            assert!(f.code() != 0 && f.code() != 2, "{f:?}");
        }
    }

    #[test]
    fn stop_outcome_constructors() {
        assert_eq!(
            StopOutcome::block("r"),
            StopOutcome::Block {
                reason: "r".into(),
                notice: None
            }
        );
        assert_eq!(
            StopOutcome::block_with_notice("r", "n"),
            StopOutcome::Block {
                reason: "r".into(),
                notice: Some("n".into())
            }
        );
        assert_eq!(StopOutcome::notice("n"), StopOutcome::Notice("n".into()));
    }

    #[test]
    fn from_carries_every_decision() {
        assert_eq!(
            Outcome::from(PermissionDecision::deny("d")),
            Outcome::Deny("d".into())
        );
        assert_eq!(
            Outcome::from(PermissionDecision::ask("k")),
            Outcome::Ask("k".into())
        );
        assert_eq!(
            Outcome::from(PermissionDecision::allow("a")),
            Outcome::Allow("a".into())
        );
    }
}
