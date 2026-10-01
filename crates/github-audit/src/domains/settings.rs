//! settings ドメイン(リポジトリ設定 9 項目、ADR-0031、ADR-568 D2/D5b)。

use crate::model::{Finding, RepoMeta, RepoRest};

/// bash: `judge_settings`。`meta` は `gh repo list` の GraphQL 由来の値、
/// `rest` は `gh api repos/O/R` の REST 由来の値(取得失敗なら全項目 `None`
/// — REST 由来の 4 項目はすべて drift 側に倒れる。黙って compliant にしない)。
pub fn judge_settings(meta: &RepoMeta, rest: &RepoRest) -> Finding {
    let mut missing: Vec<String> = Vec::new();
    if !(meta.squash_merge_allowed == Some(true)
        && meta.merge_commit_allowed == Some(false)
        && meta.rebase_merge_allowed == Some(false))
    {
        missing.push("merge-not-squash-only".into());
    }
    if meta.delete_branch_on_merge != Some(true) {
        missing.push("delete-branch-on-merge-disabled".into());
    }
    let default_branch = meta
        .default_branch_ref
        .as_ref()
        .and_then(|r| r.name.as_deref())
        .unwrap_or("");
    if default_branch != "main" {
        missing.push("default-branch-not-main".into());
    }
    if meta.has_wiki_enabled != Some(false) {
        missing.push("wiki-enabled".into());
    }
    if meta.has_projects_enabled != Some(false) {
        missing.push("projects-enabled".into());
    }
    // ADR-0031: 「PR タイトル = main の squash commit subject」という契約は
    // この 2 値の上に成立する(*-repo-governance の apply-repo-settings.sh と
    // 同じ宣言値)。
    if rest.squash_merge_commit_title.as_deref() != Some("PR_TITLE") {
        missing.push("squash-title-not-pr-title".into());
    }
    if rest.squash_merge_commit_message.as_deref() != Some("BLANK") {
        missing.push("squash-message-not-blank".into());
    }
    // ADR-568 D2/D5b: automerge の安全性は GitHub-native auto-merge が前提で、
    // fix-PR の経路は Renovate だけ(Dependabot security updates は off)。
    if rest.allow_auto_merge != Some(true) {
        missing.push("auto-merge-disabled".into());
    }
    if rest.dependabot_security_updates_status.as_deref() != Some("disabled") {
        missing.push("dependabot-security-updates-enabled".into());
    }
    Finding::ok_or_drifted(missing)
}
