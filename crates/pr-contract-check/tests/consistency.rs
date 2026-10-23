//! テンプレート間の整合性(かつての `rulesets-context-check --selftest` の `check_consistency`)。
//!
//! composite action への置換(ADR-591 D3)で workflow_call の連結名という概念が無くなった
//! ので、見るのは「repo-governance-common テンプレートの caller job name(= 全リポ共通の
//! PR title required context)が、3 つの seed の quality.json と、共通テンプレートと、dotfiles
//! 自身の `.github/rulesets/quality.json` の全部に含まれているか」だけ。

use pr_contract_check::gh;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `jobs:` より後ろの最初の `name:` の値(sed -n '/^jobs:/,$ { s/^ *name: *//p }' | head -1)。
fn caller_job_name(text: &str) -> String {
    let mut in_jobs = false;
    for line in text.lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if in_jobs {
            let t = line.trim_start_matches(' ');
            if let Some(rest) = t.strip_prefix("name:") {
                return rest.trim_start_matches(' ').to_owned();
            }
        }
    }
    String::new()
}

#[test]
fn the_pr_title_caller_context_is_in_every_quality_json() {
    let root = repo_root();
    let tpl = std::fs::read_to_string(root.join(
        "config/claude/skills/repo-governance-common/templates/.github/workflows/pr-title.yml",
    ))
    .unwrap();
    let caller = caller_job_name(&tpl);
    assert!(
        !caller.is_empty(),
        "テンプレートの caller job name を読めない"
    );

    for f in [
        "config/claude/skills/rust-repo-governance/templates/.github/rulesets/quality.json",
        "config/claude/skills/typst-repo-governance/templates/.github/rulesets/quality.json",
        "config/claude/skills/astro-site-governance/templates/.github/rulesets/quality.json",
        "config/claude/skills/repo-governance-common/templates/.github/rulesets/quality.json",
        ".github/rulesets/quality.json",
    ] {
        let text = std::fs::read_to_string(root.join(f)).unwrap();
        let contexts = gh::jq(
            &[
                "-r",
                "[.rules[]? | select(.type==\"required_status_checks\") | .parameters.required_status_checks[]?.context] | .[]",
            ],
            &text,
        )
        .unwrap_or_else(|| panic!("{f} を jq で読めない"));
        assert!(
            contexts.lines().any(|c| c == caller),
            "{f} does not have context \"{caller}\" (has: {contexts:?})"
        );
    }
}
