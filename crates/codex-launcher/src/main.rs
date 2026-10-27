//! Credential-aware entry point for the native Codex CLI.
use codex_launcher::{command, Launch};
use std::{os::unix::process::CommandExt, path::PathBuf, process::ExitCode};

fn run() -> Result<(), String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let mut child = command(
        &home.join(".local/bin/codex"),
        Launch::Cli(std::env::args_os().skip(1).collect()),
    )?;
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
