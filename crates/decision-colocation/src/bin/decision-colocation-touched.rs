//! decision-colocation-touched — 触れた決定の一覧(PR 本文の `## 触れた決定` の節)を出す。
//! 判定は `decision_colocation::touched::run`(`docs/agents/agent-mode.md` の第 2 段の入力)。
//!
//! 使い方:
//!   decision-colocation-touched --base `<ref>`
//!
//! 終了コード: 0 = 出力した(一覧が空でも 0)、2 = 使い方の誤り、または base を解決できない。
//! 一覧は標準出力、エラーは標準エラー。何も書き換えない。

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: decision-colocation-touched --base <ref>\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let base = match args.as_slice() {
        [flag, value] if flag == "--base" && !value.is_empty() => value.clone(),
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = git_repository::toplevel(&cwd).unwrap_or(cwd);
    match decision_colocation::touched::run(&root, &base) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("decision-colocation-touched: {e}");
            ExitCode::from(2)
        }
    }
}
