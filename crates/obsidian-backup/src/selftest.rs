//! Offline selftest with fake external services, matching the deployed CLI contract.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn script(path: &Path, body: &str) -> Result<(), String> {
    std::fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).map_err(|e| e.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())
}

pub fn run() -> Result<(), String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let dir = tmp.path();
    let vault = dir.join("vault");
    std::fs::create_dir_all(vault.join("acceptance")).map_err(|e| e.to_string())?;
    std::fs::write(vault.join("acceptance/roundtrip.md"), b"round trip\n")
        .map_err(|e| e.to_string())?;
    script(
        &dir.join("restic"),
        r#"
echo "$*" >> "$TEST_RESTIC_LOG"
if [ "$1" = restore ]; then
  while [ "$1" != --target ]; do shift; done
  cp -R "$TEST_VAULT"/. "$2"/
fi"#,
    )?;
    script(
        &dir.join("secret-tool"),
        "[ \"$1\" = lookup ]\nprintf test-machine-token",
    )?;
    script(
        &dir.join("bws"),
        r#"
[ "$BWS_ACCESS_TOKEN" = test-machine-token ]
[ "$1" = run ] && [ "$2" = --no-inherit-env ] && [ "$3" = -- ]
shift 3
export AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test RESTIC_PASSWORD=test RESTIC_REPOSITORY=test
exec "$@""#,
    )?;
    script(&dir.join("herdr"), "printf '{\"reason\":\"disabled\"}'")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let run = |args: &[&str], missing_token: bool| -> Result<std::process::Output, String> {
        Command::new(&exe)
            .args(args)
            .env("OBSIDIAN_BACKUP_HOST", "selftest")
            .env("OBSIDIAN_BACKUP_RESTIC_BIN", dir.join("restic"))
            .env("OBSIDIAN_BACKUP_BWS_BIN", dir.join("bws"))
            .env(
                "OBSIDIAN_BACKUP_SECRET_TOOL_BIN",
                if missing_token {
                    std::path::PathBuf::from("/bin/false")
                } else {
                    dir.join("secret-tool")
                },
            )
            .env("OBSIDIAN_BACKUP_HERDR_BIN", dir.join("herdr"))
            .env("OBSIDIAN_BACKUP_VAULT", &vault)
            .env("TEST_RESTIC_LOG", dir.join("restic.log"))
            .env("TEST_VAULT", &vault)
            .env("AWS_ACCESS_KEY_ID", "test")
            .env("AWS_SECRET_ACCESS_KEY", "test")
            .env("RESTIC_PASSWORD", "test")
            .env("RESTIC_REPOSITORY", "test")
            .output()
            .map_err(|e| e.to_string())
    };
    for args in [
        &["__run", "init"][..],
        &["__run", "backup"],
        &["__run", "maintenance"],
        &["__run", "restore-test", "acceptance/roundtrip.md"],
        &["backup"],
    ] {
        let out = run(args, false)?;
        if !out.status.success() {
            return Err(format!(
                "selftest failed: {:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    if run(&["__run", "restore-test", "../outside"], false)?
        .status
        .success()
    {
        return Err("selftest: unsafe path accepted".into());
    }
    let fail = run(&["backup", "--notify"], true)?;
    if fail.status.success() || !String::from_utf8_lossy(&fail.stderr).contains("reason=disabled") {
        return Err("selftest: failed notification not reported".into());
    }
    let log = std::fs::read_to_string(dir.join("restic.log")).map_err(|e| e.to_string())?;
    for verb in [
        "init",
        "backup --host",
        "forget --host",
        "check",
        "restore latest:",
    ] {
        if !log.lines().any(|line| line.starts_with(verb)) {
            return Err(format!("selftest: missing {verb}"));
        }
    }
    println!("obsidian-backup selftest passed");
    Ok(())
}
