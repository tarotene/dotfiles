//! hook-router — ツール呼び出しに付く検査を、1 つのプロセスで、規則の表に従って走らせる。
//!
//! ```text
//! hook-router pre    # PreToolUse(拒否だけを出す検査)
//! hook-router post   # PostToolUse(記録役)
//! ```
//!
//! `~/.claude/settings.json` には、この 2 つだけを登録する(`matcher` なし)。どのツールにどの
//! 検査を当てるかは、[`rules::RULES`] の表([`rules::Tools`] という閉じた型)が持つ。
//! 設計と、載せなかったものの理由は [`rules`] と `docs/claude/hook-router.md`。

pub mod codex;
pub mod router;
pub mod rules;

pub use router::{execute, execute_for, merge, render, Merged, Verdict};
pub use rules::{Event, Rule, Tools, RULES};

/// イベントとツール名に当たる規則。
pub fn applicable(event: Event, tool: &str) -> Vec<Rule> {
    RULES
        .iter()
        .filter(|r| r.event == event && r.tools.matches(tool))
        .copied()
        .collect()
}

/// 標準入力の JSON から `tool_name` を読む。読めなければ `None`(判定しない)。
pub fn tool_name(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let name = v.get("tool_name")?.as_str()?;
    (!name.is_empty()).then(|| name.to_owned())
}
