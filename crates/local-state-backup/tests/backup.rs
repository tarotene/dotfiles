use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn local_snapshot_contains_jsonl_and_git_untracked_files() {
    let Some(restic) = std::env::var_os("TEST_RESTIC_BIN") else {
        return;
    };
    let Some(sqlite) = std::env::var_os("TEST_SQLITE_BIN") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("work");
    let store = dir.path().join("restic");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&repo).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(&repo)
        .status()
        .unwrap()
        .success());
    fs::write(repo.join("untracked"), b"unique content\n").unwrap();
    fs::write(repo.join("tracked"), b"original\n").unwrap();
    fs::write(repo.join("deleted"), b"deleted\n").unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["add", "tracked", "deleted"])
        .status()
        .unwrap()
        .success());
    fs::write(repo.join("tracked"), b"modified\n").unwrap();
    fs::remove_file(repo.join("deleted")).unwrap();
    fs::write(repo.join("exclude-me.tmp"), b"excluded\n").unwrap();
    fs::write(repo.join(".gitignore"), b"exclude-me.tmp\n").unwrap();
    fs::create_dir(repo.join("target")).unwrap();
    fs::write(repo.join("target/generated"), b"generated\n").unwrap();
    let jsonl = dir.path().join("activity.jsonl");
    fs::write(&jsonl, b"{\"event\":1}\n{\"incomplete\"").unwrap();
    let database = dir.path().join("atuin.db");
    assert!(Command::new(&sqlite)
        .arg(&database)
        .arg("CREATE TABLE t(v TEXT); INSERT INTO t VALUES('sqlite fixture');")
        .status()
        .unwrap()
        .success());
    let mozc = dir.path().join("mozc");
    fs::create_dir(&mozc).unwrap();
    fs::write(mozc.join("learning"), b"mozc fixture\n").unwrap();
    let systemctl = dir.path().join("systemctl");
    fs::write(&systemctl, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o700)).unwrap();
    let no_process = dir.path().join("no-process");
    fs::write(&no_process, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&no_process, fs::Permissions::from_mode(0o700)).unwrap();
    let curl = dir.path().join("curl");
    fs::write(&curl, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
    let nmcli = dir.path().join("nmcli");
    fs::write(
        &nmcli,
        "#!/bin/sh\nprintf 'GENERAL.STATE:100 (connected)\\nGENERAL.METERED:no\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&nmcli, fs::Permissions::from_mode(0o700)).unwrap();
    let mut r = Command::new(&restic);
    r.env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .arg("init");
    assert!(r.status().unwrap().success());
    let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":"git","path":repo},{"kind":"jsonl","path":jsonl},{"kind":"sqlite","path":database},{"kind":"mozc","path":mozc}],"excludes":["exclude-me.tmp"],"audit_roots":[dir.path()]}).to_string();
    let dry = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .args(["__run", "dry-run"])
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let output = String::from_utf8(dry.stdout).unwrap();
    assert!(output.contains("filesystem boundary:") && output.contains("device="));
    assert!(
        output.contains("exclude built-in\ttarget")
            && output.contains("exclude profile\texclude-me.tmp")
    );
    assert!(output
        .lines()
        .any(|line| line.starts_with("restic\tnew ") && line.contains("untracked")));
    assert!(!output.lines().any(|line| line.starts_with("restic\t")
        && (line.contains("exclude-me.tmp") || line.contains("target/generated"))));
    let output = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .env("LOCAL_STATE_CURL_BIN", &curl)
        .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
        .env("LOCAL_STATE_SQLITE_BIN", &sqlite)
        .env("LOCAL_STATE_SYSTEMCTL_BIN", &systemctl)
        .env("LOCAL_STATE_PKILL_BIN", &no_process)
        .env("LOCAL_STATE_PGREP_BIN", &no_process)
        .args(["__run", "backup"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let check = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .env("LOCAL_STATE_CURL_BIN", &curl)
        .args(["__run", "restore-test"])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let restored = dir.path().join("restore");
    let output = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .arg("restore")
        .arg(format!("latest:{}", repo.display()))
        .args(["--tag", "local-state"])
        .arg("--target")
        .arg(&restored)
        .arg("--verify")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(restored.join("untracked")).unwrap(),
        b"unique content\n"
    );
    let listing = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .args(["ls", "--json", "--tag", "local-state", "latest"])
        .output()
        .unwrap();
    assert!(listing.status.success());
    let staged = listing
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .filter_map(|v| {
            v.get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .find(|p| p.ends_with("/1.jsonl"))
        .unwrap();
    let staged_parent = std::path::Path::new(&staged).parent().unwrap();
    let restored_stage = dir.path().join("restore-stage");
    let stage_result = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .arg("restore")
        .arg(format!("latest:{}", staged_parent.display()))
        .args(["--tag", "local-state"])
        .arg("--target")
        .arg(&restored_stage)
        .args(["--include", "1.jsonl"])
        .arg("--verify")
        .output()
        .unwrap();
    assert!(
        stage_result.status.success(),
        "{}",
        String::from_utf8_lossy(&stage_result.stderr)
    );
    assert_eq!(
        fs::read(restored_stage.join("1.jsonl")).unwrap(),
        b"{\"event\":1}\n"
    );
    let restored_sqlite = dir.path().join("restore-sqlite");
    let sqlite_restore = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .arg("restore")
        .arg(format!("latest:{}", staged_parent.display()))
        .args(["--tag", "local-state"])
        .arg("--target")
        .arg(&restored_sqlite)
        .args(["--include", "2.sqlite", "--verify"])
        .output()
        .unwrap();
    assert!(
        sqlite_restore.status.success(),
        "{}",
        String::from_utf8_lossy(&sqlite_restore.stderr)
    );
    let sqlite_result = Command::new(&sqlite)
        .arg(restored_sqlite.join("2.sqlite"))
        .arg("SELECT v FROM t")
        .output()
        .unwrap();
    assert!(sqlite_result.status.success());
    assert_eq!(sqlite_result.stdout, b"sqlite fixture\n");
    let restored_mozc = dir.path().join("restore-mozc");
    let mozc_restore = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .arg("restore")
        .arg(format!("latest:{}/3.mozc", staged_parent.display()))
        .args(["--tag", "local-state"])
        .arg("--target")
        .arg(&restored_mozc)
        .arg("--verify")
        .output()
        .unwrap();
    assert!(
        mozc_restore.status.success(),
        "{}",
        String::from_utf8_lossy(&mozc_restore.stderr)
    );
    assert_eq!(
        fs::read(restored_mozc.join("learning")).unwrap(),
        b"mozc fixture\n"
    );
    let run = |operation: &str| {
        Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", &home)
            .env("LOCAL_STATE_PROFILE", &profile)
            .env("RESTIC_REPOSITORY", &store)
            .env("RESTIC_PASSWORD", "fixture")
            .env("AWS_ACCESS_KEY_ID", "fixture")
            .env("AWS_SECRET_ACCESS_KEY", "fixture")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
            .env("LOCAL_STATE_RESTIC_BIN", &restic)
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .args(["__run", operation])
            .output()
            .unwrap()
    };
    let preview = run("maintenance-dry-run");
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("keep 1, remove 0"));
    let calls = dir.path().join("restic-calls");
    let wrapper = dir.path().join("restic-wrapper");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexport RESTIC_REPOSITORY='{}'\nexec '{}' \"$@\"\n",
            calls.display(),
            store.display(),
            restic.to_string_lossy()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let heartbeat_log = dir.path().join("heartbeat-events");
    let b2_reply = |status: &str| {
        format!(
            r##"#!/bin/sh
input=$(cat)
case "$input" in
  *b2_authorize_account*) printf '%s\n' '{{"accountId":"account","authorizationToken":"synthetic","apiInfo":{{"storageApi":{{"apiUrl":"https://api001.backblazeb2.com","s3ApiUrl":"https://s3.us-west-001.backblazeb2.com","allowed":{{"buckets":[{{"id":"test"}}],"namePrefix":null,"capabilities":["listFiles","listBuckets"]}}}}}}}}' ;;
  *b2_list_buckets*) printf '%s\n' '{{"buckets":[{{"bucketId":"test","bucketName":"fixture-bucket"}}]}}' ;;
  *b2_list_file_versions*) printf '%s\n' '{{"files":[{{"action":"upload","fileName":"object","fileId":"1","replicationStatus":"{status}"}}],"nextFileName":null,"nextFileId":null}}' ;;
  *hc-ping.com*/start*) echo start >> '{}' ;;
  *hc-ping.com*/fail*) echo fail >> '{}' ;;
  *hc-ping.com*) echo success >> '{}' ;;
  *) exit 0 ;;
esac
"##,
            heartbeat_log.display(),
            heartbeat_log.display(),
            heartbeat_log.display()
        )
    };
    fs::write(&curl, b2_reply("PENDING")).unwrap();
    let maintain = |repository: &str, profile: &str| {
        Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", &home)
            .env("LOCAL_STATE_PROFILE", profile)
            .env("RESTIC_REPOSITORY", repository)
            .env("RESTIC_PASSWORD", "fixture")
            .env("AWS_ACCESS_KEY_ID", "fixture")
            .env("AWS_SECRET_ACCESS_KEY", "fixture")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
            .env("LOCAL_STATE_RESTIC_BIN", &wrapper)
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .args(["__run", "maintenance"])
            .output()
            .unwrap()
    };
    let repository = "s3:https://s3.us-west-001.backblazeb2.com/fixture-bucket";
    let pending = maintain(repository, &profile);
    assert!(!pending.status.success());
    assert!(!fs::read_to_string(&calls)
        .unwrap_or_default()
        .lines()
        .any(|line| line.starts_with("forget") || line.starts_with("prune")));
    for replacement in [
        "\"nextFileName\":\"object\",\"nextFileId\":null",
        "\"nextFileName\":null,\"nextFileId\":\"2\"",
    ] {
        fs::write(
            &curl,
            b2_reply("COMPLETED").replace("\"nextFileName\":null,\"nextFileId\":null", replacement),
        )
        .unwrap();
        fs::write(&calls, "").unwrap();
        assert!(!maintain(repository, &profile).status.success());
        assert!(!fs::read_to_string(&calls)
            .unwrap_or_default()
            .lines()
            .any(|line| line.starts_with("forget") || line.starts_with("prune")));
    }
    let orphan_tag = format!("for-{}", "0".repeat(64));
    let mut orphan = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .args([
            "backup",
            "--stdin",
            "--stdin-filename",
            "local-state-manifest.json",
            "--host",
            "12345678-1234-1234-1234-123456789abc",
            "--tag",
            "local-state-manifest",
            "--tag",
            &orphan_tag,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    use std::io::Write;
    orphan.stdin.take().unwrap().write_all(b"{}").unwrap();
    assert!(orphan.wait().unwrap().success());
    fs::write(
        &curl,
        b2_reply("COMPLETED").replace("\"listFiles\",\"listBuckets\"", "\"listFiles\""),
    )
    .unwrap();
    fs::write(&calls, "").unwrap();
    let insufficient = maintain(repository, &profile);
    assert!(!insufficient.status.success());
    assert!(!fs::read_to_string(&calls)
        .unwrap_or_default()
        .lines()
        .any(|line| line.starts_with("forget") || line.starts_with("prune")));
    fs::write(&curl, b2_reply("COMPLETED")).unwrap();
    let mut wrong_id: serde_json::Value = serde_json::from_str(&profile).unwrap();
    wrong_id["source_bucket_id"] = "other".into();
    let mut wrong_prefix: serde_json::Value = serde_json::from_str(&profile).unwrap();
    wrong_prefix["source_prefix"] = "other/".into();
    for (different_repository, different_profile) in [
        (
            "s3:https://s3.us-west-002.backblazeb2.com/fixture-bucket",
            profile.clone(),
        ),
        (
            "s3:https://s3.us-west-001.backblazeb2.com/other-bucket",
            profile.clone(),
        ),
        (
            "s3:https://s3.us-west-001.backblazeb2.com/fixture-bucket/other",
            profile.clone(),
        ),
        (repository, wrong_id.to_string()),
        (repository, wrong_prefix.to_string()),
    ] {
        fs::write(&calls, "").unwrap();
        let rejected = maintain(different_repository, &different_profile);
        assert!(!rejected.status.success());
        assert!(!fs::read_to_string(&calls)
            .unwrap_or_default()
            .lines()
            .any(|line| line.starts_with("forget") || line.starts_with("prune")));
    }
    let completed = maintain(repository, &profile);
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    assert!(fs::read_to_string(&calls)
        .unwrap()
        .lines()
        .any(|line| line.starts_with("prune")));
    let commands = fs::read_to_string(&calls).unwrap();
    let forget = commands
        .lines()
        .find(|line| line.starts_with("forget --host"))
        .unwrap();
    assert!(forget.contains("--group-by host"));
    assert!(forget.contains("--keep-daily 7 --keep-weekly 5 --keep-monthly 12"));
    let manifests = Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .args(["snapshots", "--json", "--tag", "local-state-manifest"])
        .output()
        .unwrap();
    let values: Vec<serde_json::Value> = serde_json::from_slice(&manifests.stdout).unwrap();
    assert_eq!(values.len(), 1, "orphan manifest should be removed");
    let id = values[0]["id"].as_str().unwrap();
    assert!(Command::new(&restic)
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .args(["forget", id])
        .status()
        .unwrap()
        .success());
    let missing = run("restore-test");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("manifest missing"));
    assert!(!run("maintenance-dry-run").status.success());
    let logs = home.join(".local/state/local-state-backup/local");
    let entries: Vec<_> = fs::read_dir(logs).unwrap().map(Result::unwrap).collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    let log = fs::read_to_string(entries[0].path()).unwrap();
    assert!(!log.contains(repo.to_str().unwrap()));
    assert!(!log.contains("fixture"));
    for excluded in [
        repo.to_str().unwrap(),
        "*.sqlite",
        ".git",
        "untracked",
        "tracked",
    ] {
        fs::write(&heartbeat_log, "").unwrap();
        let mut bad: serde_json::Value = serde_json::from_str(&profile).unwrap();
        bad["excludes"] = serde_json::json!([excluded]);
        let result = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", &home)
            .env("LOCAL_STATE_PROFILE", bad.to_string())
            .env("RESTIC_REPOSITORY", &store)
            .env("RESTIC_PASSWORD", "fixture")
            .env("AWS_ACCESS_KEY_ID", "fixture")
            .env("AWS_SECRET_ACCESS_KEY", "fixture")
            .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
            .env("LOCAL_STATE_RESTIC_BIN", &restic)
            .env("LOCAL_STATE_CURL_BIN", &curl)
            .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
            .env("LOCAL_STATE_SQLITE_BIN", &sqlite)
            .env("LOCAL_STATE_SYSTEMCTL_BIN", &systemctl)
            .env("LOCAL_STATE_PKILL_BIN", &no_process)
            .env("LOCAL_STATE_PGREP_BIN", &no_process)
            .args(["__run", "backup"])
            .output()
            .unwrap();
        assert!(!result.status.success(), "excluded {excluded}");
        assert!(!String::from_utf8_lossy(&result.stdout).contains("snapshot "));
        let events = fs::read_to_string(&heartbeat_log).unwrap();
        assert!(events.lines().any(|line| line == "fail"));
        assert!(!events.lines().any(|line| line == "success"));
    }
    let racing_git = dir.path().join("racing-git");
    fs::write(&racing_git, format!("#!/bin/sh\ncase \"$*\" in *ls-files*--others*) /usr/bin/git \"$@\"; rm -f '{}';; *) exec /usr/bin/git \"$@\";; esac\n", repo.join("untracked").display())).unwrap();
    fs::set_permissions(&racing_git, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&heartbeat_log, "").unwrap();
    let raced = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("LOCAL_STATE_PROFILE", &profile)
        .env("RESTIC_REPOSITORY", &store)
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("HEALTHCHECKS_URL", "https://hc-ping.com/fixture")
        .env("LOCAL_STATE_GIT_BIN", &racing_git)
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .env("LOCAL_STATE_CURL_BIN", &curl)
        .env("LOCAL_STATE_NMCLI_BIN", &nmcli)
        .env("LOCAL_STATE_SQLITE_BIN", &sqlite)
        .env("LOCAL_STATE_SYSTEMCTL_BIN", &systemctl)
        .env("LOCAL_STATE_PKILL_BIN", &no_process)
        .env("LOCAL_STATE_PGREP_BIN", &no_process)
        .args(["__run", "backup"])
        .output()
        .unwrap();
    assert!(!raced.status.success());
    assert!(String::from_utf8_lossy(&raced.stderr).contains("cannot inspect git file"));
    assert!(!fs::read_to_string(&heartbeat_log)
        .unwrap()
        .lines()
        .any(|line| line == "success"));
}
