//! pr-gate の入口。`pr-gate session-start` / `pr-gate stop` を settings.json の
//! SessionStart / Stop から stdin JSON 付きで呼ぶ。それ以外の引数は何もせず
//! exit 0(bash 版の dispatch と同じ)。設計は lib.rs と docs/claude/pr-gate.md。

use std::io::Read;

fn main() {
    let sub = std::env::args().nth(1).unwrap_or_default();
    let run: fn(&str) -> i32 = match sub.as_str() {
        "session-start" => pr_gate::session_start::run,
        "stop" => pr_gate::stop::run,
        _ => return,
    };
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    std::process::exit(run(&input));
}
