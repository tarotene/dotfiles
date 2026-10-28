//! Staging primitives. Errors deliberately omit private paths.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
const SERVICE: &str = "app-fcitx5@autostart.service";
const RECOVERY_MARKER: &str = "mozc-input-service-was-active";

fn input_status(systemctl: &Path, action: &str) -> Result<std::process::ExitStatus, &'static str> {
    Command::new(systemctl)
        .args(["--user", action, SERVICE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "cannot manage input service")
}

/// Re-establish the service after an interrupted previous run. The durable
/// marker is deleted only after systemd reports it active again.
pub fn recover_mozc(systemctl: &Path, state: &Path) -> Result<(), &'static str> {
    let marker = state.join(RECOVERY_MARKER);
    let metadata = match std::fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("cannot inspect input recovery marker"),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("invalid input recovery marker");
    }
    if !input_status(systemctl, "start")?.success()
        || !input_status(systemctl, "is-active")?.success()
    {
        return Err("cannot restore input service");
    }
    std::fs::remove_file(marker).map_err(|_| "cannot remove input recovery marker")?;
    File::open(state)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "cannot sync input recovery state")?;
    Ok(())
}

fn mark_input_recovery(state: &Path) -> Result<(), &'static str> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(state.join(RECOVERY_MARKER))
        .map_err(|_| "cannot create input recovery marker")?;
    file.write_all(b"active\n")
        .map_err(|_| "cannot write input recovery marker")?;
    file.sync_all()
        .map_err(|_| "cannot sync input recovery marker")?;
    File::open(state)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "cannot sync input recovery state")?;
    Ok(())
}
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

struct SignalGuard {
    old_term: libc::sigaction,
    old_int: libc::sigaction,
}
impl SignalGuard {
    fn install() -> Result<Self, &'static str> {
        INTERRUPTED.store(false, Ordering::SeqCst);
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = interrupt as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut old_term: libc::sigaction = std::mem::zeroed();
            let mut old_int: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGTERM, &action, &mut old_term) != 0 {
                return Err("cannot protect input service against interruption");
            }
            if libc::sigaction(libc::SIGINT, &action, &mut old_int) != 0 {
                libc::sigaction(libc::SIGTERM, &old_term, std::ptr::null_mut());
                return Err("cannot protect input service against interruption");
            }
            Ok(Self { old_term, old_int })
        }
    }
}
impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGTERM, &self.old_term, std::ptr::null_mut());
            libc::sigaction(libc::SIGINT, &self.old_int, std::ptr::null_mut());
        }
    }
}

fn copy_tree(source: &Path, destination: &Path, device: u64) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    if INTERRUPTED.load(Ordering::SeqCst) {
        return Err("Mozc staging interrupted");
    }
    let meta = std::fs::symlink_metadata(source).map_err(|_| "cannot inspect Mozc source")?;
    if meta.file_type().is_symlink() || meta.dev() != device {
        return Err("unsafe Mozc source");
    }
    if meta.is_dir() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(destination)
            .map_err(|_| "cannot create Mozc staging directory")?;
        for entry in std::fs::read_dir(source).map_err(|_| "cannot read Mozc source")? {
            let entry = entry.map_err(|_| "cannot read Mozc source")?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()), device)?;
        }
    } else if meta.is_file() {
        let mut input = File::open(source).map_err(|_| "cannot open Mozc source")?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(destination)
            .map_err(|_| "cannot create Mozc staging")?;
        let mut buffer = [0u8; 65536];
        loop {
            if INTERRUPTED.load(Ordering::SeqCst) {
                return Err("Mozc staging interrupted");
            }
            let n = input
                .read(&mut buffer)
                .map_err(|_| "cannot read Mozc staging")?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buffer[..n])
                .map_err(|_| "cannot copy Mozc staging")?;
        }
        output.sync_all().map_err(|_| "cannot sync Mozc staging")?;
    } else {
        return Err("unsupported Mozc source");
    }
    Ok(())
}

/// Quiesce the user input service and its Mozc server, then restore its prior
/// state even when the copy fails.
pub fn mozc(
    systemctl: &Path,
    pkill: &Path,
    pgrep: &Path,
    source: &Path,
    destination: &Path,
    state: &Path,
) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let _signals = SignalGuard::install()?;
    let initial = input_status(systemctl, "is-active")?;
    let was_active = if initial.success() {
        true
    } else if initial.code() == Some(3) {
        false
    } else {
        return Err("input service state is unknown");
    };
    if was_active {
        mark_input_recovery(state)?;
    }
    if was_active && !input_status(systemctl, "stop").is_ok_and(|status| status.success()) {
        recover_mozc(systemctl, state)?;
        return Err("cannot stop input service");
    }
    let result = (|| {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Err("Mozc staging interrupted");
        }
        let uid = unsafe { libc::geteuid() }.to_string();
        if was_active {
            let stop = Command::new(pkill)
                .args(["-u", &uid, "-x", "mozc_server"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|_| "cannot stop Mozc")?;
            if !stop.success() && stop.code() != Some(1) {
                return Err("cannot stop Mozc");
            }
        }
        let running = Command::new(pgrep)
            .args(["-u", &uid, "-x", "mozc_server"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "cannot inspect Mozc")?;
        if running.success() || running.code() != Some(1) {
            return Err("Mozc is still running or its state is unknown");
        }
        let metadata = std::fs::symlink_metadata(source).map_err(|_| "missing Mozc source")?;
        copy_tree(source, destination, metadata.dev())
    })();
    if was_active {
        recover_mozc(systemctl, state)?;
    }
    if INTERRUPTED.load(Ordering::SeqCst) {
        return Err("Mozc staging interrupted");
    }
    result
}

/// Use SQLite's online backup API through its CLI, including WAL contents.
/// The destination must be in a newly created private staging directory.
pub fn sqlite(sqlite: &Path, source: &Path, destination: &Path) -> Result<(), &'static str> {
    let dest = destination.to_str().ok_or("invalid SQLite staging path")?;
    if !destination.is_absolute()
        || dest
            .chars()
            .any(|c| c.is_whitespace() || "'\"\\".contains(c))
        || destination.exists()
    {
        return Err("unsafe SQLite staging path");
    }
    let source_meta = std::fs::metadata(source).map_err(|_| "missing SQLite source")?;
    if !source_meta.is_file() {
        return Err("SQLite source is not a file");
    }
    let status = Command::new(sqlite)
        .arg(source)
        .arg(format!(".backup '{dest}'"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "cannot start SQLite backup")?;
    if !status.success() {
        let _ = std::fs::remove_file(destination);
        return Err("SQLite backup failed");
    }
    let output = File::open(destination).map_err(|_| "SQLite backup did not create output")?;
    if !output
        .metadata()
        .map_err(|_| "cannot inspect SQLite staging")?
        .is_file()
    {
        return Err("SQLite staging is not a file");
    }
    output
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "cannot protect SQLite staging")?;
    output
        .sync_all()
        .map_err(|_| "cannot sync SQLite staging")?;
    Ok(())
}

/// Copy an append-only JSONL file to a new private staging file, at the byte
/// boundary measured when the source is opened. A partial last line is omitted.
pub fn jsonl(source: &Path, destination: &Path) -> Result<u64, &'static str> {
    let mut input = File::open(source).map_err(|_| "cannot open JSONL source")?;
    let metadata = input
        .metadata()
        .map_err(|_| "cannot inspect JSONL source")?;
    if !metadata.is_file() {
        return Err("JSONL source is not a file");
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .map_err(|_| "cannot create JSONL staging")?;
    let result = (|| {
        let mut remaining = metadata.len();
        let mut copied = 0u64;
        let mut complete = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let limit = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| "invalid JSONL boundary")?;
            let n = input
                .read(&mut buffer[..limit])
                .map_err(|_| "cannot read JSONL source")?;
            if n == 0 {
                return Err("JSONL source truncated during staging");
            }
            output
                .write_all(&buffer[..n])
                .map_err(|_| "cannot write JSONL staging")?;
            if let Some(index) = buffer[..n].iter().rposition(|b| *b == b'\n') {
                complete = copied + index as u64 + 1;
            }
            copied += n as u64;
            remaining -= n as u64;
        }
        output
            .set_len(complete)
            .map_err(|_| "cannot finalize JSONL staging")?;
        output.sync_all().map_err(|_| "cannot sync JSONL staging")?;
        Ok(complete)
    })();
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn truncates_incomplete_line_and_never_overwrites_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let source = dir.join("source");
        let destination = dir.join("staged");
        std::fs::write(&source, b"{\"v\":1}\n{\"v\":").unwrap();
        assert_eq!(jsonl(&source, &destination), Ok(8));
        assert_eq!(std::fs::read(&destination).unwrap(), b"{\"v\":1}\n");
        assert_eq!(
            destination.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(jsonl(&source, &destination).is_err());
    }

    #[test]
    fn sqlite_backup_includes_wal_and_refuses_existing_destination() {
        use std::io::{BufRead, BufReader};
        let Some(sqlite_bin) = std::env::var_os("TEST_SQLITE_BIN") else {
            return;
        };
        let binary = Path::new(&sqlite_bin);
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let source = dir.join("source.db");
        let destination = dir.join("staged.db");
        let mut shell = Command::new(binary)
            .arg(&source)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = shell.stdin.take().unwrap();
        let mut output = BufReader::new(shell.stdout.take().unwrap());
        input.write_all(b"PRAGMA journal_mode=WAL;\nCREATE TABLE t (value TEXT);\nINSERT INTO t VALUES ('saved in wal');\nSELECT 'ready';\n").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(output.read_line(&mut line).unwrap() > 0);
            if line.trim() == "ready" {
                break;
            }
        }
        assert!(source.with_extension("db-wal").exists());
        sqlite(binary, &source, &destination).unwrap();
        assert_eq!(
            destination.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        let query = Command::new(binary)
            .arg(&destination)
            .arg("SELECT value FROM t")
            .output()
            .unwrap();
        assert!(query.status.success());
        assert_eq!(query.stdout, b"saved in wal\n");
        assert!(sqlite(binary, &source, &destination).is_err());
        drop(input);
        assert!(shell.wait().unwrap().success());
    }

    #[test]
    fn mozc_restarts_input_service_even_when_staging_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let log = dir.join("service-log");
        let ctl = dir.join("systemctl");
        std::fs::write(
            &ctl,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$2\" >> '{}'\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o700)).unwrap();
        for attempt in 0..50 {
            match Command::new(&ctl).args(["--user", "preflight"]).status() {
                Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                result => {
                    result.unwrap();
                    break;
                }
            }
        }
        std::fs::remove_file(&log).unwrap();
        let no_process = dir.join("no-process");
        std::fs::write(&no_process, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&no_process, std::fs::Permissions::from_mode(0o700)).unwrap();
        for attempt in 0..50 {
            match Command::new(&no_process).status() {
                Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                result => {
                    result.unwrap();
                    break;
                }
            }
        }
        let result = mozc(
            &ctl,
            &no_process,
            &no_process,
            &dir.join("missing"),
            &dir.join("out"),
            dir,
        );
        assert_eq!(result, Err("missing Mozc source"));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "is-active\nstop\nstart\nis-active\n"
        );
    }
}
