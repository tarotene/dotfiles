use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn actual_retention_preview_keeps_daily_weekly_and_monthly_periods() {
    let Some(restic) = std::env::var_os("TEST_RESTIC_BIN") else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = tmp.path().join("repo");
    let run = || {
        let mut c = Command::new(&restic);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", &home)
            .env("TZ", "UTC")
            .env("RESTIC_REPOSITORY", &repo)
            .env("RESTIC_PASSWORD", "fixture");
        c
    };
    assert!(run()
        .arg("init")
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success());
    let dates = [
        "2026-10-30",
        "2026-10-29",
        "2026-10-28",
        "2026-10-27",
        "2026-10-26",
        "2026-10-25",
        "2026-10-24",
        "2026-10-23",
        "2026-10-22",
        "2026-10-16",
        "2026-10-09",
        "2026-10-02",
        "2026-09-25",
        "2026-08-28",
        "2026-07-31",
        "2026-06-30",
        "2026-05-31",
        "2026-04-30",
        "2026-03-31",
        "2026-02-28",
        "2026-01-31",
        "2025-12-31",
        "2025-11-30",
        "2025-10-31",
        "2025-09-30",
    ];
    let id = "12345678-1234-1234-1234-123456789abc";
    let mut expected = std::collections::HashSet::new();
    for date in dates {
        let time = format!("{date} 12:00:00");
        let mut data = run();
        let mut process = data
            .args([
                "backup",
                "--json",
                "--stdin",
                "--stdin-filename",
                "fixture",
                "--host",
                id,
                "--tag",
                "local-state",
                "--time",
                &time,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        process.stdin.take().unwrap().write_all(b"fixture").unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(output.status.success());
        let snapshot = output
            .stdout
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .find_map(|v| {
                v.get("snapshot_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap();
        if !["2026-10-22", "2026-10-23", "2025-10-31", "2025-09-30"].contains(&date) {
            expected.insert(snapshot.clone());
        }
        let tag = format!("for-{snapshot}");
        let mut manifest = run()
            .args([
                "backup",
                "--stdin",
                "--stdin-filename",
                "local-state-manifest.json",
                "--host",
                id,
                "--tag",
                "local-state-manifest",
                "--tag",
                &tag,
                "--time",
                &time,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        manifest.stdin.take().unwrap().write_all(b"{}").unwrap();
        assert!(manifest.wait().unwrap().success());
    }
    let preview = run()
        .args([
            "forget",
            "--json",
            "--dry-run",
            "--host",
            id,
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
        ])
        .output()
        .unwrap();
    assert!(preview.status.success());
    let groups: Vec<serde_json::Value> = serde_json::from_slice(&preview.stdout).unwrap();
    let selected: std::collections::HashSet<_> = groups
        .iter()
        .flat_map(|v| v["keep"].as_array().into_iter().flatten())
        .map(|v| v["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(selected, expected);
    let profile = serde_json::json!({"identity":id,"source_bucket_id":"test","source_prefix":"","sources":[{"kind":"jsonl","path":"/tmp/example"}],"excludes":[],"audit_roots":["/tmp"]}).to_string();
    let result = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &home)
        .env("TZ", "UTC")
        .env("LOCAL_STATE_PROFILE", profile)
        .env("RESTIC_REPOSITORY", &repo)
        .env("RESTIC_PASSWORD", "fixture")
        .env("AWS_ACCESS_KEY_ID", "fixture")
        .env("AWS_SECRET_ACCESS_KEY", "fixture")
        .env("LOCAL_STATE_RESTIC_BIN", &restic)
        .args(["__run", "maintenance-dry-run"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("keep 21, remove 4"));
    let after: Vec<serde_json::Value> = serde_json::from_slice(
        &run()
            .args(["snapshots", "--json", "--host", id, "--tag", "local-state"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(
        after.len(),
        dates.len(),
        "dry-run must not delete data snapshots"
    );
}
