//! annotations ドメイン(ADR-568 Amendment 2026-10-23 の再発検出)。
//!
//! 既定ブランチで workflow ごとの最新の完了 run を読み、その全 job の
//! `warning` annotation を拾う。Node.js 20 の非推奨や、キャッシュの空振り
//! (設定とリポジトリの中身の食い違い)は、実行した run の annotation にしか
//! 現れず、workflow を静的に検査する lint では見えない。`failure` は拾わない
//! (赤は CI 自身が出す)。`notice` も拾わない。
//!
//! 明示指定でだけ走る([`crate::model::Domain::OPT_IN`])。取得は
//! [`crate::gh::Gh::fetch_warning_annotations`]、ここは純粋な判定だけ。

use crate::model::{Finding, RepoGql};
use std::collections::BTreeSet;

/// 1 job の warning annotation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobWarnings {
    pub workflow: String,
    pub job: String,
    pub messages: Vec<String>,
}

/// `Node.js 20 is deprecated. … forced to run on Node.js 24: <actions>. For more …`
/// から action の一覧(`name@ref`、ref は 12 文字まで)を取る。
fn node20_actions(message: &str) -> Option<String> {
    const MARK: &str = "run on Node.js 24: ";
    let rest = &message[message.find(MARK)? + MARK.len()..];
    let list = rest.split(". For more").next().unwrap_or(rest);
    let names: Vec<String> = list
        .split(',')
        .map(|a| {
            let a = a.trim();
            match a.split_once('@') {
                Some((n, r)) => format!("{n}@{}", r.chars().take(12).collect::<String>()),
                None => a.to_string(),
            }
        })
        .filter(|a| !a.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join("+"))
}

/// annotation の本文を、triage が機械的に引ける短い種類名にする。
/// 空白と `,` は `missing=` の区切りと衝突するので、最後に `-` / `;` に置き換える。
pub fn classify(message: &str) -> String {
    let m = message.trim();
    let kind = if m.starts_with("Node.js 20 is deprecated") {
        match node20_actions(m) {
            Some(a) => format!("node20-deprecated:{a}"),
            None => "node20-deprecated".to_string(),
        }
    } else if m.contains("Dependencies file is not found") || m.starts_with("No file matched to") {
        "cache-dependency-file-not-found".to_string()
    } else if m.starts_with("Failed to save") {
        "cache-save-failed".to_string()
    } else if m.starts_with("Failed to restore") || m.starts_with("Restore cache failed") {
        "cache-restore-failed".to_string()
    } else {
        format!("other:{}", m.chars().take(60).collect::<String>())
    };
    sanitize(&kind)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ',' => ';',
            c if c.is_whitespace() => '-',
            c => c,
        })
        .collect()
}

/// `jobs` は既定ブランチの、workflow ごとの最新の完了 run の全 job。
/// 空は「run が無い・取れなかった」で、warning が無いのと同じに ok へ倒す
/// (空データを drift に読まない)。
pub fn judge_annotations(gql: &RepoGql, jobs: &[JobWarnings]) -> Finding {
    if !gql.has_workflows() {
        return Finding::not_applicable();
    }
    let missing: BTreeSet<String> = jobs
        .iter()
        .flat_map(|j| {
            j.messages
                .iter()
                .map(|m| sanitize(&format!("{}@{}/{}", classify(m), j.workflow, j.job)))
        })
        .collect();
    Finding::ok_or_drifted(missing.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Verdict;
    use serde_json::json;

    fn gql_with_workflows() -> RepoGql {
        RepoGql::from_value(&json!({
            "workflowsDir": {"entries": [{"name": "ci.yml"}]}
        }))
    }

    fn jw(workflow: &str, job: &str, messages: &[&str]) -> JobWarnings {
        JobWarnings {
            workflow: workflow.into(),
            job: job.into(),
            messages: messages.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn classify_known_kinds() {
        assert_eq!(
            classify(
                "Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4, astral-sh/setup-uv@v5. For more information see: https://github.blog/x"
            ),
            "node20-deprecated:actions/checkout@v4+astral-sh/setup-uv@v5"
        );
        assert_eq!(
            classify("Restore cache failed: Dependencies file is not found in /x. Supported file pattern: go.mod"),
            "cache-dependency-file-not-found"
        );
        assert_eq!(
            classify("No file matched to [/x/**/*requirements*.txt]. The cache will never get invalidated."),
            "cache-dependency-file-not-found"
        );
        assert_eq!(
            classify("Failed to restore: Cache service responded with 400"),
            "cache-restore-failed"
        );
        assert_eq!(
            classify("Failed to save: <h2>Our services aren't available</h2>"),
            "cache-save-failed"
        );
    }

    #[test]
    fn classify_sanitizes_separators() {
        let k = classify("something odd, with spaces");
        assert!(!k.contains(' ') && !k.contains(','), "{k}");
        assert!(k.starts_with("other:"));
    }

    #[test]
    fn sha_pinned_refs_are_shortened() {
        let k = classify(
            "Node.js 20 is deprecated. … forced to run on Node.js 24: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1. For more information",
        );
        assert_eq!(k, "node20-deprecated:actions/checkout@3d3c42e5aac5");
    }

    #[test]
    fn no_workflows_is_not_applicable() {
        let f = judge_annotations(&RepoGql::default(), &[jw("CI", "x", &["w"])]);
        assert_eq!(f.verdict, Verdict::NotApplicable);
    }

    #[test]
    fn no_warnings_is_ok() {
        assert_eq!(
            judge_annotations(&gql_with_workflows(), &[]).verdict,
            Verdict::Ok
        );
        assert_eq!(
            judge_annotations(&gql_with_workflows(), &[jw("CI", "Lint", &[])]).verdict,
            Verdict::Ok
        );
    }

    #[test]
    fn warnings_are_drifted_sorted_and_deduped() {
        let f = judge_annotations(
            &gql_with_workflows(),
            &[
                jw("CI", "Lint", &["Failed to save: x", "Failed to save: y"]),
                jw("CI", "Build, test", &["Failed to restore: z"]),
            ],
        );
        assert_eq!(f.verdict, Verdict::Drifted);
        assert_eq!(
            f.missing,
            vec![
                "cache-restore-failed@CI/Build;-test".to_string(),
                "cache-save-failed@CI/Lint".to_string(),
            ]
        );
    }
}
