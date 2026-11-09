//! hook エントリポイント。詳細は lib.rs / docs/agents/pkexec-guard.md。
//!
//! PreToolUse 専用。3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から
//! 同じ 1 バイナリを呼ぶ——`attribution-guard.sh` の bash `source` に相当
//! する仕組みを Rust は持たないため、代わりに `--agent <claude|codex|
//! copilot>` フラグで出力形式を切り替える。フラグ省略時
//! は `claude`(home/modules/claude.nix の登録では常に明示するが、手動
//! 実行時のデフォルトとして残す)。入口(引数・標準入力・出力)は
//! `agent_session::entry` が持つ。
//!
//! 判定できない入力(stdin が不正 JSON、対象イベントでない)では何も
//! 出力せず exit 0 する(判定できないことを deny に変えない — hook 入力
//! そのものが読めない場合だけの fail-open。pkexec を含むコマンドを解析
//! できない場合は lib.rs 側で deny に倒す)。

use agent_session::entry::{run, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "pkexec-guard",
    agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    run(&SPEC, pkexec_guard::hook)
}
