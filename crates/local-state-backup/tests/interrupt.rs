use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
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
fn interruption_recovers_input_service() {
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mozc = dir.path().join("mozc");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&mozc).unwrap();
        fs::write(mozc.join("learning"), b"fixture").unwrap();
        let log = dir.path().join("service.log");
        let ctl = dir.path().join("systemctl");
        script(&ctl, &format!("#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\nprintf '%s\\n' \"$2\" >> '{}'\n[ \"$2\" = stop ] && sleep 1\nexit 0\n", log.display()));
        let no_process = dir.path().join("no-process");
        script(&no_process, "#!/bin/sh\nexit 1\n");
        let curl = dir.path().join("curl");
        script(&curl, "#!/bin/sh\ncat >/dev/null\nexit 0\n");
        let nmcli = dir.path().join("nmcli");
        script(
            &nmcli,
            "#!/bin/sh\nprintf 'GENERAL.STATE:100 (connected)\\nGENERAL.METERED:no\\n'\n",
        );
        let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":"mozc","path":mozc}],"excludes":[],"audit_roots":[dir.path()]}).to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("LOCAL_STATE_PROFILE", profile)
            .env("RESTIC_REPOSITORY", "fixture")
            .env("RESTIC_PASSWORD", "fixture")
            .env("AWS_ACCESS_KEY_ID", "fixture")
            .env("AWS_SECRET_ACCESS_KEY", "fixture")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
            .env("LOCAL_STATE_SYSTEMCTL_BIN", &ctl)
            .env("LOCAL_STATE_PKILL_BIN", &no_process)
            .env("LOCAL_STATE_PGREP_BIN", &no_process)
            .args(["__run", "backup"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .any(|line| line == "stop")
            {
                break;
            }
            assert!(Instant::now() < deadline, "input service was not stopped");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let status = child.wait().unwrap();
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(!status.success(), "interrupted backup succeeded");
        let marker = home.join(".local/state/local-state-backup/mozc-input-service-was-active");
        if signal == libc::SIGKILL {
            assert!(marker.exists());
            // Recovery precedes even an unavailable BWS/keyring lookup.
            let next = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &home)
                .env("LOCAL_STATE_SYSTEMCTL_BIN", &ctl)
                .env("LOCAL_STATE_SECRET_TOOL_BIN", &no_process)
                .arg("inventory")
                .output()
                .unwrap();
            assert!(!next.status.success());
            assert!(!marker.exists());
            assert!(String::from_utf8_lossy(&next.stderr).contains("abandoned staging remains"));
            let staging = home.join(".local/state/local-state-backup/staging");
            assert!(fs::read_dir(staging).unwrap().next().is_some());
        } else {
            assert!(!marker.exists());
        }
        assert!(
            fs::read_to_string(&log)
                .unwrap()
                .lines()
                .any(|line| line == "start"),
            "{stderr}"
        );
    }
}

#[test]
fn sigkill_after_staging_refuses_abandoned_private_data() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir(&home).unwrap();
    let jsonl = dir.path().join("activity.jsonl");
    fs::write(&jsonl, b"{\"event\":1}\n").unwrap();
    let entered = dir.path().join("entered");
    let restic = dir.path().join("restic");
    script(
        &restic,
        &format!(
            "#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\ntouch '{}'\nsleep 3\n",
            entered.display()
        ),
    );
    let curl = dir.path().join("curl");
    script(&curl, "#!/bin/sh\ncat >/dev/null\nexit 0\n");
    let nmcli = dir.path().join("nmcli");
    script(
        &nmcli,
        "#!/bin/sh\nprintf 'GENERAL.STATE:100 (connected)\\nGENERAL.METERED:no\\n'\n",
    );
    let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":"jsonl","path":jsonl}],"excludes":[],"audit_roots":[dir.path()]}).to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .env("RESTIC_REPOSITORY", "fixture")
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
        .env("LOCAL_STATE_CURL_BIN", &curl)
        .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .args(["__run", "backup"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.exists() {
        assert!(Instant::now() < deadline, "restic did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGKILL) }, 0);
    assert!(!child.wait().unwrap().success());
    let staging = home.join(".local/state/local-state-backup/staging");
    let abandoned = fs::read_dir(staging)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(fs::read_dir(abandoned).unwrap().next().is_some());
    let next = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .args(["__run", "backup"])
        .output()
        .unwrap();
    assert!(!next.status.success());
    assert!(String::from_utf8_lossy(&next.stderr).contains("abandoned staging remains"));
}
