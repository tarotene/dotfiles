//! routines ドメイン(ADR-519)。
//!
//! `.claude/routines/*.json` を持つ repo が、週次の自己監査 routine の
//! (必然的に private な)`sources` に入っているか。入っていなければ auditor が
//! clone しないので宣言は黙って reconcile されない。宣言 ⇄ live の比較そのもの
//! は routines-plan.sh(auditor が回す)の仕事で、ここでは再導出しない
//! (titles の「存在検査 vs 適合検査」と同じ境界)。

use crate::model::{Finding, RepoGql};

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
