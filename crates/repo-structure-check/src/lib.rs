//! `repo-structure-check` — リポジトリ構造の不変条件を、`cargo test` で検査する。
//!
//! 設計と根拠: `docs/agents/repo-structure-check.md`。
//!
//! 検査は純粋関数(manifest の文字列 → 違反の一覧)で、実際のツリーへの適用は
//! `tests/tree.rs` が行う。検査を足すときは、違反を作る fixture(負例)を同じ場所に置き、
//! 検査が実際に落ちることを確かめる。

pub mod civil_time;
pub mod deps;
pub mod escape;
pub mod hook_io;
pub mod host_names;
pub mod manifest;
pub mod process_env;
