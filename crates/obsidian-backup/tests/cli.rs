use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
struct Test {
    dir: tempfile::TempDir,
}
impl Test {
    fn new() -> Self {
        let t = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir(t.path().join("vault")).unwrap();
        fs::create_dir(t.path().join("vault/acceptance")).unwrap();
        fs::write(t.path().join("vault/acceptance/file"), b"round trip\n").unwrap();
        script(
            &t.path().join("restic"),
            r#"echo "$*" >> "$TEST_LOG"
if [ "$1" = restore ]; then
  while [ "$1" != --target ]; do shift; done
  cp -R "$TEST_VAULT"/. "$2"/
  [ "${FAIL_RESTORE:-0}" = 0 ] || exit 9
fi"#,
        );
        script(
            &t.path().join("secret-tool"),
            "[ \"$1\" = lookup ]\nprintf test-machine-token",
        );
        script(
            &t.path().join("bws"),
            r#"[ "$BWS_ACCESS_TOKEN" = test-machine-token ]
[ "$1" = run ] && [ "$2" = --no-inherit-env ] && [ "$3" = -- ]
shift 3
export RESTIC_REPOSITORY=test RESTIC_PASSWORD=test AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test
exec "$@""#,
        );
        script(
            &t.path().join("herdr"),
            "printf '{\"reason\":\"disabled\"}'",
        );
        t
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_obsidian-backup"));
        c.env("OBSIDIAN_BACKUP_HOST", "test-host")
            .env("OBSIDIAN_BACKUP_VAULT", self.path().join("vault"))
            .env("OBSIDIAN_BACKUP_RESTIC_BIN", self.path().join("restic"))
            .env("OBSIDIAN_BACKUP_BWS_BIN", self.path().join("bws"))
            .env(
                "OBSIDIAN_BACKUP_SECRET_TOOL_BIN",
                self.path().join("secret-tool"),
            )
            .env("OBSIDIAN_BACKUP_HERDR_BIN", self.path().join("herdr"))
            .env("TEST_LOG", self.path().join("log"))
            .env("TEST_VAULT", self.path().join("vault"))
            .env("RESTIC_REPOSITORY", "test")
            .env("RESTIC_PASSWORD", "test")
            .env("AWS_ACCESS_KEY_ID", "test")
            .env("AWS_SECRET_ACCESS_KEY", "test");
        c
    }
}
#[test]
fn operations_and_restore() {
    let t = Test::new();
    for args in [
        vec!["__run", "init"],
        vec!["__run", "backup"],
        vec!["__run", "maintenance"],
        vec!["__run", "restore-test", "acceptance/file"],
    ] {
        let out = t.cmd().args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let log = fs::read_to_string(t.path().join("log")).unwrap();
    assert!(log.contains("init\n"));
    assert!(log.contains("backup --host test-host --tag obsidian-personal"));
    assert!(log.contains("forget --host test-host --tag obsidian-personal --keep-daily 7 --keep-weekly 5 --keep-monthly 12 --prune"));
    assert!(log.contains("check\n"));
    assert!(log.contains(&format!(
        "restore latest:{} --host test-host",
        t.path().join("vault").display()
    )));
}
#[test]
fn refuses_failed_restore_even_if_files_exist_and_unsafe_paths() {
    let t = Test::new();
    for args in [
        vec!["__run", "restore-test", "../outside"],
        vec!["__run", "restore-test"],
        vec!["__run", "restore-test", "acceptance/file"],
    ] {
        let out = t
            .cmd()
            .env("FAIL_RESTORE", "1")
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
    }
}
#[test]
fn missing_secret_and_disabled_notification_fail() {
    let t = Test::new();
    let out = t
        .cmd()
        .env_remove("RESTIC_PASSWORD")
        .args(["__run", "backup"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let out = t
        .cmd()
        .env("OBSIDIAN_BACKUP_SECRET_TOOL_BIN", "/bin/false")
        .args(["backup", "--notify"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("reason=disabled"));
}

#[test]
fn backup_acquires_token_and_runs_through_bws() {
    let t = Test::new();
    let out = t.cmd().arg("backup").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(fs::read_to_string(t.path().join("log"))
        .unwrap()
        .contains("backup --host test-host"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("test-machine-token"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("test-machine-token"));
}

#[test]
fn legacy_cli_selftest_and_exit_codes_are_preserved() {
    let t = Test::new();
    let out = t.cmd().arg("--selftest").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("obsidian-backup selftest passed"));
    for args in [
        vec![],
        vec!["configure-token", "unexpected"],
        vec!["__run", "restore-test"],
    ] {
        assert_eq!(t.cmd().args(args).output().unwrap().status.code(), Some(2));
    }
    let out = t
        .cmd()
        .env("FAIL_RESTORE", "1")
        .args(["__run", "restore-test", "acceptance/file"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn successful_notification_suppresses_disabled_warning() {
    let t = Test::new();
    script(
        &t.path().join("herdr"),
        "printf '{\"shown\":true,\"reason\":\"disabled\"}'",
    );
    let out = t
        .cmd()
        .env("OBSIDIAN_BACKUP_SECRET_TOOL_BIN", "/bin/false")
        .args(["backup", "--notify"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("reason=disabled"));
}

#[test]
fn bws_failure_keeps_exit_code_and_clear_token_uses_same_attributes() {
    let t = Test::new();
    script(&t.path().join("bws"), "exit 9");
    assert_eq!(
        t.cmd()
            .args(["backup", "--notify"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(9)
    );
    script(
        &t.path().join("secret-tool"),
        "[ \"$*\" = 'clear application obsidian-backup credential bws-access-token' ]",
    );
    assert!(t
        .cmd()
        .arg("clear-token")
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn jq_and_find_overrides_are_honored() {
    let t = Test::new();
    let out = t
        .cmd()
        .env("OBSIDIAN_BACKUP_JQ_BIN", "/bin/false")
        .env("OBSIDIAN_BACKUP_SECRET_TOOL_BIN", "/bin/false")
        .args(["backup", "--notify"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("reason=disabled"));
    script(
        &t.path().join("find"),
        "printf '%s\\n' \"$*\" > \"$TEST_LOG.find\"\nexec /usr/bin/find \"$@\"",
    );
    let out = t
        .cmd()
        .env("OBSIDIAN_BACKUP_FIND_BIN", t.path().join("find"))
        .args(["__run", "restore-test", "acceptance/file"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(fs::read_to_string(t.path().join("log.find"))
        .unwrap()
        .contains("-depth -delete"));
}

#[test]
fn bws_with_clean_environment_keeps_only_explicit_runtime_inputs() {
    let t = Test::new();
    let log = t.path().join("clean-bws.log");
    script(
        &t.path().join("restic"),
        &format!("printf '%s\\n' \"$*\" > '{}'", log.display()),
    );
    script(
        &t.path().join("bws"),
        r#"[ "$BWS_ACCESS_TOKEN" = test-machine-token ]
[ "$1" = run ] && [ "$2" = --no-inherit-env ] && [ "$3" = -- ]
shift 3
exec /usr/bin/env -i RESTIC_REPOSITORY=test RESTIC_PASSWORD=test AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test "$@""#,
    );
    let path = format!("{}:/usr/bin:/bin", t.path().display());
    let out = t.cmd().env("PATH", path).arg("backup").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = fs::read_to_string(log).unwrap();
    assert!(log.contains("backup --host test-host --tag obsidian-personal"));
    assert!(log.contains(&t.path().join("vault").display().to_string()));
}
