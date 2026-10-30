//! リポジトリの merge 設定の判定(ADR-0031、ADR-568 D2/D5b)。
//!
//! 判定そのものは入力が素の値だけの純関数。`scripts/github-audit` の
//! `judge_settings` と、それを `source` していた `scripts/pr-merge-settings-check` の
//! 単一正本で、`crates/github-audit` の settings ドメインも同じ関数を使う
//! (ADR-0035 D1「単一正本 > 複写+同期」)。

/// 「非公開かつ無料プランのため、自動マージと rulesets が使えない」ことを
/// 宣言するトピック(docs/agents/free-plan-declaration.md)。
pub const PLAN_FREE_PRIVATE_TOPIC: &str = "plan-free-private";

/// 判定に使うフィールドだけ。取得できなかった値は `None`(drift 側に倒れる —
/// 黙って compliant にしない)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub allow_squash_merge: Option<bool>,
    pub allow_merge_commit: Option<bool>,
    pub allow_rebase_merge: Option<bool>,
    pub delete_branch_on_merge: Option<bool>,
    pub default_branch: Option<String>,
    pub has_wiki: Option<bool>,
    pub has_projects: Option<bool>,
    pub squash_merge_commit_title: Option<String>,
    pub squash_merge_commit_message: Option<String>,
    pub allow_auto_merge: Option<bool>,
    pub private: Option<bool>,
    pub topics: Vec<String>,
    /// `.security_and_analysis.dependabot_security_updates.status`。
    pub dependabot_security_updates_status: Option<String>,
}

impl Settings {
    pub fn plan_free_private_declared(&self) -> bool {
        self.topics.iter().any(|t| t == PLAN_FREE_PRIVATE_TOPIC)
    }

    /// `gh api repos/O/R --jq` に渡す射影([`GH_PROJECTION`])の出力 1 行(タブ区切り)を読む。
    pub fn from_tsv(line: &str) -> Option<Settings> {
        let f: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
        if f.len() != 13 {
            return None;
        }
        let b = |s: &str| match s {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        let s = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        Some(Settings {
            allow_squash_merge: b(f[0]),
            allow_merge_commit: b(f[1]),
            allow_rebase_merge: b(f[2]),
            delete_branch_on_merge: b(f[3]),
            default_branch: s(f[4]),
            has_wiki: b(f[5]),
            has_projects: b(f[6]),
            squash_merge_commit_title: s(f[7]),
            squash_merge_commit_message: s(f[8]),
            allow_auto_merge: b(f[9]),
            private: b(f[10]),
            topics: f[11]
                .split(',')
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
            dependabot_security_updates_status: s(f[12]),
        })
    }
}

/// [`Settings::from_tsv`] が読む列の順に並べた、`gh api repos/O/R --jq` の射影。
/// `@tsv` は null を空、真偽を `true` / `false` で出す。
pub const GH_PROJECTION: &str = "[.allow_squash_merge, .allow_merge_commit, \
.allow_rebase_merge, .delete_branch_on_merge, .default_branch, .has_wiki, .has_projects, \
.squash_merge_commit_title, .squash_merge_commit_message, .allow_auto_merge, .private, \
((.topics // []) | join(\",\")), \
(.security_and_analysis.dependabot_security_updates.status // null)] | @tsv";

/// drift の code(`missing`)。空なら適合。
pub fn judge_settings(s: &Settings) -> Vec<&'static str> {
    let mut missing: Vec<&'static str> = Vec::new();
    if !(s.allow_squash_merge == Some(true)
        && s.allow_merge_commit == Some(false)
        && s.allow_rebase_merge == Some(false))
    {
        missing.push("merge-not-squash-only");
    }
    if s.delete_branch_on_merge != Some(true) {
        missing.push("delete-branch-on-merge-disabled");
    }
    if s.default_branch.as_deref().unwrap_or("") != "main" {
        missing.push("default-branch-not-main");
    }
    if s.has_wiki != Some(false) {
        missing.push("wiki-enabled");
    }
    if s.has_projects != Some(false) {
        missing.push("projects-enabled");
    }
    // ADR-0031: 「PR タイトル = main の squash commit subject」という契約は
    // この 2 値の上に成立する(repo-governance の apply-repo-settings.sh と
    // 同じ宣言値)。
    if s.squash_merge_commit_title.as_deref() != Some("PR_TITLE") {
        missing.push("squash-title-not-pr-title");
    }
    if s.squash_merge_commit_message.as_deref() != Some("BLANK") {
        missing.push("squash-message-not-blank");
    }
    // ADR-568 D2/D5b: automerge の安全性は GitHub-native auto-merge が前提で、
    // fix-PR の経路は Renovate だけ(Dependabot security updates は off)。
    //
    // ただし `plan-free-private` が宣言された(非公開かつ無料プランで、自動マージが
    // 使えない)リポジトリでは、この期待を外す。宣言が実態と食い違った(公開に
    // なった、または自動マージが有効になった)ときは `plan-free-private-stale` を
    // 出し、トピックの外し忘れを必ず検出する(docs/agents/free-plan-declaration.md)。
    if s.plan_free_private_declared() {
        if s.private == Some(false) || s.allow_auto_merge == Some(true) {
            missing.push("plan-free-private-stale");
        }
    } else if s.allow_auto_merge != Some(true) {
        missing.push("auto-merge-disabled");
    }
    if s.dependabot_security_updates_status.as_deref() != Some("disabled") {
        missing.push("dependabot-security-updates-enabled");
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_ok() -> Settings {
        Settings {
            allow_squash_merge: Some(true),
            allow_merge_commit: Some(false),
            allow_rebase_merge: Some(false),
            delete_branch_on_merge: Some(true),
            default_branch: Some("main".into()),
            has_wiki: Some(false),
            has_projects: Some(false),
            squash_merge_commit_title: Some("PR_TITLE".into()),
            squash_merge_commit_message: Some("BLANK".into()),
            allow_auto_merge: Some(true),
            private: Some(false),
            topics: vec![],
            dependabot_security_updates_status: Some("disabled".into()),
        }
    }

    #[test]
    fn compliant_has_no_missing() {
        assert!(judge_settings(&all_ok()).is_empty());
    }

    #[test]
    fn nothing_fetched_drifts_on_every_item() {
        let m = judge_settings(&Settings::default());
        assert_eq!(
            m,
            [
                "merge-not-squash-only",
                "delete-branch-on-merge-disabled",
                "default-branch-not-main",
                "wiki-enabled",
                "projects-enabled",
                "squash-title-not-pr-title",
                "squash-message-not-blank",
                "auto-merge-disabled",
                "dependabot-security-updates-enabled",
            ]
        );
    }

    #[test]
    fn plan_free_private_declaration_changes_the_auto_merge_expectation() {
        let free = Settings {
            private: Some(true),
            allow_auto_merge: Some(false),
            topics: vec![PLAN_FREE_PRIVATE_TOPIC.into()],
            ..all_ok()
        };
        assert!(judge_settings(&free).is_empty());
        let stale = Settings {
            allow_auto_merge: Some(true),
            ..free.clone()
        };
        assert_eq!(judge_settings(&stale), ["plan-free-private-stale"]);
        let now_public = Settings {
            private: Some(false),
            ..free
        };
        assert_eq!(judge_settings(&now_public), ["plan-free-private-stale"]);
    }

    #[test]
    fn tsv_roundtrip_of_the_projection_order() {
        let line = "true\tfalse\tfalse\ttrue\tmain\tfalse\tfalse\tPR_TITLE\tBLANK\ttrue\ttrue\ta,plan-free-private\tdisabled";
        let s = Settings::from_tsv(line).unwrap();
        assert_eq!(s.allow_squash_merge, Some(true));
        assert_eq!(s.default_branch.as_deref(), Some("main"));
        assert_eq!(s.topics, ["a", "plan-free-private"]);
        assert!(s.plan_free_private_declared());
        assert_eq!(
            s.dependabot_security_updates_status.as_deref(),
            Some("disabled")
        );
        // null は空になり、None になる。
        let s = Settings::from_tsv("\t\t\t\t\t\t\t\t\t\t\t\t").unwrap();
        assert_eq!(s, Settings::default());
        assert!(Settings::from_tsv("too\tfew").is_none());
    }
}
