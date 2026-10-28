//! Isolate parent environment in subprocesses, as each native launch does.
use codex_launcher::{command, Launch};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::Command,
};

const TOKEN: &str = "ghp_0000000000000000000000000000000000000000";
const CONFIG: &str = r#"default_permissions = "fixture"
approval_policy = "on-request"
approvals_reviewer = "auto_review"
[features]
network_proxy = true
credential_masking = true
[permissions.fixture]
extends = ":workspace"
[permissions.fixture.network]
enabled = true
"#;
fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn prepared_entrypoints() {
    if let Ok(kind) = std::env::var("ENTRYPOINT_CHILD") {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let launch = match kind.as_str() {
            "session" => Launch::SessionServer,
            "discovery" => Launch::DiscoveryServer,
            "tui" => Launch::RemoteTui(vec!["--remote".into(), "unix:///synthetic".into()]),
            _ => unreachable!(),
        };
        match command(&home.join(".local/bin/codex"), launch) {
            Ok(mut child) => {
                eprintln!("{}", child.exec());
                std::process::exit(99);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex/config.toml"), CONFIG).unwrap();
    script(
        &home.join("bin/gh"),
        &format!("echo acquire >> \"$HOME/acquired\"; echo '{TOKEN}'"),
    );
    script(
        &home.join(".local/bin/codex"),
        &format!(
            r#"
if [ "$1" = --version ]; then
  test -z "${{GH_TOKEN:-}}${{GITHUB_TOKEN:-}}${{GH_ENTERPRISE_TOKEN:-}}${{GITHUB_ENTERPRISE_TOKEN:-}}" || exit 90
  echo 'codex-cli 0.165.0'; exit
fi
case "$ENTRYPOINT_CHILD" in
  session) test "$GH_TOKEN" = '{TOKEN}' || exit 91 ;;
  *) test -z "${{GH_TOKEN:-}}" || exit 92 ;;
esac
test -z "${{GITHUB_TOKEN:-}}${{GH_ENTERPRISE_TOKEN:-}}${{GITHUB_ENTERPRISE_TOKEN:-}}" || exit 93
printf '%s\n' "$@"
exit 23
"#
        ),
    );
    let run = |kind: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "prepared_entrypoints", "--nocapture"])
            .env_clear()
            .env("HOME", home)
            .env("PATH", home.join("bin"))
            .env("ENTRYPOINT_CHILD", kind)
            .env("GH_TOKEN", "inherited")
            .env("GITHUB_TOKEN", "inherited")
            .env("GH_ENTERPRISE_TOKEN", "inherited")
            .env("GITHUB_ENTERPRISE_TOKEN", "inherited")
            .current_dir(home)
            .output()
            .unwrap()
    };
    for kind in ["discovery", "tui"] {
        assert_eq!(run(kind).status.code(), Some(23));
    }
    assert!(!home.join("acquired").exists());
    // New, resumed and forked adapters each construct a fresh server.
    for _ in 0..3 {
        let output = run("session");
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("app-server\n--listen\nstdio://"));
        assert!(!stdout.contains(TOKEN));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
    }
    assert_eq!(
        fs::read_to_string(home.join("acquired")).unwrap(),
        "acquire\n".repeat(3)
    );
    fs::write(home.join(".codex/config.toml"), "").unwrap();
    assert_eq!(run("session").status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(home.join("acquired")).unwrap(),
        "acquire\n".repeat(3)
    );
    fs::write(home.join(".codex/config.toml"), CONFIG).unwrap();
    script(&home.join("bin/gh"), &format!("echo '{TOKEN}' >&2; exit 1"));
    let output = run("session");
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
    assert!(String::from_utf8_lossy(&output.stderr).contains("relaunch or resume"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("app-server"));
}
