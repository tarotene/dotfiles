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
        "config/agents/skills/repo-governance-common/templates/.github/workflows/pr-title.yml",
    ))
    .unwrap();
    let caller = caller_job_name(&tpl);
    assert!(
        !caller.is_empty(),
        "テンプレートの caller job name を読めない"
    );

    for f in [
        "config/agents/skills/rust-repo-governance/templates/.github/rulesets/quality.json",
        "config/agents/skills/typst-repo-governance/templates/.github/rulesets/quality.json",
        "config/agents/skills/astro-site-governance/templates/.github/rulesets/quality.json",
        "config/agents/skills/repo-governance-common/templates/.github/rulesets/quality.json",
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

/// 言語別テンプレートの `ci.yml` が、`workflow-naming-check` と github-audit の共有の判定
/// (`judge_workflows`)を通ること。テンプレートを播いた直後のリポジトリが、最初の PR で
/// `ci-passed-needs-incomplete:*` や `ci-passed-failure-step-missing` で落ちないため
/// (ADR-1005 D6・D8 が `needs:` の網羅と `ci-passed` の形を要求する)。
#[test]
fn every_ci_yml_template_passes_the_shared_workflow_judge() {
    use pr_contract_check::workflows::{judge_workflows, Inputs};
    let root = repo_root();
    let files = vec!["ci.yml".to_string(), "pr-title.yml".to_string()];
    for skill in [
        "repo-governance-common",
        "rust-repo-governance",
        "typst-repo-governance",
        "astro-site-governance",
    ] {
        let path = root.join(format!(
            "config/agents/skills/{skill}/templates/.github/workflows/ci.yml"
        ));
        let text = std::fs::read_to_string(&path).unwrap();
        let missing = judge_workflows(
            &Inputs {
                file_names: &files,
                ci_text: &text,
                pr_title_text: "",
                quality_text: "",
                canonical_quality_text: None,
            },
            &|_| None,
        );
        assert!(
            missing.is_empty(),
            "{skill} の ci.yml テンプレート: {missing:?}"
        );
    }
}

/// `rulesets-context-check` が使う静的な突き合わせ(`job_names`)が、実在の宣言と実在の
/// workflow(dotfiles 自身と、各 governance skill のテンプレート)に対して、宣言の required
/// context を全て見つけること。`PR title` は、この検査を走らせている job 自身の名前で、
/// 別 workflow(`pr-title.yml`)にある — `ci.yml` だけを読むと正常な PR でも必ず落ちる(#1014)。
#[test]
fn declared_contexts_are_found_statically_in_every_real_workflow_set() {
    use pr_contract_check::job_names;
    let root = repo_root();
    let mut sets: Vec<(String, PathBuf)> = vec![("dotfiles".into(), root.join(".github"))];
    for skill in [
        "repo-governance-common",
        "rust-repo-governance",
        "typst-repo-governance",
        "astro-site-governance",
    ] {
        sets.push((
            skill.into(),
            root.join(format!("config/agents/skills/{skill}/templates/.github")),
        ));
    }
    for (label, dot_github) in sets {
        let mut jobs = job_names::JobNames::new();
        let mut files = 0;
        for entry in std::fs::read_dir(dot_github.join("workflows")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".yml") || name.ends_with(".yaml") {
                jobs.extend(job_names::parse(
                    &name,
                    &std::fs::read_to_string(&path).unwrap(),
                ));
                files += 1;
            }
        }
        assert!(files > 0, "{label}: workflow が 1 つも無い");
        assert!(
            !jobs.has_unresolved(),
            "{label}: 読み切れない workflow がある: {:?}",
            jobs.unresolved_files().collect::<Vec<_>>()
        );
        let text = std::fs::read_to_string(dot_github.join("rulesets/quality.json")).unwrap();
        let contexts = gh::jq(
            &[
                "-r",
                "[.rules[]? | select(.type==\"required_status_checks\") | .parameters.required_status_checks[]?.context] | .[]",
            ],
            &text,
        )
        .unwrap_or_else(|| panic!("{label}: quality.json を jq で読めない"));
        assert!(contexts.lines().count() > 0, "{label}: context が無い");
        for c in contexts.lines() {
            assert!(
                jobs.matches(c),
                "{label}: required context {c:?} が workflow の job 名として読めない(読めた名前: {:?})",
                jobs.exact().collect::<Vec<_>>()
            );
        }
    }
    // dotfiles 自身: 検査を走らせている job の名前(`PR title`)が、一覧に入っている。
    let pr_title = std::fs::read_to_string(root.join(".github/workflows/pr-title.yml")).unwrap();
    assert!(job_names::parse("pr-title.yml", &pr_title).matches("PR title"));
}
