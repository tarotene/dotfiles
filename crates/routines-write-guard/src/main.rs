//! hook エントリポイント。詳細は lib.rs / docs/claude/claude-routines.md。
//!
//! PreToolUse 専用。判定できない入力では何も出力せず exit 0 する
//! (判定できないことを deny に変えない — rulesets-write-guard と同じ
//! fail-open 方針)。

use hook_io::Agent;

fn main() {
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    if input.hook_event_name != "PreToolUse" {
        return;
    }
    if let Some(decision) = routines_write_guard::check(&input) {
        decision.emit(Agent::Claude);
    }
}
