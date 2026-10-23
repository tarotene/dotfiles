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

/// `--help` の末尾に足す、明示指定でだけ走るドメインの説明(Rust 版のみ)。
/// bash 版の `--help` には無いので、差分テストはこの区画を外してから比べる。
pub const OPT_IN_USAGE: &str = r#"
Opt-in domains (named explicitly; never part of the default run, and not in
the bash version):

  annotations  warning annotations of the latest completed run of every
             workflow on the default branch (ADR-568 Amendment 2026-10-23).
             Node.js 20 deprecations and cache misses (a config that
             disagrees with the repository's contents) show up only in a
             run's annotations, never in a static lint. Every job of every
             latest run is read, so this costs many REST calls: it is not
             run by default. missing=<kind>@<workflow>/<job>, where kind is
             node20-deprecated:<actions> | cache-dependency-file-not-found |
             cache-restore-failed | cache-save-failed | other:<text>.
             not-applicable for repos with no .github/workflows. failure
             and notice annotations are not read (a red run is CI's own
             signal)
"#;

/// home-manager が generation ごとに置く、dotfiles のコミットの 1 行テキスト
/// (`~/.local/share/dotfiles/rev`、flake.nix の `mkHome`)。
const GENERATION_REV_REL: &str = ".local/share/dotfiles/rev";

/// いま動いているバイナリが入った generation の rev。読めなければ `None`。
///
/// コンパイル時の環境変数でバイナリに埋め込まない。コミットごとにワークスペース
/// 全体が再ビルドされ、`cleanCargoSource` が文書だけの変更で再ビルドさせない
/// 作りを壊すため(#807)。
pub fn generation_rev() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text =
        std::fs::read_to_string(std::path::Path::new(&home).join(GENERATION_REV_REL)).ok()?;
    let rev = text.trim();
    (!rev.is_empty()).then(|| rev.to_string())
}

/// `--version` の 1 行(#807)。古い版を使い続けていることを、判定を信じる前に
/// 気づけるようにする(ADR-792 より前の版が、無料プランの非公開リポジトリを
/// 誤報した、2026-10-15)。
pub fn version_line(rev: Option<&str>) -> String {
    format!(
        "github-audit {} (generation {})",
        env!("CARGO_PKG_VERSION"),
        rev.unwrap_or("unknown")
    )
}

pub use audit::{any_drift, audit, parse_domains, render_report, write_ledger};
pub use config::Config;
pub use gh::{build_graphql_query, FetchError, Gh};
pub use model::{
    findings_from_json, findings_to_json, Declaration, Detail, Domain, Finding, RepoFindings,
    RepoGql, RepoMeta, RepoRest, ReviewLayer, Verdict,
};
