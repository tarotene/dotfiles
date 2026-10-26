use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        script(
            &f.home().join(".local/bin/codex"),
            r#"
if [ "$1" = '--version' ]; then echo 'codex-cli 0.164.0'; exit; fi
test "${GH_TOKEN:-}" = "ghp_0000000000000000000000000000000000000000" || exit 80
test -z "${GITHUB_TOKEN:-}" || exit 81
printf '%s\n' "$@"
exit 23"#,
        );
        script(
            &f.home().join(".local/bin/agent-permissions"),
            "echo check >> \"$HOME/order\"; exit 0",
        );
        script(
            &f.home().join("bin/gh"),
            r#"echo auth >> "$HOME/order"; printf 'ghp_0000000000000000000000000000000000000000\n'"#,
        );
        f
    }
    fn home(&self) -> &Path {
        self.dir.path()
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_codex-launcher"));
        c.env_clear()
            .env("HOME", self.home())
            .env("PATH", self.home().join("bin"))
            .current_dir(self.home());
        c
    }
    fn order(&self) -> String {
        fs::read_to_string(self.home().join("order")).unwrap_or_default()
    }
}
fn no_secret(o: &Output) {
    assert!(!String::from_utf8_lossy(&o.stdout)
        .contains("ghp_0000000000000000000000000000000000000000"));
    assert!(!String::from_utf8_lossy(&o.stderr)
        .contains("ghp_0000000000000000000000000000000000000000"));
}

#[test]
fn passes_args_exit_code_and_checks_before_auth_without_shell_export() {
    let f = Fixture::new();
    let o = f
        .cmd()
        .env("GITHUB_TOKEN", "inherited")
        .args(["exec", "--json", "--", "a prompt with spaces; $x"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(23));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "exec\n--json\n--\na prompt with spaces; $x\n"
    );
    assert_eq!(f.order(), "check\nauth\n");
    no_secret(&o);
}

#[test]
fn refuses_overrides_and_unknown_flags_before_auth() {
    for args in [
        vec!["-c", "features.credential_masking=false"],
        vec!["exec", "--ignore-user-config"],
        vec!["--yolo"],
        vec!["--profile=x"],
        vec!["-pfoo"],
        vec!["--disable", "network_proxy"],
        vec!["--sandbox=read-only"],
        vec!["--add-dir", "/"],
        vec!["--unknown"],
        vec!["--model", "--yolo"],
        vec!["exec-server"],
        vec!["sandbox", "--sandbox-state-json", "{}"],
        vec!["--model", "x", "app-server"],
    ] {
        let f = Fixture::new();
        let o = f.cmd().args(&args).output().unwrap();
        assert!(!o.status.success(), "{args:?}");
        assert!(f.order().is_empty(), "{args:?}");
        no_secret(&o);
    }
    for key in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "CODEX_PROFILE"] {
        let f = Fixture::new();
        assert!(!f
            .cmd()
            .env(key, "/tmp/alternate")
            .output()
            .unwrap()
            .status
            .success());
        assert!(f.order().is_empty());
    }
}

#[test]
fn check_and_auth_failures_never_launch_or_print_credentials() {
    let f = Fixture::new();
    script(&f.home().join(".local/bin/agent-permissions"), "exit 1");
    let o = f.cmd().output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("permission check failed"));
    assert!(f.order().is_empty());
    for body in [
        "echo ghp_0000000000000000000000000000000000000000 >&2; exit 1",
        "printf ''",
        "printf 'bad credential'",
        "printf '\\377'",
    ] {
        let f = Fixture::new();
        script(&f.home().join("bin/gh"), body);
        let o = f.cmd().output().unwrap();
        assert_eq!(o.status.code(), Some(1));
        no_secret(&o);
    }
}

#[test]
fn maintenance_delegates_without_any_github_credentials_or_check() {
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["completion", "zsh"],
        vec!["update"],
        vec!["login", "status"],
        vec!["logout"],
        vec!["help", "exec"],
        vec!["exec", "--help"],
        vec!["resume", "-h"],
    ] {
        let f = Fixture::new();
        script(&f.home().join(".local/bin/codex"), "test -z \"${GH_TOKEN:-}${GITHUB_TOKEN:-}${GH_ENTERPRISE_TOKEN:-}${GITHUB_ENTERPRISE_TOKEN:-}\" || exit 81; exit 42");
        let o = f
            .cmd()
            .env("GH_TOKEN", "inherited")
            .env("GITHUB_TOKEN", "inherited")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(42));
        assert!(f.order().is_empty());
    }
}

#[test]
fn rejects_recursive_native_and_unreviewed_version() {
    let f = Fixture::new();
    fs::remove_file(f.home().join(".local/bin/codex")).unwrap();
    symlink(
        env!("CARGO_BIN_EXE_codex-launcher"),
        f.home().join(".local/bin/codex"),
    )
    .unwrap();
    let o = f.cmd().output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("points back"));
    assert!(f.order().is_empty());
    let f = Fixture::new();
    script(
        &f.home().join(".local/bin/codex"),
        "echo 'codex-cli 0.165.0'",
    );
    let o = f.cmd().output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("unreviewed Codex version"));
    assert!(f.order().is_empty());
}

#[test]
fn all_session_routes_attach_credentials_and_cd_is_checked() {
    for args in [
        vec![],
        vec!["resume", "--last"],
        vec!["fork", "--last"],
        vec!["review", "--uncommitted"],
        vec!["exec", "resume", "--last"],
        vec!["sandbox", "--", "sh", "-c", "true"],
    ] {
        let f = Fixture::new();
        let o = f.cmd().args(args).output().unwrap();
        assert_eq!(o.status.code(), Some(23));
        assert_eq!(f.order(), "check\nauth\n");
    }
    let f = Fixture::new();
    let other = f.home().join("other");
    fs::create_dir(&other).unwrap();
    script(
        &f.home().join(".local/bin/agent-permissions"),
        "test \"$1\" = check && test \"$2\" = --project && test \"$3\" = \"$HOME/other\"",
    );
    assert_eq!(
        f.cmd()
            .args(["--cd", "other"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(23)
    );
}
