//! `worktree-lease status|release [dir]` — 人が見る・外すための入口。判定本体はルータ
//! (`crates/hook-router`)が `worktree_lease::hook` を呼ぶ。設計: docs/agents/worktree-lease.md。

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    let dir = args
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match cmd.as_deref() {
        Some("status") => {
            println!("{}", worktree_lease::status(&dir));
            ExitCode::SUCCESS
        }
        Some("release") => {
            if worktree_lease::release(&dir) {
                println!("lease を外しました");
            } else {
                println!("外す lease はありません(linked worktree ではないか、lease が無い)");
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: worktree-lease status|release [dir]");
            ExitCode::from(2)
        }
    }
}
