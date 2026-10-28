//! Export deliberately contains aggregates only, never transcript identifiers.
use crate::{claude, codex, experiment::Experiment};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spread {
    pub n: usize,
    pub median: Option<u64>,
    pub p90: Option<u64>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Missing {
    pub to_pr_agent_ms: usize,
    pub to_pr_wall_ms: usize,
    pub default_turn_ms: usize,
    pub default_ttft_ms: usize,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arm {
    pub assignment: String,
    pub model: String,
    pub effort: String,
    pub plan_model: String,
    pub threads: usize,
    pub threads_with_pr: usize,
    pub sufficient: bool,
    pub aborted_turns: usize,
    pub to_pr_agent_ms: Spread,
    pub to_pr_wall_ms: Spread,
    pub to_pr_user_turns: Spread,
    pub default_turn_ms: Spread,
    pub default_ttft_ms: Spread,
    pub missing: Missing,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub manual_effort_change: usize,
    pub manual_model_change: usize,
    pub non_default_service_tier: usize,
    pub no_default_mode_turn: usize,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub days: u32,
    pub min_threads_with_pr: usize,
    pub threads_read: usize,
    pub unreadable_files: usize,
    pub excluded: Excluded,
    pub experiment: Option<Experiment>,
    pub arms: Vec<Arm>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observations {
    pub schema: u8,
    pub codex: Report,
    pub claude: Report,
}

pub fn collect(
    codex_home: &Path,
    claude_projects: &Path,
    days: u32,
    now: i64,
    experiment: Option<&Experiment>,
) -> Result<Observations> {
    let (codex_threads, codex_unreadable) = codex::collect(codex_home, days, now, experiment)?;
    let (claude_threads, claude_unreadable) = claude::collect(claude_projects, days, now)?;
    Ok(Observations {
        schema: 1,
        codex: serde_json::from_value(codex::report(
            &codex_threads,
            codex_unreadable,
            days,
            experiment,
        ))?,
        claude: serde_json::from_value(codex::report(
            &claude_threads,
            claude_unreadable,
            days,
            None,
        ))?,
    })
}
