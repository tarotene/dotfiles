use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn script(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    for attempt in 0..50 {
        match Command::new(path).arg("--preflight").status() {
            Err(error) if error.raw_os_error() == Some(26) && attempt < 49 => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            result => {
                result.unwrap();
                break;
            }
        }
    }
}

#[test]
fn bws_reparse_preserves_spaces_and_does_not_output_token() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home with spaces");
    fs::create_dir(&home).unwrap();
    let source = temp.path().join("source with spaces.jsonl");
    fs::write(&source, b"{}\n").unwrap();
    let profile = serde_json::json!({"identity":"12345678-1234-1234-1234-123456789abc","source_bucket_id":"test","source_prefix":"","sources":[{"kind":"jsonl","path":source}],"excludes":[],"audit_roots":[temp.path()]}).to_string();
    let secret = temp.path().join("secret-tool");
    script(
        &secret,
        "#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\nprintf 'synthetic-token\\n'\n",
    );
    let bws = temp.path().join("bws");
    let quoted = profile.replace('\'', "'\\''");
    script(&bws, &format!("#!/bin/sh\n[ \"$1\" = --preflight ] && exit 0\n[ \"$BWS_ACCESS_TOKEN\" = synthetic-token ] || exit 5\n[ \"$1\" = run ] && [ \"$2\" = --no-inherit-env ] && [ \"$3\" = -- ] || exit 6\nexec env -i LOCAL_STATE_PROFILE='{quoted}' /bin/sh -c \"$4\"\n"));
    let output = Command::new(env!("CARGO_BIN_EXE_local-state-backup"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("LOCAL_STATE_SECRET_TOOL_BIN", &secret)
        .env("LOCAL_STATE_BWS_BIN", &bws)
        .arg("inventory")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(source.to_str().unwrap()));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-token"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-token"));
}
