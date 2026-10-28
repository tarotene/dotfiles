//! Backup one Obsidian vault through the restic and BWS command line interfaces.
use backup_runtime::{arg_env, capture, checked, command, keyring_lookup, nonempty_env, status_ok};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
mod selftest;

const APP: &str = "obsidian-backup";
const CREDENTIAL: &str = "bws-access-token";
const TAG: &str = "obsidian-personal";

fn configured(name: &str, default: OsString) -> OsString {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
}
fn vault() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME missing")?;
    Ok(configured(
        "OBSIDIAN_BACKUP_VAULT",
        PathBuf::from(home)
            .join("Documents/obsidian/personal")
            .into_os_string(),
    )
    .into())
}
fn host() -> Result<OsString, String> {
    if let Some(host) = std::env::var_os("OBSIDIAN_BACKUP_HOST").filter(|v| !v.is_empty()) {
        return Ok(host);
    }
    let output = capture(&mut Command::new("hostname"))?;
    status_ok(output.status)?;
    let host = String::from_utf8(output.stdout).map_err(|_| "invalid hostname")?;
    Ok(host.trim_end().into())
}
fn restic() -> Command {
    command("OBSIDIAN_BACKUP_RESTIC_BIN", "restic")
}
fn vault_exists(vault: &Path) -> Result<(), String> {
    if vault.is_dir() {
        Ok(())
    } else {
        Err(format!("vault does not exist: {}", vault.display()))
    }
}
struct RestoreDir(tempfile::TempDir);
impl Drop for RestoreDir {
    fn drop(&mut self) {
        // Keep the deployed cleanup override while TempDir also handles early returns.
        let _ = command("OBSIDIAN_BACKUP_FIND_BIN", "find")
            .arg(self.0.path())
            .args(["-depth", "-delete"])
            .status();
    }
}
fn restore(vault: &Path, host: &OsString, paths: &[OsString]) -> Result<(), String> {
    vault_exists(vault)?;
    if paths.is_empty() {
        return Err("usage: restore-test requires at least one vault-relative path".into());
    }
    for path in paths {
        let path = Path::new(path);
        if path.as_os_str().is_empty()
            || path.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!("unsafe vault-relative path: {}", path.display()));
        }
        if !vault.join(path).is_file() {
            return Err(format!("source fixture is missing: {}", path.display()));
        }
    }
    let tmp = RestoreDir(
        tempfile::tempdir().map_err(|e| format!("cannot create restore directory: {e}"))?,
    );
    // restic v0.18.1 supports snapshot:subfolder; restore the vault root so
    // metadata for its ancestors is not restored. https://restic.readthedocs.io/en/v0.18.1/050_restore.html (retrieved 2026-10-28).
    let mut snapshot = OsString::from("latest:");
    snapshot.push(vault);
    checked(
        restic()
            .arg("restore")
            .arg(snapshot)
            .arg("--host")
            .arg(host)
            .args(["--tag", TAG, "--target"])
            .arg(tmp.0.path())
            .args(["--exclude-xattr", "system.posix_acl_*"]),
    )?;
    for path in paths {
        let source = std::fs::read(vault.join(path))
            .map_err(|e| format!("cannot read source fixture: {e}"))?;
        let restored = std::fs::read(tmp.0.path().join(path))
            .map_err(|e| format!("cannot read restored fixture: {e}"))?;
        if source != restored {
            return Err(format!(
                "restored fixture differs: {}",
                Path::new(path).display()
            ));
        }
    }
    println!("restore test passed for {} fixture(s)", paths.len());
    Ok(())
}
fn inner(operation: &str, paths: &[OsString]) -> Result<(), String> {
    for name in [
        "RESTIC_REPOSITORY",
        "RESTIC_PASSWORD",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
    ] {
        nonempty_env(name)?;
    }
    let host = host()?;
    match operation {
        "init" => checked(restic().arg("init")),
        "backup" => {
            let vault = vault()?;
            vault_exists(&vault)?;
            checked(
                restic()
                    .arg("backup")
                    .arg("--host")
                    .arg(host)
                    .args(["--tag", TAG])
                    .arg(vault),
            )
        }
        "maintenance" => {
            checked(restic().arg("forget").arg("--host").arg(host).args([
                "--tag",
                TAG,
                "--keep-daily",
                "7",
                "--keep-weekly",
                "5",
                "--keep-monthly",
                "12",
                "--prune",
            ]))?;
            checked(restic().arg("check"))
        }
        "restore-test" => restore(&vault()?, &host, paths),
        _ => Err("usage: unknown inner operation".into()),
    }
}
fn with_bws(operation: &str, paths: &[OsString]) -> Result<(), String> {
    let token = keyring_lookup(APP, CREDENTIAL, "OBSIDIAN_BACKUP_SECRET_TOOL_BIN")
        .map_err(|_| "Bitwarden machine token is missing from the login keyring; run: obsidian-backup configure-token")?;
    let mut bws = command("OBSIDIAN_BACKUP_BWS_BIN", "bws");
    bws.env("BWS_ACCESS_TOKEN", token)
        .args(["run", "--no-inherit-env", "--"]);
    bws.arg(configured("OBSIDIAN_BACKUP_ENV_BIN", "env".into()));
    for key in ["HOME", "PATH"] {
        arg_env(
            &mut bws,
            key,
            &std::env::var_os(key).ok_or(format!("{key} missing"))?,
        );
    }
    arg_env(&mut bws, "OBSIDIAN_BACKUP_VAULT", vault()?.as_os_str());
    arg_env(&mut bws, "OBSIDIAN_BACKUP_HOST", &host()?);
    bws.arg(std::env::current_exe().map_err(|e| format!("cannot resolve executable: {e}"))?)
        .args(["__run", operation])
        .args(paths);
    checked(&mut bws)
}
fn jq_matches(response: &[u8], filter: &str) -> bool {
    let Ok(mut child) = command("OBSIDIAN_BACKUP_JQ_BIN", "jq")
        .args(["-e", filter])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(response);
    }
    child.wait().is_ok_and(|status| status.success())
}
fn notify_failure(operation: &str) {
    let body = format!("{operation} failed; inspect systemctl --user status obsidian-backup*");
    if let Ok(out) = capture(
        command("OBSIDIAN_BACKUP_HERDR_BIN", "herdr")
            .args([
                "notification",
                "show",
                "Obsidian backup failed",
                "--body",
                &body,
                "--sound",
                "request",
            ])
            .stderr(Stdio::null()),
    ) {
        if out.status.success()
            && !jq_matches(
                &out.stdout,
                ".. | objects | select(has(\"shown\")) | .shown == true",
            )
            && jq_matches(
                &out.stdout,
                ".. | objects | select(has(\"reason\")) | .reason == \"disabled\"",
            )
        {
            eprintln!("obsidian-backup: herdr notification delivery is disabled (reason=disabled) — {operation} failed with no toast delivered. config/herdr/config.toml の [ui.toast] delivery を設定してください。");
        }
    }
}
fn configure_token() -> Result<(), String> {
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("cannot open terminal: {e}"))?;
    write!(tty, "Bitwarden Secrets Manager access token: ").map_err(|e| e.to_string())?;
    tty.flush().map_err(|e| e.to_string())?;
    // Use the same silent-read primitive as the former shell implementation;
    // the token travels over a pipe, never in argv or the environment.
    let read = Command::new("bash")
        .args([
            "-c",
            "IFS= read -r -s token || exit; printf '%s' \"$token\"",
        ])
        .stdin(tty.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    writeln!(tty).map_err(|e| e.to_string())?;
    status_ok(read.status)?;
    let token = read.stdout;
    if token.is_empty() {
        return Err("refusing to store an empty access token".into());
    }
    let mut child = command("OBSIDIAN_BACKUP_SECRET_TOOL_BIN", "secret-tool")
        .args([
            "store",
            "--label=Obsidian backup Bitwarden machine token",
            "application",
            APP,
            "credential",
            CREDENTIAL,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("cannot open secret-tool stdin")?
        .write_all(&token)
        .map_err(|e| e.to_string())?;
    status_ok(child.wait().map_err(|e| e.to_string())?)
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let op = args.next().ok_or("usage: operation required")?;
    let op = op.to_str().ok_or("invalid operation")?;
    let mut rest: Vec<OsString> = args.collect();
    match op {
        "--selftest" => selftest::run(),
        "configure-token" if rest.is_empty() => configure_token(),
        "clear-token" if rest.is_empty() => checked(command("OBSIDIAN_BACKUP_SECRET_TOOL_BIN", "secret-tool").args(["clear", "application", APP, "credential", CREDENTIAL])),
        "__run" if !rest.is_empty() => { let inner_op = rest.remove(0).into_string().map_err(|_| "invalid inner operation")?; inner(&inner_op, &rest) },
        "init" | "backup" | "maintenance" | "restore-test" => {
            let notify = rest.first().is_some_and(|a| a == "--notify"); if notify { rest.remove(0); }
            let result = with_bws(op, &rest); if result.is_err() && notify { notify_failure(op); } result
        },
        _ => Err("usage: obsidian-backup configure-token|clear-token|init|backup|maintenance|restore-test".into()),
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("obsidian-backup: {e}");
            let code = if e.starts_with("usage:") {
                2
            } else {
                e.strip_prefix("command failed (exit code ")
                    .and_then(|s| s.strip_suffix(')'))
                    .and_then(|s| s.parse::<u8>().ok())
                    .filter(|&code| code != 0)
                    .unwrap_or(1)
            };
            ExitCode::from(code)
        }
    }
}
