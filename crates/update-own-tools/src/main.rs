//! update-own-tools [--dry-run] [--registry <path>] [<name>...]
//!
//! 詳細は lib.rs と docs/update-own-tools.md。

use std::process::ExitCode;
use update_own_tools::{execute, on_path, parse_registry, plan, xdg_dir};

const USAGE: &str = "usage: update-own-tools [--dry-run] [--registry <path>] [<name>...]";

fn main() -> ExitCode {
    let mut dry_run = false;
    let mut registry = None;
    let mut names = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dry-run" | "-n" => dry_run = true,
            "--registry" => match args.next() {
                Some(p) => registry = Some(p.into()),
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            s if s.starts_with('-') => {
                eprintln!("unknown option: {s}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => names.push(a),
        }
    }
    let Some(registry) = registry.or_else(|| {
        xdg_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join("update-own-tools/registry.toml"))
    }) else {
        eprintln!("update-own-tools: HOME が未設定です");
        return ExitCode::FAILURE;
    };
    let Some(cache) = xdg_dir("XDG_CACHE_HOME", ".cache").map(|d| d.join("update-own-tools"))
    else {
        eprintln!("update-own-tools: HOME が未設定です");
        return ExitCode::FAILURE;
    };
    let text = match std::fs::read_to_string(&registry) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "update-own-tools: レジストリを読めません: {}: {e}\n(スキーマは docs/update-own-tools.md)",
                registry.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let reg = match parse_registry(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("update-own-tools: {}: {e}", registry.display());
            return ExitCode::FAILURE;
        }
    };
    let tools: Vec<_> = reg
        .tools
        .iter()
        .filter(|t| names.is_empty() || names.contains(&t.name))
        .collect();
    for n in &names {
        if !reg.tools.iter().any(|t| &t.name == n) {
            eprintln!("update-own-tools: レジストリに無い名前です: {n}");
            return ExitCode::FAILURE;
        }
    }
    let run_id = std::process::id().to_string();
    let mut failed = 0;
    for t in tools {
        let steps = plan(t, &cache, &run_id);
        println!("== {}", t.name);
        if dry_run {
            for s in &steps {
                println!("  {s}");
            }
            continue;
        }
        if !on_path(&t.install[0]) {
            eprintln!(
                "update-own-tools: {}: {} が PATH にありません(cargo は rustup 等で用意してください — ADR-0002)",
                t.name, t.install[0]
            );
            failed += 1;
            continue;
        }
        for sub in ["worktrees", "target"] {
            if let Err(e) = std::fs::create_dir_all(cache.join(sub)) {
                eprintln!("update-own-tools: {}: {e}", cache.display());
                return ExitCode::FAILURE;
            }
        }
        match execute(&steps) {
            Ok(()) => println!("  ok"),
            Err(e) => {
                eprintln!("update-own-tools: {}: {e}", t.name);
                failed += 1;
            }
        }
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
