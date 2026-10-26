//! hook エントリポイント。詳細は lib.rs / docs/agents/rulesets-write-guard.md。
//!
//! PreToolUse 専用(gh-edit-allow と違い記録役を持たない)。判定できない
//! 入力では何も出力せず exit 0 する(判定できないことを deny に変えない)。

use agent_session::Agent;
use agent_session::Outcome;
use agent_session::PermissionDecision;
use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(Outcome::Deny(reason)) = rulesets_write_guard::hook(&raw) {
        PermissionDecision::deny(reason).emit(Agent::Claude);
    }
}
