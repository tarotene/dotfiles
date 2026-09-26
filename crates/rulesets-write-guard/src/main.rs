//! hook エントリポイント。詳細は lib.rs / docs/claude/rulesets-write-guard.md。
//!
//! PreToolUse 専用(gh-edit-allow と違い記録役を持たない)。判定できない
//! 入力では何も出力せず exit 0 する(判定できないことを deny に変えない)。

use hook_io::Agent;

fn main() {
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    if input.hook_event_name != "PreToolUse" {
        return;
    }
    if let Some(decision) = rulesets_write_guard::check(&input) {
        decision.emit(Agent::Claude);
    }
}
