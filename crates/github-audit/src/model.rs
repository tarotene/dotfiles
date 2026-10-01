//! 判定の入出力の型(#414)。
//!
//! bash 版は judge_* の入出力をすべて jq で組み立てた JSON 文字列で受け渡して
//! いた。ここではそれを serde の型にする:
//!
//! - 入力: [`RepoMeta`](`gh repo list --json` の 1 要素)、[`RepoGql`]
//!   (GraphQL バッチの 1 リポジトリ分)、[`RepoRest`](`gh api repos/O/R`)
//! - 出力: [`Finding`] (ドメインごとの判定)、[`RepoFindings`](1 リポジトリ分)
//!
//! 出力 JSON のキー順・形は bash 版とバイト単位で一致させる([`Finding::to_j`])。
//! `github-audit --json` や ledger を読む側(github-audit-triage スキル等)が
//! そのまま読めるようにするため。

use crate::jq;
use hook_io::jqfmt::J;
use serde::Deserialize;
use serde_json::Value;

/// 監査ドメイン。並びは bash 版 `ALL_DOMAINS` の順(既定の実行順・出力順)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    Rulesets,
    Charters,
    Naming,
    Settings,
    Renovate,
    Titles,
    Lifecycle,
    Releaser,
    Routines,
    Workflows,
    Docs,
}

impl Domain {
    pub const ALL: [Domain; 11] = [
        Domain::Rulesets,
        Domain::Charters,
        Domain::Naming,
        Domain::Settings,
        Domain::Renovate,
        Domain::Titles,
        Domain::Lifecycle,
        Domain::Releaser,
        Domain::Routines,
        Domain::Workflows,
        Domain::Docs,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Domain::Rulesets => "rulesets",
            Domain::Charters => "charters",
            Domain::Naming => "naming",
            Domain::Settings => "settings",
            Domain::Renovate => "renovate",
            Domain::Titles => "titles",
            Domain::Lifecycle => "lifecycle",
            Domain::Releaser => "releaser",
            Domain::Routines => "routines",
            Domain::Workflows => "workflows",
            Domain::Docs => "docs",
        }
    }

    pub fn parse(s: &str) -> Option<Domain> {
        Domain::ALL.into_iter().find(|d| d.as_str() == s)
    }
}

/// 判定。`DormancyCandidate`(lifecycle)と `Advisory`(releaser)は
/// 候補提示・情報級で、[`crate::any_drift`] の失敗判定から除外される。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Drifted,
    Ungoverned,
    Exempt,
    NotApplicable,
    DormancyCandidate,
    Advisory,
}

impl Verdict {
    /// human report の集計行の並び(bash 版 render_report の for ループ順)。
    pub const REPORT_ORDER: [Verdict; 7] = [
        Verdict::Ok,
        Verdict::Drifted,
        Verdict::Ungoverned,
        Verdict::Exempt,
        Verdict::NotApplicable,
        Verdict::DormancyCandidate,
        Verdict::Advisory,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Drifted => "drifted",
            Verdict::Ungoverned => "ungoverned",
            Verdict::Exempt => "exempt",
            Verdict::NotApplicable => "not-applicable",
            Verdict::DormancyCandidate => "dormancy-candidate",
            Verdict::Advisory => "advisory",
        }
    }

    pub fn parse(s: &str) -> Option<Verdict> {
        Verdict::REPORT_ORDER.into_iter().find(|v| v.as_str() == s)
    }
}

/// rulesets ドメインの review layer(ADR-0021)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewLayer {
    Absent,
    Complete,
    PartialDrift,
}

impl ReviewLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewLayer::Absent => "absent",
            ReviewLayer::Complete => "complete",
            ReviewLayer::PartialDrift => "partial-drift",
        }
    }
}

/// rulesets ドメインの宣言(`.github/rulesets/*.json`、ADR-503)の有無。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declaration {
    Present,
    Missing,
    NotJudged,
}

impl Declaration {
    pub fn as_str(self) -> &'static str {
        match self {
            Declaration::Present => "present",
            Declaration::Missing => "missing",
            Declaration::NotJudged => "not-judged",
        }
    }
}

/// ドメイン固有の付帯フィールド。
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// `{verdict, missing}` だけのドメイン。
    Plain,
    Rulesets {
        status_checks: Vec<String>,
        review_layer: ReviewLayer,
        declaration: Declaration,
    },
    Naming {
        class: String,
        lifecycle_candidates: Vec<String>,
    },
    Lifecycle {
        score: i64,
    },
}

/// 1 ドメインの判定(bash 版 judge_* が出していた JSON オブジェクト)。
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub verdict: Verdict,
    pub missing: Vec<String>,
    pub detail: Detail,
}

impl Finding {
    pub fn plain(verdict: Verdict, missing: Vec<String>) -> Finding {
        Finding {
            verdict,
            missing,
            detail: Detail::Plain,
        }
    }

    /// missing が空なら ok、そうでなければ drifted(大半の judge_* の末尾の形)。
    pub fn ok_or_drifted(missing: Vec<String>) -> Finding {
        let verdict = if missing.is_empty() {
            Verdict::Ok
        } else {
            Verdict::Drifted
        };
        Finding::plain(verdict, missing)
    }

    pub fn not_applicable() -> Finding {
        Finding::plain(Verdict::NotApplicable, Vec::new())
    }

    pub fn exempt() -> Finding {
        Finding::plain(Verdict::Exempt, Vec::new())
    }

    pub fn has(&self, code: &str) -> bool {
        self.missing.iter().any(|m| m == code)
    }

    /// rulesets の `review_layer`(他ドメインは `None`)。
    pub fn review_layer(&self) -> Option<ReviewLayer> {
        match &self.detail {
            Detail::Rulesets { review_layer, .. } => Some(*review_layer),
            _ => None,
        }
    }

    /// bash 版と同じキー順の JSON 値。
    pub fn to_j(&self) -> J {
        let strs = |v: &[String]| J::Arr(v.iter().map(|s| J::str(s.as_str())).collect());
        let mut pairs: Vec<(&str, J)> = vec![
            ("verdict", J::str(self.verdict.as_str())),
            ("missing", strs(&self.missing)),
        ];
        match &self.detail {
            Detail::Plain => {}
            Detail::Rulesets {
                status_checks,
                review_layer,
                declaration,
            } => {
                pairs.push(("status_checks", strs(status_checks)));
                pairs.push(("review_layer", J::str(review_layer.as_str())));
                pairs.push(("declaration", J::str(declaration.as_str())));
            }
            Detail::Naming {
                class,
                lifecycle_candidates,
            } => {
                pairs.push(("class", J::str(class.as_str())));
                pairs.push(("lifecycle_candidates", strs(lifecycle_candidates)));
            }
            Detail::Lifecycle { score } => pairs.push(("score", J::Num(score.to_string()))),
        }
        J::obj(pairs)
    }

    /// `to_j` の逆(ledger や `--json` 出力を読み戻す)。形が合わなければ `None`。
    pub fn from_value(v: &Value) -> Option<Finding> {
        let verdict = Verdict::parse(v.get("verdict")?.as_str()?)?;
        let missing = jq::string_array(v.get("missing").unwrap_or(&Value::Null));
        let detail = if let Some(rl) = v.get("review_layer").and_then(Value::as_str) {
            let review_layer = match rl {
                "complete" => ReviewLayer::Complete,
                "partial-drift" => ReviewLayer::PartialDrift,
                _ => ReviewLayer::Absent,
            };
            let declaration = match v.get("declaration").and_then(Value::as_str) {
                Some("present") => Declaration::Present,
                Some("missing") => Declaration::Missing,
                _ => Declaration::NotJudged,
            };
            Detail::Rulesets {
                status_checks: jq::string_array(v.get("status_checks").unwrap_or(&Value::Null)),
                review_layer,
                declaration,
            }
        } else if let Some(class) = v.get("class").and_then(Value::as_str) {
            Detail::Naming {
                class: class.to_string(),
                lifecycle_candidates: jq::string_array(
                    v.get("lifecycle_candidates").unwrap_or(&Value::Null),
                ),
            }
        } else if let Some(score) = v.get("score").and_then(Value::as_i64) {
            Detail::Lifecycle { score }
        } else {
            Detail::Plain
        };
        Some(Finding {
            verdict,
            missing,
            detail,
        })
    }
}

/// 1 リポジトリ分の判定(`--json` 出力配列の 1 要素)。
#[derive(Debug, Clone, PartialEq)]
pub struct RepoFindings {
    pub repo: String,
    pub language: String,
    pub visibility: String,
    /// 実行したドメインの順(`--json` の `domains` オブジェクトのキー順)。
    pub domains: Vec<(Domain, Finding)>,
}

impl RepoFindings {
    pub fn domain(&self, d: Domain) -> Option<&Finding> {
        self.domains.iter().find(|(k, _)| *k == d).map(|(_, f)| f)
    }

    pub fn to_j(&self) -> J {
        J::obj(vec![
            ("repo", J::str(self.repo.as_str())),
            ("language", J::str(self.language.as_str())),
            ("visibility", J::str(self.visibility.as_str())),
            (
                "domains",
                J::Obj(
                    self.domains
                        .iter()
                        .map(|(d, f)| (d.as_str().to_string(), f.to_j()))
                        .collect(),
                ),
            ),
        ])
    }

    pub fn from_value(v: &Value) -> Option<RepoFindings> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let mut domains = Vec::new();
        if let Some(Value::Object(m)) = v.get("domains") {
            for (k, f) in m {
                domains.push((Domain::parse(k)?, Finding::from_value(f)?));
            }
        }
        Some(RepoFindings {
            repo: s("repo"),
            language: s("language"),
            visibility: s("visibility"),
            domains,
        })
    }
}

/// findings 配列の `jq -c` 表記(`--json` の 1 行、末尾改行なし)。
pub fn findings_to_json(findings: &[RepoFindings]) -> String {
    J::Arr(findings.iter().map(RepoFindings::to_j).collect()).compact()
}

/// findings 配列の JSON を読む(ledger の `repos`、`--json` の出力)。
pub fn findings_from_json(text: &str) -> Option<Vec<RepoFindings>> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.as_array()?.iter().map(RepoFindings::from_value).collect()
}

// ---------------------------------------------------------------------------
// 入力の型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Named {
    #[serde(default)]
    pub name: Option<String>,
}

/// `gh repo list --json name,primaryLanguage,...` の 1 要素。
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoMeta {
    pub name: String,
    pub primary_language: Option<Named>,
    pub visibility: Option<String>,
    pub description: Option<String>,
    pub repository_topics: Option<Vec<Named>>,
    pub default_branch_ref: Option<Named>,
    pub squash_merge_allowed: Option<bool>,
    pub merge_commit_allowed: Option<bool>,
    pub rebase_merge_allowed: Option<bool>,
    pub delete_branch_on_merge: Option<bool>,
    pub has_wiki_enabled: Option<bool>,
    pub has_projects_enabled: Option<bool>,
    pub created_at: Option<String>,
    pub pushed_at: Option<String>,
    pub is_archived: Option<bool>,
    pub viewer_permission: Option<String>,
}

impl RepoMeta {
    /// `jq -r '.primaryLanguage.name // "-"'`
    pub fn language(&self) -> String {
        self.primary_language
            .as_ref()
            .and_then(|l| l.name.clone())
            .unwrap_or_else(|| "-".to_string())
    }

    /// `jq -r '.visibility'`(null は文字列 "null")
    pub fn visibility_str(&self) -> String {
        self.visibility
            .clone()
            .unwrap_or_else(|| "null".to_string())
    }

    /// `[.repositoryTopics[]?.name]`
    pub fn topics(&self) -> Vec<String> {
        self.repository_topics
            .iter()
            .flatten()
            .filter_map(|t| t.name.clone())
            .collect()
    }
}

/// GraphQL の `object(expression: ...)`。ファイルなら `text`/`id` を持ち、
/// パスが無ければ JSON の null(= `Option::None`)。
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Blob {
    pub text: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TreeEntry {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// REST は 8 進文字列("120000")、GraphQL は 10 進数値(40960)で返す(#259)。
    pub mode: Option<Value>,
    pub object: Option<Blob>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tree {
    pub entries: Option<Vec<TreeEntry>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TotalCount {
    pub total_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReleaseNode {
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Releases {
    pub nodes: Option<Vec<ReleaseNode>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IssueNode {
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IssueActivity {
    pub total_count: Option<i64>,
    pub nodes: Option<Vec<IssueNode>>,
}

/// GraphQL バッチ(`build_graphql_query`)の 1 リポジトリ分。フィールド名は
/// クエリの alias。
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoGql {
    pub readme: Option<Blob>,
    pub contributing: Option<Blob>,
    pub claude_md: Option<Blob>,
    pub agents_md: Option<Blob>,
    pub root_tree: Option<Tree>,
    pub claude_skills_dir: Option<Tree>,
    pub routines_dir: Option<Tree>,
    pub cargo_toml: Option<Blob>,
    pub package_json: Option<Blob>,
    pub pyproject_toml: Option<Blob>,
    pub go_mod: Option<Blob>,
    pub flake_nix: Option<Blob>,
    pub workflows_dir: Option<Tree>,
    pub decl_ci: Option<Blob>,
    pub decl_pr_title: Option<Blob>,
    pub rulesets_dir: Option<Tree>,
    pub decl_security: Option<Blob>,
    pub decl_quality: Option<Blob>,
    pub decl_workflow: Option<Blob>,
    pub decl_review: Option<Blob>,
    pub renovate_json: Option<Blob>,
    pub renovate_json5: Option<Blob>,
    pub gh_renovate_json: Option<Blob>,
    pub gh_renovate_json5: Option<Blob>,
    pub renovaterc: Option<Blob>,
    pub renovate_dashboard: Option<TotalCount>,
    pub latest_release: Option<Releases>,
    pub open_issue_activity: Option<IssueActivity>,
}

impl RepoGql {
    /// JSON 値から読む。null や形の合わない値は空(bash 版の `// {}`)。
    pub fn from_value(v: &Value) -> RepoGql {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    pub fn from_json_str(s: &str) -> RepoGql {
        serde_json::from_str::<Value>(s)
            .map(|v| RepoGql::from_value(&v))
            .unwrap_or_default()
    }

    /// `(.workflowsDir.entries // [])`
    pub fn workflow_entries(&self) -> &[TreeEntry] {
        self.workflows_dir
            .as_ref()
            .and_then(|t| t.entries.as_deref())
            .unwrap_or(&[])
    }

    /// `(.workflowsDir.entries // []) | length > 0`(CI の有無)
    pub fn has_workflows(&self) -> bool {
        !self.workflow_entries().is_empty()
    }

    /// `(.workflowsDir.entries // []) | any(.name == $n)`
    pub fn has_workflow_file(&self, name: &str) -> bool {
        self.workflow_entries()
            .iter()
            .any(|e| e.name.as_deref() == Some(name))
    }
}

/// `$(jq -r '.<blob>.text // ""')`: テキスト(無ければ空)の末尾改行を落としたもの。
pub fn blob_text(b: &Option<Blob>) -> String {
    jq::sh_trim(b.as_ref().and_then(|b| b.text.as_deref()).unwrap_or("")).to_string()
}

/// `gh api repos/OWNER/REPO`(REST)のうち判定に使うフィールド。
///
/// bash 版は `jq -e '.field == value'` で見ていたので、型が合わない値・
/// 取得失敗(`{}` や `[]`)はすべて「一致しない」になる。手で `.get()` して
/// その意味を保つ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepoRest {
    pub squash_merge_commit_title: Option<String>,
    pub squash_merge_commit_message: Option<String>,
    pub allow_auto_merge: Option<bool>,
    pub dependabot_security_updates_status: Option<String>,
    pub has_pages: Option<bool>,
}

impl RepoRest {
    pub fn from_value(v: &Value) -> RepoRest {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let b = |k: &str| v.get(k).and_then(Value::as_bool);
        RepoRest {
            squash_merge_commit_title: s("squash_merge_commit_title"),
            squash_merge_commit_message: s("squash_merge_commit_message"),
            allow_auto_merge: b("allow_auto_merge"),
            dependabot_security_updates_status: v
                .pointer("/security_and_analysis/dependabot_security_updates/status")
                .and_then(Value::as_str)
                .map(str::to_string),
            has_pages: b("has_pages"),
        }
    }
}
