//! 実行時設定(bash 版スクリプト冒頭の大域変数群)。
//!
//! bash 版はすべて `${GITHUB_AUDIT_*:-既定値}` で読んでいた — 空文字は未設定と
//! 同じ扱い。[`Config::from_env`] も同じ規則で読む。依存側(apply-rulesets.sh
//! 等)は `OWNER` を対象リポジトリの owner に上書きして使っていたので、
//! ライブラリ利用者は `owner` を直接書き換えてよい。

use std::env;
use std::path::{Path, PathBuf};

/// `${VAR:-}`: 未設定・空なら `None`。
fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn path_var(name: &str, default: PathBuf) -> PathBuf {
    var(name).map(PathBuf::from).unwrap_or(default)
}

#[derive(Debug, Clone)]
pub struct Config {
    /// `GITHUB_AUDIT_OWNER`(既定 tarotene)。
    pub owner: String,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub overrides_file: PathBuf,
    /// `GITHUB_AUDIT_GH_BIN`(既定 `gh`、PATH から引く)。
    pub gh_bin: String,
    /// `GITHUB_AUDIT_REPO_LIMIT`(#533、既定 "500")。`gh repo list --limit` へ
    /// そのまま渡す。
    pub repo_limit: String,
    /// `GITHUB_AUDIT_VIEWER_PERMISSION`(#533)。空なら絞り込まない。
    pub viewer_permission: Option<String>,
    // ADR-0020: naming ドメインの閉語彙。repo 追跡の PUBLIC 版と、コミット
    // しない .local.tsv(PRIVATE リポジトリ分)の和集合を読む。
    pub codename_registry_file: PathBuf,
    pub codename_registry_local_file: PathBuf,
    pub descriptive_species_file: PathBuf,
    pub site_domains_file: PathBuf,
    pub site_domains_local_file: PathBuf,
    /// ADR-0026: lifecycle-study 候補の語彙(PUBLIC のみ)。
    pub lifecycle_species_file: PathBuf,
    /// ADR-519: 自己監査 routine の sources(PUBLIC/PRIVATE の対)。
    pub routines_auditor_sources_file: PathBuf,
    pub routines_auditor_sources_local_file: PathBuf,
    /// workflows ドメインの quality.json 単一正本(ADR-0035 単一正本 > 複写+同期)。
    pub workflows_canonical_quality_file: PathBuf,
    /// ADR-590 D3: `github-app-snapshot` が書く snapshot(読むだけ)。
    pub app_snapshot_file: PathBuf,
    pub releaser_app_name: String,
    /// ADR-568 D4: 全リポジトリの renovate.json が extends すべき浮動参照。
    pub renovate_policy_preset: String,
    /// `GITHUB_AUDIT_LIFECYCLE_NOW`(lifecycle の基準時刻、未設定なら現在時刻)。
    pub lifecycle_now: Option<String>,
}

impl Config {
    pub fn from_env() -> Config {
        let home = env::var("HOME").unwrap_or_default();
        let config_dir = path_var(
            "GITHUB_AUDIT_CONFIG_DIR",
            var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(&home).join(".config"))
                .join("github-audit"),
        );
        let state_dir = path_var(
            "GITHUB_AUDIT_STATE_DIR",
            var("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(&home).join(".local/state"))
                .join("github-audit"),
        );
        let c = |name: &str, file: &str| path_var(name, config_dir.join(file));
        Config {
            owner: var("GITHUB_AUDIT_OWNER").unwrap_or_else(|| "tarotene".to_string()),
            overrides_file: c("GITHUB_AUDIT_OVERRIDES_FILE", "overrides.tsv"),
            gh_bin: var("GITHUB_AUDIT_GH_BIN").unwrap_or_else(|| "gh".to_string()),
            repo_limit: var("GITHUB_AUDIT_REPO_LIMIT").unwrap_or_else(|| "500".to_string()),
            viewer_permission: var("GITHUB_AUDIT_VIEWER_PERMISSION"),
            codename_registry_file: c(
                "GITHUB_AUDIT_CODENAME_REGISTRY_FILE",
                "codename-registry.tsv",
            ),
            codename_registry_local_file: c(
                "GITHUB_AUDIT_CODENAME_REGISTRY_LOCAL_FILE",
                "codename-registry.local.tsv",
            ),
            descriptive_species_file: c(
                "GITHUB_AUDIT_DESCRIPTIVE_SPECIES_FILE",
                "descriptive-species.tsv",
            ),
            site_domains_file: c("GITHUB_AUDIT_SITE_DOMAINS_FILE", "site-domains.tsv"),
            site_domains_local_file: c(
                "GITHUB_AUDIT_SITE_DOMAINS_LOCAL_FILE",
                "site-domains.local.tsv",
            ),
            lifecycle_species_file: c(
                "GITHUB_AUDIT_LIFECYCLE_SPECIES_FILE",
                "lifecycle-species.tsv",
            ),
            routines_auditor_sources_file: c(
                "GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_FILE",
                "routines-auditor-sources.tsv",
            ),
            routines_auditor_sources_local_file: c(
                "GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_LOCAL_FILE",
                "routines-auditor-sources.local.tsv",
            ),
            workflows_canonical_quality_file: path_var(
                "GITHUB_AUDIT_WORKFLOWS_CANONICAL_QUALITY_FILE",
                default_canonical_quality_file(),
            ),
            app_snapshot_file: path_var(
                "GITHUB_AUDIT_APP_SNAPSHOT_FILE",
                state_dir.join("app-snapshot.json"),
            ),
            releaser_app_name: var("GITHUB_AUDIT_RELEASER_APP_NAME")
                .unwrap_or_else(|| "releaser".to_string()),
            renovate_policy_preset: var("GITHUB_AUDIT_RENOVATE_POLICY_PRESET")
                .unwrap_or_else(|| "github>tarotene/dotfiles//renovate/policy".to_string()),
            lifecycle_now: var("GITHUB_AUDIT_LIFECYCLE_NOW"),
            config_dir,
            state_dir,
        }
    }

    /// この設定の owner / gh で REST・GraphQL を叩くハンドル。
    pub fn gh(&self) -> crate::gh::Gh {
        crate::gh::Gh::new(&self.gh_bin, &self.owner)
    }
}

/// bash 版の既定値 `$(dirname "$SELF")/../config/claude/skills/
/// repo-governance-common/templates/.github/rulesets/quality.json` と同じ規則:
/// 実行ファイルの実体(realpath)の 1 つ上からの相対パス。リポジトリの
/// checkout から `scripts/github-audit` を直接叩いたときだけ実在し、
/// home-manager で nix store に配備された実体からは解決できない(その場合
/// bash 版と同じく `-r` 不成立で検査自体を飛ばす)。
fn default_canonical_quality_file() -> PathBuf {
    let exe = env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_default();
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.join(
        "../config/claude/skills/repo-governance-common/templates/.github/rulesets/quality.json",
    )
}
