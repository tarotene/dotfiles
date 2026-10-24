//! ルータの規則の表。ツール呼び出しに付く検査は、ここに 1 行(1 ブロック)ずつ並ぶ。
//!
//! かつては検査ごとに `home/modules/claude.nix` へ別々の hook として登録し、`matcher`
//! (`Bash|mcp__.*` のような文字列)で絞っていた。`matcher` の書き間違いは、その検査が
//! 黙って素通りになる事故になる(`claude.nix` に実例の注意書きがあった)。ここでは、
//! どのツールに当てるかを [`Tools`] という閉じた型で書く: 書ける形が決まっていて、
//! 表に載せ忘れると pipeline-map のテスト(`cargo test`)が落ちる。
//!
//! 載せないもの(今の登録のまま): 許可(allow)を出す検査 — `git-worktree-allow` と
//! `gh-edit-allow` の判定役は、`if` の絞り込みが許可の範囲を決めるので、その評価を
//! Claude Code に任せる。ほかに、Stop・SessionStart・ExitPlanMode の検査、herdr、外部ツール
//! (bleep・atuin・copilot-plan-review)。

use hook_io::{Agent, Outcome};
use std::time::Duration;

/// ルータを呼ぶイベント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Pre,
    Post,
}

impl Event {
    /// Claude Code の `hook_event_name`。
    pub fn name(self) -> &'static str {
        match self {
            Event::Pre => "PreToolUse",
            Event::Post => "PostToolUse",
        }
    }
}

/// 規則が当たるツール。旧 `matcher` の閉じた表現。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// `Bash` だけ。
    Bash,
    /// `Bash` と、MCP のツール(`mcp__*`)。旧 `Bash|mcp__.*`。
    BashOrMcp,
    /// MCP のツール(`mcp__*`)だけ。旧 `mcp__.*`。
    Mcp,
    /// 名前の完全一致のどれか。
    Named(&'static [&'static str]),
}

impl Tools {
    pub fn matches(self, tool: &str) -> bool {
        match self {
            Tools::Bash => tool == "Bash",
            Tools::BashOrMcp => tool == "Bash" || tool.starts_with("mcp__"),
            Tools::Mcp => tool.starts_with("mcp__"),
            Tools::Named(names) => names.contains(&tool),
        }
    }
}

/// 検査 1 件。
#[derive(Clone, Copy)]
pub struct Rule {
    /// pipeline-map のノード id(`pipeline-map.toml`)。表に載せると、登録箇所として
    /// 数えられる(`crates/pipeline-map` の `Class::RouterRule`)。
    pub node: &'static str,
    pub event: Event,
    pub tools: Tools,
    /// この検査に許す時間。越えたら、その検査の結果は捨てて素通りにする(fail-open)。
    /// かつて個別の hook として宣言していた `timeout` と同じ値。
    pub budget: Duration,
    /// 判定の本体。引数は標準入力の JSON(生)。
    pub run: fn(&str) -> Option<Outcome>,
}

const S10: Duration = Duration::from_secs(10);
const S15: Duration = Duration::from_secs(15);
const S20: Duration = Duration::from_secs(20);

/// 規則の表。並びは、まとめた結果の理由の並びになる。
pub static RULES: &[Rule] = &[
    // ---- PreToolUse(拒否だけを出す検査)
    Rule {
        node: "git-stash-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: git_stash_guard::hook,
    },
    Rule {
        node: "main-checkout-guard-pre",
        event: Event::Pre,
        tools: Tools::Named(&[
            "Bash",
            "Write",
            "Edit",
            "MultiEdit",
            "NotebookEdit",
            "Read",
            "Grep",
            "Glob",
        ]),
        budget: S10,
        run: main_checkout_guard::hook,
    },
    Rule {
        node: "attribution-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |raw| attribution_guard::hook(Agent::Claude, raw),
    },
    Rule {
        node: "worktree-create-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: git_worktree_create_guard::hook,
    },
    Rule {
        node: "stack-base-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S20,
        run: |raw| stack_base_guard::hook(Agent::Claude, raw),
    },
    Rule {
        node: "pr-title-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |raw| pr_title_guard::hook(Agent::Claude, raw),
    },
    Rule {
        node: "pr-confirm-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |raw| pr_confirm_guard::hook(Agent::Claude, raw),
    },
    Rule {
        node: "decision-colocation-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S20,
        run: decision_colocation::guard::hook,
    },
    Rule {
        node: "external-send-guard",
        event: Event::Pre,
        tools: Tools::Mcp,
        budget: S10,
        run: external_send_guard::hook,
    },
    Rule {
        node: "rulesets-write-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: rulesets_write_guard::hook,
    },
    Rule {
        node: "routines-write-guard",
        event: Event::Pre,
        tools: Tools::Named(&["RemoteTrigger"]),
        budget: S10,
        run: routines_write_guard::hook,
    },
    Rule {
        node: "pkexec-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |raw| pkexec_guard::hook(Agent::Claude, raw),
    },
    Rule {
        node: "new-tool-guard",
        event: Event::Pre,
        tools: Tools::Named(&["Write"]),
        budget: S10,
        run: new_tool_guard::hook::hook,
    },
    Rule {
        node: "feedback-target-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: feedback_target_guard::hook,
    },
    Rule {
        node: "repo-create-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S15,
        run: repo_create_guard::hook,
    },
    // ---- PostToolUse(記録役。出力は、あっても additionalContext だけ)
    Rule {
        node: "gh-edit-allow-record",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: gh_edit_allow::record_hook,
    },
    Rule {
        node: "cmd-hash-log",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: cmd_hash_log::hook,
    },
    Rule {
        node: "adr-number",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: adr_number::hook,
    },
    Rule {
        node: "new-tool-guard-record",
        event: Event::Post,
        tools: Tools::Named(&["ExitPlanMode"]),
        budget: S10,
        run: new_tool_guard::hook::hook,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn tools_matchers_replace_the_old_matcher_strings() {
        // かつての matcher `Bash|mcp__.*`。
        assert!(Tools::BashOrMcp.matches("Bash"));
        assert!(Tools::BashOrMcp.matches("mcp__github__create_issue"));
        assert!(!Tools::BashOrMcp.matches("Write"));
        assert!(
            !Tools::BashOrMcp.matches("Bashful"),
            "完全一致で、前方一致ではない"
        );
        // かつての matcher `mcp__.*`。
        assert!(Tools::Mcp.matches("mcp__claude_ai_Gmail__send_message"));
        assert!(!Tools::Mcp.matches("Bash"));
        assert!(Tools::Bash.matches("Bash") && !Tools::Bash.matches("mcp__x"));
        assert!(Tools::Named(&["Write"]).matches("Write"));
        assert!(!Tools::Named(&["Write"]).matches("Edit"));
    }

    /// 載せた検査の集合を、このテストが固定する。足す・外すは、ここを意識して直す
    /// (載せ忘れは、pipeline-map のテストが「auto なのに登録箇所が無い」で落とす)。
    #[test]
    fn the_table_holds_exactly_the_agreed_rules() {
        let pre: BTreeSet<&str> = RULES
            .iter()
            .filter(|r| r.event == Event::Pre)
            .map(|r| r.node)
            .collect();
        assert_eq!(
            pre,
            BTreeSet::from([
                "attribution-guard",
                "decision-colocation-guard",
                "external-send-guard",
                "feedback-target-guard",
                "git-stash-guard",
                "main-checkout-guard-pre",
                "new-tool-guard",
                "pkexec-guard",
                "pr-confirm-guard",
                "pr-title-guard",
                "repo-create-guard",
                "routines-write-guard",
                "rulesets-write-guard",
                "stack-base-guard",
                "worktree-create-guard",
            ])
        );
        let post: BTreeSet<&str> = RULES
            .iter()
            .filter(|r| r.event == Event::Post)
            .map(|r| r.node)
            .collect();
        assert_eq!(
            post,
            BTreeSet::from([
                "adr-number",
                "cmd-hash-log",
                "gh-edit-allow-record",
                "new-tool-guard-record",
            ])
        );
    }

    #[test]
    fn nodes_are_unique_and_budgets_stay_within_the_old_declared_timeouts() {
        let mut seen = BTreeSet::new();
        for r in RULES {
            assert!(seen.insert(r.node), "node が重複: {}", r.node);
            assert!(
                r.budget >= Duration::from_secs(10) && r.budget <= Duration::from_secs(20),
                "{}: budget {:?}(旧 timeout は 10〜20 秒)",
                r.node,
                r.budget
            );
        }
    }

    #[test]
    fn applicable_picks_by_event_and_tool() {
        let nodes = |e, t| {
            crate::applicable(e, t)
                .into_iter()
                .map(|r| r.node)
                .collect::<Vec<_>>()
        };
        let bash = nodes(Event::Pre, "Bash");
        assert!(bash.contains(&"git-stash-guard") && bash.contains(&"stack-base-guard"));
        assert!(
            !bash.contains(&"external-send-guard"),
            "Bash には MCP 専用の検査は当たらない"
        );
        assert_eq!(nodes(Event::Pre, "Read"), ["main-checkout-guard-pre"]);
        assert_eq!(nodes(Event::Pre, "RemoteTrigger"), ["routines-write-guard"]);
        assert!(
            nodes(Event::Pre, "ExitPlanMode").is_empty(),
            "ExitPlanMode の gate は載せていない"
        );
        assert_eq!(
            nodes(Event::Post, "ExitPlanMode"),
            ["new-tool-guard-record"]
        );
        let mcp = nodes(Event::Pre, "mcp__x__y");
        assert!(mcp.contains(&"external-send-guard") && mcp.contains(&"attribution-guard"));
        assert!(!mcp.contains(&"git-stash-guard"));
    }
}
