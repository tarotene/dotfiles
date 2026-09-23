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

pub mod decision;
pub mod git;
pub mod input;
pub mod ledger;
pub mod plan;

pub use decision::{Decision, PermissionDecision};
pub use input::{Agent, HookInput};
pub use ledger::SessionLedger;
