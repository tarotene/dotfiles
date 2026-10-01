//! decision-colocation-check — CI required check の入口(判定は
//! `decision_colocation::check::run_check`、client guard と共有する単一
//! ソース、ADR-396)。bash 版 `scripts/decision-colocation-check` の移植。
//!
//! 使い方:
//!   decision-colocation-check --base <ref>   # 判定
//!   (自己検査 `--selftest` は `cargo test -p decision-colocation` に移った)
//!
//! 終了コード: 0 = 適合, 1 = 非適合, 2 = 判定不能(使い方誤り等)。
//! 違反メッセージは stderr、適合なら stdout に `decision-colocation-check: OK`。

use std::path::PathBuf;
use std::process::ExitCode;

use decision_colocation::check::run_check;

const USAGE: &str = "usage: decision-colocation-check --base <ref>\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let mut base = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            // bash 版は値なしの `--base` で `shift 2` が失敗し、errexit で
            // rc=1(非適合と区別できない)になっていた。ここでは使い方誤り
            // (rc=2)にする。
            "--base" => match it.next() {
                Some(v) => base = v.clone(),
                None => return usage_error(),
            },
            _ => return usage_error(),
        }
    }
    if base.is_empty() {
        return usage_error();
    }

    // `git rev-parse --show-toplevel 2>/dev/null || pwd`
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = hook_io::git::toplevel(&cwd).unwrap_or(cwd);

    let violations = run_check(&root, &base);
    if violations.is_empty() {
        println!("decision-colocation-check: OK");
        ExitCode::SUCCESS
    } else {
        for v in violations {
            eprintln!("{v}");
        }
        ExitCode::from(1)
    }
}

fn usage_error() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}
