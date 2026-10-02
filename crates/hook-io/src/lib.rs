//! hook/CLI 群の共通入出力(ADR-0024、#391)。
//!
//! bash 版で手続き的にしか同期されていなかった重複(#391 のクラスタ A〜G)を
//! 型として 1 箇所に寄せる。各モジュールの doc コメントに、吸収元の bash の
//! 行を書き残す — 移植時に「どの実装を正とするか」を追えるようにするため。
//!
//! | クラスタ | モジュール |
//! |---|---|
//! | A `permissionDecision` の emit | [`decision`] |
//! | B `default_branch()` | [`git::default_branch`] |
//! | C plan テキストの 3 段フォールバック | [`plan::plan_text`] |
//! | D stdin 読み + `cd $CWD` | [`input::read_stdin`] / [`input::HookInput::enter_cwd`] |
//! | E `CLAUDE_PROJECT_DIR` / `.cwd` 解決 | [`input::HookInput::project_dir`] |
//! | F `git rev-parse --git-common-dir` | [`git::git_common_dir`] |
//! | G `state_file()`(session_id サニタイズ) | [`ledger::SessionLedger`] |
//! | H 最小 POSIX シェル語分割 | [`shell::split`] |
//! | I コマンド正規化+ハッシュ(ADR-543 段3) | [`cmd_hash`] |
//! | J gate の deny/skip イベント記録(ADR-543 段3) | [`gate_event`] |
//! | K jq 互換の挿入順 JSON 出力(Stage 4b) | [`jqfmt`] |
//! | L `timeout(1)` / `date(1)` / `command -v`(Stage 4b) | [`proc`] |
//! | M herdr socket への `pane.report_metadata` 送信(#413) | [`herdr`] |
//!
//! H はもともと `crates/gh-edit-allow/src/shell.rs` にあったが、
//! `crates/rulesets-write-guard`(ADR-503)も
//! 同じ「gh コマンド文字列を静的に解析して deny/pass を決める」形の hook
//! で、判定に使えない入力を素通しに倒す同じ語分割ロジックを要求したため
//! ここへ引き上げた(ADR-0035 D1「単一正本 > 複写+同期」)。

pub mod cmd_hash;
pub mod decision;
pub mod gate_event;
pub mod git;
pub mod herdr;
pub mod input;
pub mod jqfmt;
pub mod ledger;
pub mod plan;
pub mod proc;
pub mod shell;
pub mod transcript;

pub use decision::{Decision, PermissionDecision};
pub use input::{Agent, HookInput};
pub use ledger::SessionLedger;
