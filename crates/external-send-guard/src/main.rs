//! PreToolUse(matcher: `mcp__.*`)から stdin JSON で呼ばれる。判定の本体は lib.rs。
//!
//! 縮退: stdin 不正・tool_name 無しは黙って exit 0。`EXTERNAL_SEND_GUARD_ALLOW=1`
//! なら即 pass(この名前は理由文に書かない — 当事者が自分で bypass できてしまう)。

use std::io::{Read, Write};
use std::path::PathBuf;

fn self_list_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("external-send-guard/self.txt"))
}

fn main() {
    if std::env::var_os("EXTERNAL_SEND_GUARD_ALLOW").as_deref() == Some("1".as_ref()) {
        return;
    }
    let mut raw = Vec::new();
    if std::io::stdin().read_to_end(&mut raw).is_err() {
        return;
    }
    // jq と同じく不正な UTF-8 は U+FFFD に置き換えてから読む
    let text = String::from_utf8_lossy(&raw);
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    // `jq -r '.tool_name // empty'` の結果を `$(...)` で受ける(末尾改行は落ちる)
    let Some(tool) = input.get("tool_name").and_then(|t| t.as_str()) else {
        return;
    };
    let tool = tool.trim_end_matches('\n');
    if tool.is_empty() {
        return;
    }
    let self_set = self_list_path()
        .and_then(|p| std::fs::read(p).ok())
        .map(|c| external_send_guard::parse_self_list(&c))
        .unwrap_or_default();
    if let Some(reason) = external_send_guard::decide(tool, &input, &self_set) {
        let _ = std::io::stdout().write_all(external_send_guard::deny_json(&reason).as_bytes());
    }
}
