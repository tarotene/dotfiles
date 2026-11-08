//! hook エントリポイント。本体は lib の [`cmd_hash_log::hook`](詳細は lib.rs)。
//!
//! deny/allow は一切返さない(記録専用の PostToolUse hook)。読めない・
//! 書けない場合は黙って exit 0(best-effort — 記録の失敗で作業を止めない、
//! `gh-edit-allow`/`agent_session::gate_event` と同じ fail-open)。

fn main() {
    let raw = agent_session::entry::read_stdin();
    let _ = cmd_hash_log::hook(&raw);
}
