//! settings-reconcile — 使い方は lib.rs。

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match settings_reconcile::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            eprintln!("settings-reconcile: {}", f.message);
            ExitCode::from(f.code)
        }
    }
}
