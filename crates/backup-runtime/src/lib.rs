//! Credential and process boundary shared by backup commands.
use std::ffi::OsStr;
use std::process::{Command, ExitStatus, Output, Stdio};

pub fn command(variable: &str, default: &str) -> Command {
    Command::new(std::env::var_os(variable).unwrap_or_else(|| default.into()))
}

pub fn checked(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("cannot start command: {e}"))?;
    status_ok(status)
}

pub fn status_ok(status: ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(match status.code() {
            Some(code) => format!("command failed (exit code {code})"),
            None => format!("command terminated: {status}"),
        })
    }
}

pub fn capture(command: &mut Command) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start command: {e}"))
}

pub fn nonempty_env(name: &str) -> Result<(), String> {
    if std::env::var_os(name).is_some_and(|value| !value.is_empty()) {
        Ok(())
    } else {
        Err(format!("Bitwarden project is missing {name}"))
    }
}

pub fn keyring_lookup(app: &str, credential: &str, binary_env: &str) -> Result<String, String> {
    let output = capture(command(binary_env, "secret-tool").args([
        "lookup",
        "application",
        app,
        "credential",
        credential,
    ]))?;
    status_ok(output.status)?;
    let value = String::from_utf8(output.stdout).map_err(|_| "invalid keyring token encoding")?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err("Bitwarden machine token is missing from the login keyring".into());
    }
    Ok(value.to_owned())
}

pub fn arg_env(command: &mut Command, key: &str, value: &OsStr) {
    let mut assignment = std::ffi::OsString::from(key);
    assignment.push("=");
    assignment.push(value);
    command.arg(assignment);
}
