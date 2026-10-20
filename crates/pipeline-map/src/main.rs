//! `pipeline-map check [--root <dir>]` — `pipeline-map.toml` と登録箇所の突き合わせ。
//! 判定は `pipeline_map::audit`(`cargo test` が同じ関数を呼ぶ)。

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: pipeline-map check [--root <dir>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn check(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--root", Some(v)) => root = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let manifest = match pipeline_map::load(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("pipeline-map: {e}");
            return ExitCode::from(1);
        }
    };
    let r = pipeline_map::audit(&root, &manifest);
    eprintln!(
        "pipeline-map: node {}(auto {})/ edge {} / 登録箇所 {}",
        r.nodes, r.auto_nodes, r.edges, r.sites
    );
    if r.problems.is_empty() {
        println!("OK: pipeline-map.toml と登録箇所は一致しています");
        ExitCode::SUCCESS
    } else {
        for p in &r.problems {
            eprintln!("  - {p}");
        }
        eprintln!("pipeline-map: {} 件の不一致", r.problems.len());
        ExitCode::from(1)
    }
}
