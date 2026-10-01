//! 横断 GitHub 監査(ADR-0015)の Rust 版(#414、ADR-0024 Stage 4e)。
//!
//! 読み取り専用で、tarotene の所有リポジトリ全部をドメインごとに判定し、
//! drift を報告するだけ(直すのは github-audit-triage スキルの LLM ノード)。
//! どのドメインも決定論的に判定し、LLM を呼ばない。設計の経緯は
//! `docs/github-audit.md`、bash 関数名 → Rust 関数の対応表は同文書の
//! 「ライブラリ API」節。
//!
//! 構成:
//! - [`config`] — 環境変数(`GITHUB_AUDIT_*`)からの設定
//! - [`gh`] — `gh` 経由の取得(リポジトリ一覧・REST・GraphQL バッチ)
//! - [`vocab`] — overrides.tsv と閉語彙 .tsv
//! - [`domains`] — ドメインごとの judge_*(bash 本体の節の並び)
//! - [`mod@audit`] — 全体の組み立て・ledger・human report
//! - [`model`] — 入出力の型(出力 JSON は bash 版とバイト単位で一致)

pub mod audit;
pub mod config;
pub mod domains;
pub mod gh;
pub mod jq;
pub mod model;
pub mod vocab;

pub use audit::{any_drift, audit, parse_domains, render_report, write_ledger};
pub use config::Config;
pub use gh::{build_graphql_query, FetchError, Gh};
pub use model::{
    findings_from_json, findings_to_json, Declaration, Detail, Domain, Finding, RepoFindings,
    RepoGql, RepoMeta, RepoRest, ReviewLayer, Verdict,
};
