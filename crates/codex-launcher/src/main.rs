//! Credential-aware entry point for the native Codex CLI.
use codex_launcher::{command, update, Launch};
use std::{os::unix::process::CommandExt, path::PathBuf, process::ExitCode};

fn run() -> Result<(), String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "update") {
        if args.len() != 1 {
            return Err("codex update accepts no arguments".into());
        }
        return update(&home.join(".local/bin/codex"), &home);
    }
    let mut child = command(&home.join(".local/bin/codex"), Launch::Cli(args))?;
    Err(format!("cannot execute native Codex: {}", child.exec()))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("codex-launcher: {error}");
            ExitCode::FAILURE
        }
    }
}
