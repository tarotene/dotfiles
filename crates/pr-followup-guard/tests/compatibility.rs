//! Old and canonical executables share code, stdio, exit status and state contracts.
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn run(bin: &str, home: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(bin)
        .args([] as [&str; 0])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("CLAUDE_PROJECT_DIR", home)
        .env_remove("HERDR_SOCKET_PATH")
        .env_remove("HERDR_PANE_ID")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}\n").unwrap();
    child.wait_with_output().unwrap()
}
#[test]
fn compatibility_entry_preserves_exit_and_output() {
    let home = tempfile::tempdir().unwrap();
    let canonical = run(env!("CARGO_BIN_EXE_pr-followup-guard"), home.path());
    let old = run(env!("CARGO_BIN_EXE_pr-confirm-guard"), home.path());
    assert_eq!(old.status.code(), canonical.status.code());
    assert_eq!(old.stdout, canonical.stdout);
    assert_eq!(old.stderr, canonical.stderr);
}
