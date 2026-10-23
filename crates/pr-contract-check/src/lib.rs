//! pr-contract-check — PR タイトル契約(ADR-0031)まわりの required check の実体
//! (`.github/actions/pr-title` の step)。
//!
//! 他リポジトリの CI が `pr-title-check`(タイトルの文法、別 crate)と並べてこの crate を
//! sparse-checkout して `cargo build` するので、**依存ゼロ**(標準ライブラリだけ)を条件に
//! する。GitHub への問い合わせは `gh api --jq` が出す行を読み、JSON は自前で読まず、
//! 要るところは `jq` を呼ぶ。
//!
//! ```text
//! pr-merge-settings-check <owner>/<repo>
//! workflow-naming-check   <owner>/<repo> [--ref REF]
//! rulesets-context-check  <owner>/<repo> <head_sha> <default_branch>
//! ```
//!
//! 判定は、かつて `scripts/github-audit`(bash)の `judge_*` / `fetch_*` を `source` する
//! 3 本の bash だった。判定そのものは純関数([`settings::judge_settings`] /
//! [`workflows::judge_workflows`])で、`crates/github-audit` も同じ関数を使う
//! (ADR-0035 D1「単一正本 > 複写+同期」)。

pub mod contexts;
pub mod gh;
pub mod merge_settings;
pub mod naming;
pub mod settings;
pub mod verdict;
pub mod workflows;
