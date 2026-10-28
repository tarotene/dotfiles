//! Local state backup entry point. Private data is loaded through BWS at runtime.
use backup_runtime::{keyring_lookup, status_ok};
use local_state_backup::{inventory, profile, replication, staging};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const BUILTIN_EXCLUDES: &[&str] = &[
    "node_modules",
    "target",
    ".direnv",
    ".venv",
    "dist",
    "result",
    ".ssh",
    ".gnupg",
    ".aws",
    "**/.codex/auth.json",
    "**/.claude/.credentials.json",
    "**/.config/rclone/**",
    "**/.config/gcloud/**",
    "**/.config/clasp/**",
    "**/.clasprc.json",
    "**/.local/share/keyrings/**",
    "**/Downloads/**",
];

fn add_excludes(command: &mut Command, profile: &profile::Profile) {
    for exclude in BUILTIN_EXCLUDES
        .iter()
        .copied()
        .chain(profile.excludes.iter().map(String::as_str))
    {
        command.arg("--exclude").arg(exclude);
    }
}

fn binary(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .unwrap_or_else(|| default.into())
        .into()
}
fn required(name: &str) -> Result<String, &'static str> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or("required backup secret missing")
}
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn bws(operation: &str) -> Result<(), String> {
    // Recovery must not depend on BWS being reachable on the next invocation.
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unavailable")?);
    if !home.is_absolute() {
        return Err("invalid HOME".into());
    }
    {
        let _lock = lock(&home).map_err(str::to_owned)?;
        staging::recover_mozc(
            &binary("LOCAL_STATE_SYSTEMCTL_BIN", "systemctl"),
            &state_path(&home),
        )
        .map_err(str::to_owned)?;
        staging_root(&home).map_err(str::to_owned)?;
    }
    let token = keyring_lookup(
        "local-state-backup",
        "bws-access-token",
        "LOCAL_STATE_SECRET_TOOL_BIN",
    )?;
    let exe = std::env::current_exe().map_err(|_| "cannot resolve backup executable")?;
    let home = std::env::var("HOME").map_err(|_| "HOME is unavailable")?;
    let path = std::env::var("PATH").map_err(|_| "PATH is unavailable")?;
    let mut words = vec![
        "env".to_owned(),
        format!("HOME={home}"),
        format!("PATH={path}"),
    ];
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Ok(value) = std::env::var(key) {
            words.push(format!("{key}={value}"));
        }
    }
    words.extend([
        exe.to_str().ok_or("invalid backup executable")?.to_owned(),
        "__run".into(),
        operation.into(),
    ]);
    let words = words
        .iter()
        .map(|word| sh_quote(word))
        .collect::<Vec<_>>()
        .join(" ");
    let status = Command::new(binary("LOCAL_STATE_BWS_BIN", "bws"))
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("BWS_ACCESS_TOKEN", token)
        .args(["run", "--no-inherit-env", "--", &words])
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "cannot start BWS")?;
    status_ok(status)
}

struct Lock(std::fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn state_path(home: &Path) -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("local-state-backup")
}
fn lock(home: &Path) -> Result<Lock, &'static str> {
    let state = state_path(home);
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)
        .map_err(|_| "cannot create backup state")?;
    let metadata = std::fs::symlink_metadata(&state).map_err(|_| "cannot inspect backup state")?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("backup state is not private");
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(state.join("lock"))
        .map_err(|_| "cannot open backup lock")?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("another backup operation is running");
    }
    Ok(Lock(file))
}

fn staging_root(home: &Path) -> Result<PathBuf, &'static str> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let root = state_path(home).join("staging");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })
        .map_err(|_| "cannot create private staging root")?;
    let metadata = std::fs::symlink_metadata(&root).map_err(|_| "cannot inspect staging root")?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("staging root is not private");
    }
    if std::fs::read_dir(&root)
        .map_err(|_| "cannot inspect staging root")?
        .next()
        .is_some()
    {
        return Err("abandoned staging remains; inspect and remove it before retrying");
    }
    Ok(root)
}

fn record_result(home: &Path, operation: &str, success: bool) -> Result<(), &'static str> {
    use std::os::unix::fs::DirBuilderExt;
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("local-state-backup/local");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)
        .map_err(|_| "cannot create backup log directory")?;
    use std::os::unix::fs::PermissionsExt;
    let state_meta =
        std::fs::symlink_metadata(&state).map_err(|_| "cannot inspect backup log directory")?;
    if !state_meta.is_dir()
        || state_meta.file_type().is_symlink()
        || state_meta.permissions().mode() & 0o077 != 0
    {
        return Err("backup log directory is not private");
    }
    let output = Command::new("date")
        .env_clear()
        .env("PATH", required("PATH")?)
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot read backup log clock")?;
    if !output.status.success() {
        return Err("cannot read backup log clock");
    }
    let now = std::str::from_utf8(&output.stdout)
        .map_err(|_| "invalid backup log clock")?
        .trim();
    if now.len() != 20
        || !now
            .bytes()
            .all(|b| b.is_ascii_digit() || b"-:TZ".contains(&b))
    {
        return Err("invalid backup log clock");
    }
    let path = state.join(format!("{}.log", &now[..10]));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "cannot open backup log")?;
    writeln!(
        file,
        "{now} {operation} {}",
        if success { "success" } else { "failure" }
    )
    .map_err(|_| "cannot write backup log")?;
    let threshold = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(30 * 24 * 60 * 60))
        .ok_or("invalid system clock")?;
    for entry in std::fs::read_dir(&state).map_err(|_| "cannot read backup logs")? {
        let entry = entry.map_err(|_| "cannot read backup logs")?;
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".log") {
            continue;
        }
        let metadata =
            std::fs::symlink_metadata(entry.path()).map_err(|_| "cannot inspect backup log")?;
        if metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata
                .modified()
                .map_err(|_| "cannot inspect backup log")?
                < threshold
        {
            std::fs::remove_file(entry.path()).map_err(|_| "cannot rotate backup log")?;
        }
    }
    Ok(())
}

fn restic(args: &[&str]) -> Result<(), &'static str> {
    for key in [
        "RESTIC_REPOSITORY",
        "RESTIC_PASSWORD",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
    ] {
        required(key)?;
    }
    let mut command = restic_command()?;
    let status = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "cannot start restic")?;
    if status.success() {
        Ok(())
    } else {
        Err("restic operation failed")
    }
}

fn restic_command() -> Result<Command, &'static str> {
    let mut cmd = Command::new(binary("LOCAL_STATE_RESTIC_BIN", "restic"));
    cmd.env_clear();
    for key in [
        "RESTIC_REPOSITORY",
        "RESTIC_PASSWORD",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "HOME",
        "PATH",
    ] {
        cmd.env(key, required(key)?);
    }
    for key in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
        "XDG_CACHE_HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    Ok(cmd)
}

fn snapshot_ids(
    profile: &profile::Profile,
    tag: &str,
) -> Result<Vec<(String, Vec<String>)>, &'static str> {
    let output = restic_command()?
        .args([
            "snapshots",
            "--json",
            "--host",
            &profile.identity,
            "--tag",
            tag,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot list backup snapshots")?;
    if !output.status.success() {
        return Err("cannot list backup snapshots");
    }
    let values: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|_| "invalid snapshot list")?;
    values
        .iter()
        .map(|value| {
            let id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or("invalid snapshot id")?
                .to_owned();
            let tags = value
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .ok_or("missing snapshot tags")?
                .iter()
                .map(|tag| {
                    tag.as_str()
                        .map(str::to_owned)
                        .ok_or("invalid snapshot tag")
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((id, tags))
        })
        .collect()
}

fn retention(profile: &profile::Profile, dry_run: bool) -> Result<(), &'static str> {
    let args = [
        "forget",
        "--host",
        &profile.identity,
        "--tag",
        "local-state",
        "--group-by",
        "host",
        "--keep-daily",
        "7",
        "--keep-weekly",
        "5",
        "--keep-monthly",
        "12",
    ];
    let data = snapshot_ids(profile, "local-state")?;
    let manifests = snapshot_ids(profile, "local-state-manifest")?;
    if data.is_empty() {
        return Err("no data snapshots");
    }
    let mut links = std::collections::HashMap::new();
    for (id, tags) in &manifests {
        let associations: Vec<_> = tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("for-"))
            .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .collect();
        if associations.len() != 1
            || links
                .insert(associations[0].to_owned(), id.clone())
                .is_some()
        {
            return Err("invalid manifest association");
        }
    }
    if data.iter().any(|(id, _)| !links.contains_key(id)) {
        return Err("data snapshot lacks a manifest");
    }
    if dry_run {
        let output = restic_command()?
            .args(args)
            .args(["--dry-run", "--json"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "cannot preview retention")?;
        if !output.status.success() {
            return Err("cannot preview retention");
        }
        let groups: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).map_err(|_| "invalid retention preview")?;
        if groups.is_empty() {
            return Err("retention preview is empty");
        }
        let count = |key: &str| {
            groups
                .iter()
                .map(|g| {
                    g.get(key)
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len)
                })
                .sum::<usize>()
        };
        println!(
            "retention: keep {}, remove {}",
            count("keep"),
            count("remove")
        );
        return Ok(());
    }
    restic(&args)?;
    let kept: std::collections::HashSet<_> = snapshot_ids(profile, "local-state")?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    if kept.is_empty() {
        return Err("retention removed all data snapshots");
    }
    for (data_id, manifest_id) in links {
        if !kept.contains(&data_id) {
            restic(&["forget", &manifest_id])?;
        }
    }
    restic(&["prune"])
}

fn heartbeat(suffix: &str) -> Result<(), &'static str> {
    let url = required("HEALTHCHECKS_URL")?;
    let suffix_part = url
        .strip_prefix("https://hc-ping.com/")
        .ok_or("invalid heartbeat URL")?;
    if suffix_part.is_empty()
        || !suffix_part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/')
    {
        return Err("invalid heartbeat URL");
    }
    let mut curl = Command::new(binary("LOCAL_STATE_CURL_BIN", "curl"));
    curl.env_clear();
    for key in [
        "PATH",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            curl.env(key, value);
        }
    }
    let mut child = curl
        .args([
            "-q",
            "--fail",
            "--silent",
            "--connect-timeout",
            "15",
            "--max-time",
            "60",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "cannot start heartbeat")?;
    let write = child
        .stdin
        .take()
        .ok_or("cannot send heartbeat")?
        .write_all(format!("url = \"{url}{suffix}\"\n").as_bytes());
    let status = child.wait().map_err(|_| "heartbeat failed")?;
    if write.is_err() || !status.success() {
        return Err("heartbeat failed");
    }
    Ok(())
}

fn metered() -> Result<bool, &'static str> {
    let output = Command::new(binary("LOCAL_STATE_NMCLI_BIN", "nmcli"))
        .args([
            "-t",
            "-f",
            "GENERAL.STATE,GENERAL.METERED",
            "device",
            "show",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot inspect connection")?;
    if !output.status.success() {
        return Err("cannot inspect connection");
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|_| "invalid connection state")?;
    for block in text.split("\n\n") {
        if block
            .lines()
            .any(|line| line.starts_with("GENERAL.STATE:100 "))
            && block
                .lines()
                .any(|line| line.starts_with("GENERAL.METERED:yes"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn verify_snapshot_targets(
    profile: &profile::Profile,
    snapshot: &str,
    stage: &Path,
) -> Result<(), &'static str> {
    let listing = restic_command()?
        .args(["ls", "--json", snapshot])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot list new snapshot")?;
    if !listing.status.success() {
        return Err("cannot list new snapshot");
    }
    let mut nodes = std::collections::HashMap::<String, String>::new();
    for line in listing
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| "invalid snapshot listing")?;
        match value.get("struct_type").and_then(serde_json::Value::as_str) {
            Some("snapshot") => {}
            Some("node") => {
                let path = value
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("invalid snapshot node")?;
                let kind = value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("invalid snapshot node")?;
                if nodes.insert(path.to_owned(), kind.to_owned()).is_some() {
                    return Err("duplicate snapshot node");
                }
            }
            _ => return Err("invalid snapshot listing"),
        }
    }
    fn require_tree(
        path: &Path,
        nodes: &std::collections::HashMap<String, String>,
    ) -> Result<(), &'static str> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| "backup verification source missing")?;
        let kind = if metadata.file_type().is_symlink() {
            "symlink"
        } else if metadata.is_dir() {
            "dir"
        } else if metadata.is_file() {
            "file"
        } else {
            return Err("unsupported backup verification source");
        };
        let name = path.to_str().ok_or("invalid backup verification path")?;
        if nodes.get(name).map(String::as_str) != Some(kind) {
            return Err("registered backup source missing from snapshot");
        }
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in
                std::fs::read_dir(path).map_err(|_| "cannot inspect backup verification source")?
            {
                require_tree(
                    &entry
                        .map_err(|_| "cannot inspect backup verification source")?
                        .path(),
                    nodes,
                )?;
            }
        }
        Ok(())
    }
    require_tree(stage, &nodes)?;
    for source in &profile.sources {
        match source.kind.as_str() {
            "git" => {
                let path = source
                    .path
                    .to_str()
                    .ok_or("invalid backup verification path")?;
                if nodes.get(path).map(String::as_str) != Some("dir") {
                    return Err("registered backup source missing from snapshot");
                }
                require_tree(&source.path.join(".git"), &nodes)?;
                // Verify tracked and non-ignored untracked files, except the built-in
                // generated/credential directories intentionally excluded.
                for tracked in [true, false] {
                    let mut command = Command::new(binary("LOCAL_STATE_GIT_BIN", "git"));
                    command.arg("-C").arg(&source.path).args(["ls-files", "-z"]);
                    if tracked {
                        command.arg("--cached");
                    } else {
                        command.args(["--others", "--exclude-standard"]);
                    }
                    let untracked = command
                        .stdin(Stdio::null())
                        .stderr(Stdio::null())
                        .output()
                        .map_err(|_| "cannot inspect untracked files")?;
                    if !untracked.status.success() {
                        return Err("cannot inspect untracked files");
                    }
                    for relative in untracked
                        .stdout
                        .split(|byte| *byte == 0)
                        .filter(|part| !part.is_empty())
                    {
                        let relative =
                            std::str::from_utf8(relative).map_err(|_| "invalid untracked path")?;
                        let relative = Path::new(relative);
                        if relative.is_absolute()
                            || relative.components().any(|part| {
                                matches!(
                                    part,
                                    std::path::Component::ParentDir | std::path::Component::CurDir
                                )
                            })
                        {
                            return Err("invalid untracked path");
                        }
                        let parts: Vec<_> = relative
                            .components()
                            .filter_map(|part| part.as_os_str().to_str())
                            .collect();
                        let excluded = parts.iter().any(|part| {
                            [
                                "node_modules",
                                "target",
                                ".direnv",
                                ".venv",
                                "dist",
                                "result",
                                ".ssh",
                                ".gnupg",
                                ".aws",
                                "Downloads",
                                ".clasprc.json",
                            ]
                            .contains(part)
                        }) || parts.windows(2).any(|pair| {
                            matches!(
                                pair,
                                [".codex", "auth.json"]
                                    | [".claude", ".credentials.json"]
                                    | [".config", "rclone"]
                                    | [".config", "gcloud"]
                                    | [".config", "clasp"]
                            )
                        }) || parts
                            .windows(3)
                            .any(|triple| matches!(triple, [".local", "share", "keyrings"]));
                        if excluded {
                            continue;
                        }
                        let file = source.path.join(relative);
                        match std::fs::symlink_metadata(&file) {
                            Ok(_) => require_tree(&file, &nodes)?,
                            Err(error)
                                if tracked && error.kind() == std::io::ErrorKind::NotFound =>
                            {
                                continue
                            }
                            Err(_) => return Err("cannot inspect git file"),
                        }
                    }
                }
            }
            "git-common" => require_tree(&source.path, &nodes)?,
            "sqlite" | "mozc" | "jsonl" => {}
            _ => return Err("invalid source kind"),
        }
    }
    Ok(())
}

fn backup(profile: &profile::Profile, home: &Path) -> Result<(), &'static str> {
    inventory::inspect(profile, home, &binary("LOCAL_STATE_GIT_BIN", "git"))?;
    if metered()? {
        return Err("backup postponed because the connection is metered");
    }
    let stage = tempfile::Builder::new()
        .prefix("local-state-backup-")
        .tempdir_in(staging_root(home)?)
        .map_err(|_| "cannot create backup staging")?;
    let mut targets = Vec::new();
    let mut mapping = Vec::new();
    for (index, source) in profile.sources.iter().enumerate() {
        let name = match source.kind.as_str() {
            "git" | "git-common" => {
                targets.push(source.path.clone());
                continue;
            }
            "sqlite" => format!("{index}.sqlite"),
            "jsonl" => format!("{index}.jsonl"),
            "mozc" => format!("{index}.mozc"),
            _ => return Err("invalid source kind"),
        };
        let target = stage.path().join(&name);
        match source.kind.as_str() {
            "sqlite" => staging::sqlite(
                &binary("LOCAL_STATE_SQLITE_BIN", "sqlite3"),
                &source.path,
                &target,
            )?,
            "jsonl" => {
                staging::jsonl(&source.path, &target)?;
            }
            "mozc" => staging::mozc(
                &binary("LOCAL_STATE_SYSTEMCTL_BIN", "systemctl"),
                &binary("LOCAL_STATE_PKILL_BIN", "pkill"),
                &binary("LOCAL_STATE_PGREP_BIN", "pgrep"),
                &source.path,
                &target,
                &state_path(home),
            )?,
            _ => return Err("invalid source kind"),
        }
        mapping.push(
            serde_json::json!({"kind": source.kind, "original": source.path, "staged": name}),
        );
    }
    let version = Command::new(binary("LOCAL_STATE_RESTIC_BIN", "restic"))
        .arg("version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot inspect restic version")?;
    if !version.status.success() {
        return Err("cannot inspect restic version");
    }
    let version = std::str::from_utf8(&version.stdout)
        .map_err(|_| "invalid restic version")?
        .trim();
    if !version.starts_with("restic ") {
        return Err("invalid restic version");
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "invalid system clock")?
        .as_secs();
    let manifest = serde_json::json!({"identity": profile.identity, "created_unix": timestamp, "restic_version": version, "sources": profile.sources.iter().map(|s| serde_json::json!({"kind": s.kind, "path": s.path})).collect::<Vec<_>>(), "staging": mapping, "excludes": profile.excludes});
    targets.push(stage.path().to_owned());
    let mut command = restic_command()?;
    command.args([
        "backup",
        "--json",
        "--one-file-system",
        "--host",
        &profile.identity,
        "--tag",
        "local-state",
    ]);
    add_excludes(&mut command, profile);
    command
        .args(targets)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let output = command.output().map_err(|_| "cannot start restic backup")?;
    if !output.status.success() {
        return Err("restic backup failed");
    }
    let summary = output
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find(|v| v.get("message_type").and_then(serde_json::Value::as_str) == Some("summary"))
        .ok_or("restic summary missing")?;
    let snapshot = summary
        .get("snapshot_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("restic snapshot id missing")?;
    if snapshot.len() != 64 || !snapshot.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid restic snapshot id");
    }
    verify_snapshot_targets(profile, snapshot, stage.path())?;
    // restic cannot combine --stdin with file arguments. Store the manifest in
    // a separate encrypted snapshot, linked by the full data snapshot ID.
    // No plaintext manifest file is written to staging.
    let manifest_tag = format!("for-{snapshot}");
    let mut child = restic_command()?
        .args([
            "backup",
            "--json",
            "--stdin",
            "--stdin-filename",
            "local-state-manifest.json",
            "--host",
            &profile.identity,
            "--tag",
            "local-state-manifest",
            "--tag",
            &manifest_tag,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "cannot start manifest backup")?;
    let write = child
        .stdin
        .take()
        .ok_or("cannot write backup manifest")?
        .write_all(&serde_json::to_vec(&manifest).map_err(|_| "cannot serialize backup manifest")?);
    let saved = child
        .wait_with_output()
        .map_err(|_| "manifest backup failed")?;
    if write.is_err()
        || !saved.status.success()
        || !saved
            .stdout
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .any(|v| {
                v.get("message_type").and_then(serde_json::Value::as_str) == Some("summary")
                    && v.get("snapshot_id")
                        .and_then(serde_json::Value::as_str)
                        .is_some()
            })
    {
        return Err("manifest backup failed");
    }
    let files = summary
        .get("total_files_processed")
        .and_then(serde_json::Value::as_u64)
        .ok_or("restic file count missing")?;
    let bytes = summary
        .get("total_bytes_processed")
        .and_then(serde_json::Value::as_u64)
        .ok_or("restic byte count missing")?;
    println!("snapshot {snapshot} files={files} bytes={bytes}");
    Ok(())
}

fn dry_run(profile: &profile::Profile, home: &Path) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let counts = inventory::inspect(profile, home, &binary("LOCAL_STATE_GIT_BIN", "git"))?;
    println!("filesystem boundary: each source is limited to its own device (--one-file-system)");
    println!("staging sources are inspected in place; the real backup creates consistent copies");
    for count in counts {
        let device = std::fs::metadata(&count.path)
            .map_err(|_| "cannot inspect backup source")?
            .dev();
        println!(
            "source\t{}\tdevice={device}\tfiles={}\tbytes={}",
            count.path.display(),
            count.files,
            count.bytes
        );
    }
    for exclude in BUILTIN_EXCLUDES {
        println!("exclude built-in\t{exclude}");
    }
    for exclude in &profile.excludes {
        println!("exclude profile\t{exclude}");
    }
    let mut command = restic_command()?;
    command.args([
        "backup",
        "--dry-run",
        "-vv",
        "--one-file-system",
        "--host",
        &profile.identity,
        "--tag",
        "local-state",
    ]);
    add_excludes(&mut command, profile);
    for source in &profile.sources {
        command.arg(&source.path);
    }
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot start restic dry-run")?;
    if !output.status.success() {
        return Err("restic dry-run failed");
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|_| "invalid restic dry-run output")?;
    for line in text.lines() {
        if [
            "new ",
            "modified ",
            "unchanged ",
            "Would add to the repository:",
            "processed ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            println!("restic\t{line}");
        }
    }
    Ok(())
}

fn restore_test(profile: &profile::Profile) -> Result<(), &'static str> {
    let listing = restic_command()?
        .args([
            "ls",
            "--json",
            "--host",
            &profile.identity,
            "--tag",
            "local-state",
            "latest",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot list snapshot")?;
    if !listing.status.success() {
        return Err("cannot list snapshot");
    }
    let snapshot_id = listing
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find_map(|value| {
            if value.get("struct_type").and_then(serde_json::Value::as_str) != Some("snapshot") {
                return None;
            }
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_owned)
        })
        .ok_or("snapshot id missing")?;
    let manifest_tag = format!("for-{snapshot_id}");
    let saved_manifest = restic_command()?
        .args([
            "dump",
            "--host",
            &profile.identity,
            "--tag",
            &manifest_tag,
            "latest",
            "/local-state-manifest.json",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot read snapshot manifest")?;
    if !saved_manifest.status.success() {
        return Err("snapshot manifest missing");
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&saved_manifest.stdout).map_err(|_| "invalid snapshot manifest")?;
    if manifest.get("identity").and_then(serde_json::Value::as_str) != Some(&profile.identity) {
        return Err("snapshot manifest identity mismatch");
    }
    let candidate = listing
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find_map(|value| {
            if value.get("type").and_then(serde_json::Value::as_str) != Some("file")
                || value
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX)
                    > 5 * 1024 * 1024
            {
                return None;
            }
            let path = value.get("path").and_then(serde_json::Value::as_str)?;
            let name = Path::new(path).file_name()?.to_str()?;
            let number = name
                .strip_suffix(".jsonl")
                .or_else(|| name.strip_suffix(".sqlite"))?;
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(PathBuf::from(path))
        })
        .ok_or("no suitable staged sample in snapshot")?;
    let parent = candidate.parent().ok_or("invalid snapshot sample")?;
    let name = candidate
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("invalid snapshot sample")?;
    let temp = tempfile::Builder::new()
        .prefix("local-state-restore-")
        .tempdir()
        .map_err(|_| "cannot create restore staging")?;
    let target = temp.path().to_str().ok_or("invalid restore staging")?;
    let snapshot = format!("{}:{}", snapshot_id, parent.display());
    restic(&[
        "restore",
        &snapshot,
        "--host",
        &profile.identity,
        "--tag",
        "local-state",
        "--target",
        target,
        "--include",
        name,
        "--verify",
    ])?;
    let original = restic_command()?
        .arg("dump")
        .args([
            "--host",
            &profile.identity,
            "--tag",
            "local-state",
            &snapshot_id,
        ])
        .arg(&candidate)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot read snapshot sample")?;
    if !original.status.success() {
        return Err("cannot read snapshot sample");
    }
    let restored = std::fs::read(temp.path().join(name)).map_err(|_| "restored sample missing")?;
    if original.stdout != restored {
        return Err("restored sample differs");
    }
    println!("restore sample verified");
    Ok(())
}

fn run_inner(operation: &str) -> Result<(), &'static str> {
    let home = PathBuf::from(required("HOME")?);
    if !home.is_absolute() {
        return Err("invalid HOME");
    }
    let _lock = lock(&home)?;
    staging::recover_mozc(
        &binary("LOCAL_STATE_SYSTEMCTL_BIN", "systemctl"),
        &state_path(&home),
    )?;
    staging_root(&home)?;
    let bytes = required("LOCAL_STATE_PROFILE")?;
    let profile = profile::parse(bytes.as_bytes())?;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("local-state-backup");
    profile::persist(bytes.as_bytes(), &config)?;
    if operation == "backup" && metered()? {
        return Err("backup postponed because the connection is metered");
    }
    let monitored = matches!(
        operation,
        "backup" | "check" | "maintenance" | "restore-test" | "audit"
    );
    if monitored {
        heartbeat("/start")?;
    }
    let result = (|| -> Result<(), &'static str> {
        match operation {
            "inventory" => {
                let counts =
                    inventory::inspect(&profile, &home, &binary("LOCAL_STATE_GIT_BIN", "git"))?;
                for count in counts {
                    println!("{}\t{}\t{}", count.path.display(), count.files, count.bytes);
                }
                Ok(())
            }
            "dry-run" => dry_run(&profile, &home),
            "check" => restic(&["check"]),
            "maintenance" => {
                let curl = binary("LOCAL_STATE_CURL_BIN", "curl");
                let auth = replication::authorize(
                    &curl,
                    &required("AWS_ACCESS_KEY_ID")?,
                    &required("AWS_SECRET_ACCESS_KEY")?,
                )?;
                replication::verify_repository(
                    &curl,
                    &auth,
                    &required("RESTIC_REPOSITORY")?,
                    &profile.bucket_id,
                    &profile.prefix,
                )?;
                replication::verify_remote(&curl, &auth, &profile.bucket_id, &profile.prefix)?;
                retention(&profile, false)
            }
            "maintenance-dry-run" => retention(&profile, true),
            "backup" => backup(&profile, &home),
            "restore-test" => restore_test(&profile),
            "audit" | "audit-verbose" => {
                let paths = inventory::audit(&profile)?;
                let count = paths.len();
                if operation == "audit-verbose" {
                    for path in paths {
                        println!("{}", path.display());
                    }
                }
                if count > 0 {
                    eprintln!("warning: {count} recently updated repositories are not registered");
                } else {
                    println!("no unregistered recently updated repositories");
                }
                Ok(())
            }
            _ => Err("invalid backup operation"),
        }
    })();
    if !monitored {
        return result;
    }
    let result = if result.is_ok() {
        heartbeat("")
    } else if heartbeat("/fail").is_err() {
        Err("operation and failure heartbeat failed")
    } else {
        result
    };
    if record_result(&home, operation, result.is_ok()).is_err() {
        if result.is_ok() {
            let _ = heartbeat("/fail");
        }
        return Err("cannot record backup result");
    }
    result
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [inner, operation] if inner == "__run" => run_inner(operation).map_err(str::to_owned),
        [operation, flag] if operation == "maintenance" && flag == "--dry-run" => bws("maintenance-dry-run"),
        [operation, flag] if operation == "audit" && flag == "--verbose" => bws("audit-verbose"),
        [operation] if ["inventory", "dry-run", "backup", "check", "maintenance", "restore-test", "audit"].contains(&operation.as_str()) => bws(operation),
        _ => Err("usage: local-state-backup inventory|dry-run|backup|check|maintenance|restore-test|audit".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "local-state-backup: {error}");
            ExitCode::FAILURE
        }
    }
}
