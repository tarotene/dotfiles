//! bash 版 hook/CLI の characterization fixture を置く場所(#391 の移植方法論)。
//!
//! 移植は 4 段で行う(docs/rust-migration.md):
//!
//! 1. 対象 bash の振る舞いを `tests/cases/<bin>/*.toml`(trycmd 形式)に抽出する
//! 2. `tests/oracle.rs` の [`BASH_ORACLES`] に登録し、bash 版に対して緑にする
//! 3. fixture ディレクトリを Rust 版クレートの `tests/cmd/` へ移し、同じケースを
//!    `trycmd::cargo::cargo_bin!` に向けて緑にする
//! 4. bash 実装と selftest ランナー、ここの登録行を削除する
//!
//! Rust 側へケースを手で転記しない — 同じ fixture ファイルを bash と Rust の
//! 両方に向けることで、転記ミスというバグ源を持ち込まない。

/// fixture を bash 版に向けている hook/CLI(bin 名, リポジトリ相対パス)。
pub const BASH_ORACLES: &[(&str, &str)] = &[(
    "external-send-guard",
    "config/claude/hooks/external-send-guard.sh",
)];
