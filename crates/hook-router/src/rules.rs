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
//! Claude Code に任せる。ほかに、Stop・SessionStart の検査、ExitPlanMode の検査のうち
//! 拒否だけを出す 2 件(`plan-scope-gate`・`plan-precedent-gate`)以外、herdr、外部ツール
//! (bleep・atuin・copilot-plan-review)。検査ごとの理由は `docs/agents/hook-router.md`。

use agent_session::Agent;
use agent_session::Outcome;
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
    /// 未知のツールも含むすべての呼び出し。
    All,
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
            Tools::All => true,
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
    pub run: fn(Agent, &str) -> Option<Outcome>,
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
        run: |_, raw| git_stash_guard::hook(raw),
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
        run: |agent, raw| {
            let input = agent_session::HookInput::parse(raw)?;
            main_checkout_guard::pre_hook(agent, &input)
        },
    },
    Rule {
        node: "worktree-lease",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |_, raw| worktree_lease::hook(raw),
    },
    Rule {
        node: "attribution-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |agent, raw| attribution_guard::hook(agent, raw),
    },
    Rule {
        node: "worktree-create-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |_, raw| git_worktree_create_guard::hook(raw),
    },
    Rule {
        node: "stack-base-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S20,
        run: |agent, raw| stack_base_guard::hook(agent, raw),
    },
    Rule {
        node: "pr-title-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |agent, raw| pr_title_guard::hook(agent, raw),
    },
    Rule {
        node: "ci-wait-guard",
        event: Event::Pre,
        tools: Tools::Named(&["Bash", "Monitor"]),
        budget: S10,
        run: |agent, raw| ci_wait_guard::hook(agent, raw),
    },
    Rule {
        node: "sandbox-compound-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |agent, raw| sandbox_compound_guard::hook(agent, raw),
    },
    Rule {
        node: "pr-followup-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S10,
        run: |agent, raw| pr_followup_guard::hook(agent, raw),
    },
    Rule {
        node: "decision-colocation-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S20,
        run: |_, raw| decision_colocation::guard::hook(raw),
    },
    Rule {
        node: "external-send-guard",
        event: Event::Pre,
        tools: Tools::Mcp,
        budget: S10,
        run: |_, raw| external_send_guard::hook(raw),
    },
    Rule {
        node: "rulesets-write-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |_, raw| rulesets_write_guard::hook(raw),
    },
    Rule {
        node: "routines-write-guard",
        event: Event::Pre,
        tools: Tools::Named(&["RemoteTrigger"]),
        budget: S10,
        run: |_, raw| routines_write_guard::hook(raw),
    },
    Rule {
        node: "pkexec-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |agent, raw| pkexec_guard::hook(agent, raw),
    },
    Rule {
        node: "new-tool-guard",
        event: Event::Pre,
        tools: Tools::Named(&["Write"]),
        budget: S10,
        run: |_, raw| new_tool_guard::hook::hook(raw),
    },
    Rule {
        node: "feedback-target-guard",
        event: Event::Pre,
        tools: Tools::Bash,
        budget: S10,
        run: |_, raw| feedback_target_guard::hook(raw),
    },
    Rule {
        node: "repo-create-guard",
        event: Event::Pre,
        tools: Tools::BashOrMcp,
        budget: S15,
        run: |_, raw| repo_create_guard::hook(raw),
    },
    // ---- PreToolUse(ExitPlanMode の拒否。plan-view・plan-fresh-gate・copilot-plan-review は載せない)
    // 作業ディレクトリへ移らない `hook` 関数なので、並列に走らせても、ほかの規則に及ばない。
    // gh を呼ぶので、かつての `timeout`(20 秒)をそのまま許す。
    Rule {
        node: "plan-scope-gate",
        event: Event::Pre,
        tools: Tools::Named(&["ExitPlanMode"]),
        budget: S20,
        run: |_, raw| plan_scope_gate::hook(raw),
    },
    // 正規表現だけ。gh もネットワークも呼ばない(かつての `timeout` は 10 秒)。
    Rule {
        node: "plan-precedent-gate",
        event: Event::Pre,
        tools: Tools::Named(&["ExitPlanMode"]),
        budget: S10,
        run: |_, raw| plan_precedent_gate::hook(raw),
    },
    // ---- PostToolUse(記録役。出力は、あっても additionalContext だけ)
    Rule {
        node: "gh-edit-allow-record",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: |agent, raw| {
            (agent == Agent::Claude)
                .then(|| gh_edit_allow::record_hook(raw))
                .flatten()
        },
    },
    Rule {
        node: "cmd-hash-log",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: |_, raw| cmd_hash_log::hook(raw),
    },
    Rule {
        node: "adr-number",
        event: Event::Post,
        tools: Tools::Bash,
        budget: S10,
        run: adr_number::hook_for,
    },
    Rule {
        node: "new-tool-guard-record",
        event: Event::Post,
        tools: Tools::Named(&["ExitPlanMode"]),
        budget: S10,
        run: |_, raw| new_tool_guard::hook::hook(raw),
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
        assert!(Tools::All.matches("future_tool"));
        assert!(Tools::All.matches(""));
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
                "ci-wait-guard",
                "decision-colocation-guard",
                "external-send-guard",
                "feedback-target-guard",
                "git-stash-guard",
                "main-checkout-guard-pre",
                "new-tool-guard",
                "pkexec-guard",
                "plan-precedent-gate",
                "plan-scope-gate",
                "pr-followup-guard",
                "pr-title-guard",
                "repo-create-guard",
                "routines-write-guard",
                "rulesets-write-guard",
                "sandbox-compound-guard",
                "stack-base-guard",
                "worktree-lease",
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
        assert_eq!(
            nodes(Event::Pre, "ExitPlanMode"),
            ["plan-scope-gate", "plan-precedent-gate"]
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
