use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    for attempt in 0..50 {
        match Command::new(path)
            .arg("--preflight")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                std::thread::sleep(Duration::from_millis(10))
            }
            result => {
                result.unwrap();
                break;
            }
        }
    }
}

#[test]
fn monthly_job_can_retry_after_slow_check_releases_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let audit_root = tmp.path().join("audit");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&audit_root).unwrap();
    let marker = tmp.path().join("check-started");
    let restic = tmp.path().join("restic");
    script(
        &restic,
        &format!(
            "#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\necho started > '{}'\nsleep 2\nexit 0\n",
            marker.display()
        ),
    );
    let curl = tmp.path().join("curl");
    script(
        &curl,
        "#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\ncat >/dev/null\nexit 0\n",
    );
    let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":"jsonl","path":"/tmp/example"}],"excludes":[],"audit_roots":[audit_root]}).to_string();
    let command = |operation: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_local-state-backup"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("LOCAL_STATE_PROFILE", &profile)
            .env("RESTIC_REPOSITORY", "fixture")
            .env("RESTIC_PASSWORD", "fixture")
            .env("AWS_ACCESS_KEY_ID", "fixture")
            .env("AWS_SECRET_ACCESS_KEY", "fixture")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
            .env("LOCAL_STATE_RESTIC_BIN", &restic)
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .args(["__run", operation]);
        command
    };
    let mut slow = command("check")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "check did not acquire lock");
        std::thread::sleep(Duration::from_millis(10));
    }
    let blocked = command("audit").output().unwrap();
    assert!(!blocked.status.success());
    assert!(
        String::from_utf8_lossy(&blocked.stderr).contains("another backup operation is running")
    );
    let finished = slow.wait().unwrap();
    assert!(finished.success());
    let retry = command("audit").output().unwrap();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
}
