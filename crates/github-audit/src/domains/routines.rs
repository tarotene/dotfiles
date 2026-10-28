//! routines ドメイン(ADR-519)。
//!
//! `.claude/routines/*.json` を持つ repo が、週次の自己監査 routine の
//! (必然的に private な)`sources` に入っているか。入っていなければ auditor が
//! clone しないので宣言は黙って reconcile されない。宣言 ⇄ live の比較そのもの
//! は routines-plan.sh(auditor が回す)の仕事で、ここでは再導出しない
//! (titles の「存在検査 vs 適合検査」と同じ境界)。

use crate::model::{Finding, RepoGql, Verdict};

/// bash: `judge_routines`。`sources` は `owner/repo` の一覧。
pub fn judge_routines(owner: &str, repo: &str, gql: &RepoGql, sources: &[String]) -> Finding {
    let has_declarations = gql
        .routines_dir
        .as_ref()
        .and_then(|t| t.entries.as_deref())
        .unwrap_or(&[])
        .iter()
        .any(|e| e.name.as_deref().is_some_and(|n| n.ends_with(".json")));
    if !has_declarations {
        return Finding::not_applicable();
    }
    let full = format!("{owner}/{repo}");
    if sources.contains(&full) {
        Finding::ok_or_drifted(Vec::new())
    } else {
        Finding::ok_or_drifted(vec!["routines-sources-missing".into()])
    }
}

pub fn judge_routines_installation(
    owner: &str,
    repo: &str,
    gql: &RepoGql,
    sources: &[String],
    private: bool,
    installs: &Result<Vec<String>, &'static str>,
) -> Finding {
    let mut finding = judge_routines(owner, repo, gql, sources);
    if !private {
        return finding;
    }
    if finding.verdict == Verdict::NotApplicable {
        if !sources.contains(&format!("{owner}/{repo}")) {
            return finding;
        }
        finding = Finding::ok_or_drifted(Vec::new());
    }
    match installs {
        Ok(repos) if !repos.contains(&format!("{owner}/{repo}")) => {
            finding
                .missing
                .push("claude-app-installation-missing".into());
            finding.verdict = Verdict::Drifted;
        }
        Err(state) => {
            finding
                .missing
                .push(format!("claude-app-installation-{state}"));
            if finding.verdict == Verdict::Ok {
                finding.verdict = Verdict::Advisory;
            }
        }
        _ => {}
    }
    finding
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_routines_distinguish_missing_from_unobserved() {
        let gql: RepoGql = serde_json::from_str(
            r#"{"routinesDir":{"entries":[{"name":"weekly.json","type":"blob"}]}}"#,
        )
        .unwrap();
        let sources = vec!["owner/repo".to_owned()];
        let judge = |obs| judge_routines_installation("owner", "repo", &gql, &sources, true, &obs);
        assert_eq!(judge(Ok(vec!["owner/repo".into()])).verdict, Verdict::Ok);
        let missing = judge(Ok(vec![]));
        assert_eq!(missing.verdict, Verdict::Drifted);
        assert!(missing.has("claude-app-installation-missing"));
        for state in ["unobserved", "error"] {
            let unknown = judge(Err(state));
            assert_eq!(unknown.verdict, Verdict::Advisory);
            assert!(unknown.has(&format!("claude-app-installation-{state}")));
        }
        assert_eq!(
            judge_routines_installation("owner", "repo", &gql, &sources, false, &Err("error"))
                .verdict,
            Verdict::Ok
        );
    }
}
