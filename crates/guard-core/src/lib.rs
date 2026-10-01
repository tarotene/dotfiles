//! gh 投稿系 PreToolUse guard の共通判定エンジン(ADR-0024 Stage 4a、#415)。
//!
//! bash 版では `config/claude/hooks/attribution-guard.sh` が「1 つの判定
//! エンジン」で、stack-base-guard.sh / pr-title-guard.sh / pr-confirm-guard.sh
//! / feedback-target-guard.sh / decision-colocation-guard.sh / adr-number.sh /
//! repo-create-guard.sh がそれを `source` し、`is_target_at` / `decide_tokens`
//! などを関数の後勝ちで上書きして再利用していた。Rust には `source` に相当する
//! 仕組みが無いので、上書きされていた継ぎ目を「引数として渡す関数」に、
//! グローバル変数(`TOK` / `CMD_NOHD` / `HD_BODIES` / `TARGET_KIND`)を戻り値の
//! 型に置き換えた純関数群として公開する。
//!
//! | bash | Rust |
//! |---|---|
//! | `split_heredoc` | [`shell::split_heredoc`] |
//! | `tokenize` / `is_sep` / `CMD_SEPS` | [`shell::tokenize`] / [`shell::is_sep`] / [`shell::CMD_SEPS`] |
//! | `decide()` の範囲切り出し(`is_target_at` を上書き) | [`command::parse`] + [`command::ParsedCommand::ranges`] |
//! | 本文フラグの抽出(`decide_tokens` 前半 / `extract_body`) | [`gh::BodyFlags`] / [`gh::extract_body`] |
//! | `gh api` の `-X`/パス/本文(`decide_api_tokens` 前半) | [`gh::api_method`] / [`gh::api_path`] / [`gh::ApiBody`] |
//! | `--base`/`--repo`/`--title` 等の値フラグ | [`gh::scan_value_flags`] |
//! | `No-Attribution:` / `Independent-PR:` 型の理由必須タグ | [`marker::has_reasoned_tag`] |
//! | `owner_repo` / `default_branch`(gh フォールバック付き) | [`repo::owner_repo`] / [`repo::default_branch_or_gh`] |
//! | `main()` の `jq -r '.tool_name // empty'` 等 / `emit_deny` | [`hook::ToolCall`] / [`hook::deny_output`] |
//!
//! 対応表の詳細と後続の移植者向けの使い方は `docs/claude/guard-core.md`。
//! 振る舞いは bash 版と 1 対 1 に保つ(移植であってリファクタではない) —
//! bash 版の挙動の根拠は各関数の doc コメントに残す。

pub mod command;
pub mod gh;
pub mod hook;
pub mod marker;
pub mod repo;
pub mod shell;

pub use command::{parse, ParsedCommand, Range};
