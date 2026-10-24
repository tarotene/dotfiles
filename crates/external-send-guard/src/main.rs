//! PreToolUse(matcher: `mcp__.*`)から stdin JSON で呼ばれる。判定の本体は lib.rs。
//!
//! 縮退: stdin 不正・tool_name 無しは黙って exit 0。`EXTERNAL_SEND_GUARD_ALLOW=1`
//! なら即 pass(この名前は理由文に書かない — 当事者が自分で bypass できてしまう)。

use hook_io::Outcome;
use std::io::{Read, Write};

fn main() {
    let mut raw = Vec::new();
    if std::io::stdin().read_to_end(&mut raw).is_err() {
        return;
    }
    // jq と同じく不正な UTF-8 は U+FFFD に置き換えてから読む
    let text = String::from_utf8_lossy(&raw);
    if let Some(Outcome::Deny(reason)) = external_send_guard::hook(&text) {
        let _ = std::io::stdout().write_all(external_send_guard::deny_json(&reason).as_bytes());
    }
}
