//! hook-router のエントリポイント。`hook-router pre|post`。
//!
//! 縮退: 標準入力が読めない・JSON として読めない・`tool_name` が無いなら、何も出さず exit 0
//! (判定できない入力は、断定に変えず素通す)。素通りにした規則は `gate-events.jsonl` に
//! `fail-open` で記録する(記録は best-effort)。

use hook_router::{applicable, execute, merge, render, tool_name, Event, Verdict};
use std::io::{Read, Write};

fn main() {
    let event = match std::env::args().nth(1).as_deref() {
        Some("pre") => Event::Pre,
        Some("post") => Event::Post,
        _ => {
            eprintln!("usage: hook-router pre|post");
            std::process::exit(1);
        }
    };

    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        return;
    }
    // jq と同じく、不正な UTF-8 は U+FFFD に置き換えてから読む。
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let Some(tool) = tool_name(&raw) else {
        return;
    };

    let rules = applicable(event, &tool);
    let results = execute(&rules, &raw);

    if let Some(path) = hook_io::gate_event::default_path() {
        for (node, verdict) in &results {
            if matches!(verdict, Verdict::Panicked | Verdict::TimedOut) {
                let _ = hook_io::gate_event::record(&path, node, "fail-open");
            }
        }
    }

    if let Some(line) = render(event, &merge(&results)) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
    // 時間を越えた規則のスレッドを待たずに、プロセスごと終わる。
    std::process::exit(0);
}
