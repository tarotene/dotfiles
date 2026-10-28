use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::Duration;

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
fn missing_source_staging_restic_and_heartbeat_fail_closed_without_secrets() {
    for case in [
        "missing",
        "staging",
        "restic",
        "manifest",
        "heartbeat",
        "metered",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir(&home).unwrap();
        let old_log = home.join(".local/state/local-state-backup/local/2000-01-01.log");
        if case == "restic" {
            fs::create_dir_all(old_log.parent().unwrap()).unwrap();
            fs::set_permissions(old_log.parent().unwrap(), fs::Permissions::from_mode(0o700))
                .unwrap();
            fs::set_permissions(
                home.join(".local/state/local-state-backup"),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            fs::write(&old_log, "old\n").unwrap();
            assert!(Command::new("touch")
                .args(["-d", "31 days ago"])
                .arg(&old_log)
                .status()
                .unwrap()
                .success());
        }
        let source = temp.path().join("source");
        if case != "missing" {
            fs::write(&source, b"fixture\n").unwrap();
        }
        let kind = if case == "staging" { "sqlite" } else { "jsonl" };
        let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":kind,"path":source}],"excludes":[],"audit_roots":[temp.path()]}).to_string();
        let events = temp.path().join("events");
        let curl = temp.path().join("curl");
        let exit = if case == "heartbeat" { 1 } else { 0 };
        script(&curl, &format!("#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\ninput=$(cat)\ncase \"$input\" in */start*) echo start >> '{}' ;; */fail*) echo fail >> '{}' ;; *) echo success >> '{}' ;; esac\nexit {exit}\n", events.display(), events.display(), events.display()));
        let restic = temp.path().join("restic");
        let fake_snapshot = if case == "manifest" {
            "case \" $* \" in *' --stdin '*) exit 8;; esac\nprintf '%s\\n' '{\"message_type\":\"summary\",\"snapshot_id\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"total_files_processed\":1,\"total_bytes_processed\":1}'\nexit 0\n"
        } else {
            "exit 7\n"
        };
        script(&restic, &format!("#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\n[ \"$1\" = version ] && {{ echo 'restic 0.18.1'; exit 0; }}\n{fake_snapshot}"));
        let sqlite = temp.path().join("sqlite3");
        script(
            &sqlite,
            "#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\nexit 8\n",
        );
        let nmcli = temp.path().join("nmcli");
        let metered = if case == "metered" { "yes" } else { "no" };
        script(&nmcli, &format!("#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\nprintf 'GENERAL.STATE:100 (connected)\\nGENERAL.METERED:{metered}\\n'\n"));
        let output = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("LOCAL_STATE_PROFILE", &profile)
            .env("RESTIC_REPOSITORY", "repository-secret")
            .env("RESTIC_PASSWORD", "password-secret")
            .env("AWS_ACCESS_KEY_ID", "key-secret")
            .env("AWS_SECRET_ACCESS_KEY", "secret-secret")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/heartbeat-secret")
            .env("LOCAL_STATE_RESTIC_BIN", &restic)
            .env("LOCAL_STATE_SQLITE_BIN", &sqlite)
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
            .args(["__run", "backup"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{case}");
        for data in [&output.stdout, &output.stderr] {
            let text = String::from_utf8_lossy(data);
            assert!(!text.contains("-secret"), "{case}: {text}");
        }
        let events = fs::read_to_string(events).unwrap_or_default();
        assert!(
            !events.lines().any(|line| line == "success"),
            "{case}: {events}"
        );
        if case != "heartbeat" && case != "metered" {
            assert!(
                events.lines().any(|line| line == "fail"),
                "{case}: {events}"
            );
        }
        if case == "metered" {
            assert!(events.is_empty());
        }
        if case == "restic" {
            assert!(!old_log.exists());
        }
    }
}
