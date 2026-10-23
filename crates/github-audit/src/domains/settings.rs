//! settings ドメイン(リポジトリ設定 9 項目、ADR-0031、ADR-568 D2/D5b)。
//!
//! 判定そのものは `crates/pr-contract-check` の [`settings::judge_settings`] が単一正本
//! (他リポジトリの required check `pr-merge-settings-check` も同じ関数を使う、
//! ADR-0035 D1「単一正本 > 複写+同期」)。ここは `RepoMeta` / `RepoRest` からその入力を
//! 組み立てる。

use crate::model::{Finding, RepoMeta, RepoRest};
use pr_contract_check::settings::{self, Settings};

/// bash: `judge_settings`。`meta` は `gh repo list` の GraphQL 由来の値、
/// `rest` は `gh api repos/O/R` の REST 由来の値(取得失敗なら全項目 `None`
/// — REST 由来の 4 項目はすべて drift 側に倒れる。黙って compliant にしない)。
pub fn judge_settings(meta: &RepoMeta, rest: &RepoRest) -> Finding {
    let input = Settings {
        allow_squash_merge: meta.squash_merge_allowed,
        allow_merge_commit: meta.merge_commit_allowed,
        allow_rebase_merge: meta.rebase_merge_allowed,
        delete_branch_on_merge: meta.delete_branch_on_merge,
        default_branch: meta
            .default_branch_ref
            .as_ref()
            .and_then(|r| r.name.clone()),
        has_wiki: meta.has_wiki_enabled,
        has_projects: meta.has_projects_enabled,
        squash_merge_commit_title: rest.squash_merge_commit_title.clone(),
        squash_merge_commit_message: rest.squash_merge_commit_message.clone(),
        allow_auto_merge: rest.allow_auto_merge,
        private: rest.private,
        topics: rest.topics.clone(),
        dependabot_security_updates_status: rest.dependabot_security_updates_status.clone(),
    };
    Finding::ok_or_drifted(
        settings::judge_settings(&input)
            .into_iter()
            .map(String::from)
            .collect(),
    )
}
