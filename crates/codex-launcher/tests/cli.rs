const PROTECTED: &str = r#"default_permissions = "native"
approval_policy = "on-request"
approvals_reviewer = "auto_review"
[features]
network_proxy = true
credential_masking = true
[permissions.native]
extends = ":workspace"
[permissions.native.network]
enabled = true
"#;
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
            &r#"
if [ "$1" = '--version' ]; then echo 'codex-cli 0.165.0'; exit; fi
test "${GH_TOKEN:-}" = "ghp_0000000000000000000000000000000000000000" || exit 80
test -z "${GITHUB_TOKEN:-}" || exit 81
printf '%s\n' "$@"
exit 23"#
                .replace("0.165.0", &codex_launcher::reviewed().unwrap().0),
        );
        fs::create_dir_all(f.home().join(".codex")).unwrap();
        fs::write(f.home().join(".codex/config.toml"), PROTECTED).unwrap();
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
        "--config\ndefault_permissions=\"native\"\nexec\n--json\n--\na prompt with spaces; $x\n"
    );
    assert_eq!(f.order(), "auth\n");
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
    fs::write(f.home().join(".codex/config.toml"), "").unwrap();
    let o = f.cmd().output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("native protective config"));
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
fn update_uses_reviewed_installer_and_rejects_failure_or_wrong_version() {
    let (version, commit) = codex_launcher::reviewed().unwrap();
    for (installer_exit, installed, success) in [
        (0, version.as_str(), true),
        (9, version.as_str(), false),
        (0, "0.0.0", false),
    ] {
        let f = Fixture::new();
        script(
            &f.home().join("bin/curl"),
            &format!(
                r#"
test "$1" = -q && test "$2" = -fsSL || exit 31
test "$3" = 'https://raw.githubusercontent.com/openai/codex/{commit}/scripts/install/install.sh' || exit 32
test "$4" = -o || exit 33
printf '%s\n' '#!/bin/sh' 'test "$1" = --release && test "$2" = "{version}" || exit 34' 'test -z "${{GH_TOKEN:-}}${{CODEX_RELEASE:-}}${{CODEX_INSTALL_IF_LATEST:-}}" || exit 35' 'test "$HTTPS_PROXY" = http://127.0.0.1:1234 || exit 36' 'exit {installer_exit}' > "$5"
"#
            ),
        );
        script(
            &f.home().join(".local/bin/codex"),
            &format!("echo 'codex-cli {installed}'"),
        );
        // The test PATH normally contains only fixtures; sh is needed to execute the installer.
        let path = format!("{}:/bin:/usr/bin", f.home().join("bin").display());
        let output = f
            .cmd()
            .env("PATH", path)
            .env("GH_TOKEN", "secret")
            .env("CODEX_RELEASE", "latest")
            .env("CODEX_INSTALL_IF_LATEST", "1")
            .env("HTTPS_PROXY", "http://127.0.0.1:1234")
            .arg("update")
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert!(f.order().is_empty());
    }
    let f = Fixture::new();
    script(&f.home().join("bin/curl"), "exit 7");
    let output = f.cmd().arg("update").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("download failed"));
    assert!(f.order().is_empty());
    assert!(!f
        .cmd()
        .args(["update", "--release", "latest"])
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn update_rejects_proxy_credentials_and_missing_native_before_download() {
    for key in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
        let f = Fixture::new();
        script(
            &f.home().join("bin/curl"),
            "echo called > \"$HOME/downloaded\"; exit 0",
        );
        let output = f
            .cmd()
            .env(key, "http://user:secret@example.invalid:8080")
            .arg("update")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!f.home().join("downloaded").exists());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
    }
    for remove in [true, false] {
        let f = Fixture::new();
        script(
            &f.home().join("bin/curl"),
            "echo called > \"$HOME/downloaded\"; exit 0",
        );
        let native = f.home().join(".local/bin/codex");
        if remove {
            fs::remove_file(&native).unwrap();
        } else {
            fs::set_permissions(&native, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let output = f.cmd().arg("update").output().unwrap();
        assert!(!output.status.success());
        assert!(!f.home().join("downloaded").exists());
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
    script(&f.home().join(".local/bin/codex"), "echo 'codex-cli 0.0.0'");
    let o = f.cmd().output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("unreviewed Codex version"));
    // 止まるときは、検出した版・レビュー済みの版・直し方が分かる。
    assert!(err.contains("codex-cli 0.0.0"), "{err}");
    assert!(
        err.contains(&format!(
            "codex-cli {}",
            codex_launcher::reviewed().unwrap().0
        )),
        "{err}"
    );
    assert!(err.contains("reviewed.toml"), "{err}");
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
        assert_eq!(f.order(), "auth\n");
    }
    let f = Fixture::new();
    let other = f.home().join("other");
    fs::create_dir(&other).unwrap();
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

#[test]
fn native_protection_changes_are_rejected_before_credentials() {
    for config in [
        PROTECTED.replace("credential_masking = true", "credential_masking = false"),
        PROTECTED.replace("on-request", "never"),
        PROTECTED.replace("auto_review", "user"),
        PROTECTED.replace("network_proxy = true", "network_proxy = false"),
        PROTECTED.replace("enabled = true", "enabled = false"),
        PROTECTED.replace(
            "enabled = true",
            "enabled = true\ndangerously_allow_all_unix_sockets = true",
        ),
        PROTECTED.replace(":workspace", ":danger-full-access"),
        PROTECTED.replace(
            "default_permissions = \"native\"",
            "default_permissions = \"missing\"",
        ),
        format!("sandbox_mode = 'danger-full-access'\n{PROTECTED}"),
        format!("{PROTECTED}\n[profiles.unsafe.features]\ncredential_masking = false\n"),
    ] {
        let f = Fixture::new();
        fs::write(f.home().join(".codex/config.toml"), config).unwrap();
        let o = f.cmd().output().unwrap();
        assert!(!o.status.success());
        assert!(
            f.order().is_empty(),
            "credential acquisition must follow validation"
        );
        no_secret(&o);
    }
}

#[test]
fn project_overrides_are_rejected_but_unrelated_worktree_state_is_ignored() {
    let f = Fixture::new();
    let project = f.home().join("repo/nested");
    fs::create_dir_all(project.join(".codex")).unwrap();
    let local = project.join(".codex/config.toml");
    for config in [
        "sandbox_mode = 'danger-full-access'",
        "[features]\ncredential_masking = false",
        "[permissions.native.network]\nenabled = false",
        "[profiles.other]\ndefault_permissions = 'unsafe'",
    ] {
        fs::write(&local, config).unwrap();
        assert!(!f
            .cmd()
            .arg("--cd")
            .arg(&project)
            .output()
            .unwrap()
            .status
            .success());
        assert!(f.order().is_empty());
    }
    fs::write(&local, "model = 'example-model'").unwrap();
    let other = f.home().join("other/.claude");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("settings.local.json"), "").unwrap();
    assert_eq!(
        f.cmd()
            .arg("--cd")
            .arg(&project)
            .output()
            .unwrap()
            .status
            .code(),
        Some(23)
    );
    fs::remove_dir_all(f.home().join("other")).unwrap();
    assert_eq!(
        f.cmd()
            .arg("--cd")
            .arg(&project)
            .output()
            .unwrap()
            .status
            .code(),
        Some(23)
    );
}
