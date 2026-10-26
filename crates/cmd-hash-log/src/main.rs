//! hook エントリポイント。本体は lib の [`cmd_hash_log::hook`](詳細は lib.rs)。
//!
//! deny/allow は一切返さない(記録専用の PostToolUse hook)。読めない・
//! 書けない場合は黙って exit 0(best-effort — 記録の失敗で作業を止めない、
//! `gh-edit-allow`/`agent_session::gate_event` と同じ fail-open)。

use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    let _ = cmd_hash_log::hook(&raw);
}
