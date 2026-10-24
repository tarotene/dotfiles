//! hook エントリポイント。詳細は lib.rs / docs/claude/claude-routines.md。
//!
//! PreToolUse 専用。判定できない入力では何も出力せず exit 0 する
//! (判定できないことを deny に変えない — rulesets-write-guard と同じ
//! fail-open 方針)。

use hook_io::{Agent, Outcome, PermissionDecision};
use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    if let Some(Outcome::Deny(reason)) = routines_write_guard::hook(&raw) {
        PermissionDecision::deny(reason).emit(Agent::Claude);
    }
}
