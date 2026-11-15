//! 実際のバイナリを、偽の `dotfiles-identity` と偽の通知ツール `notify-tool`(一時ディレクトリの PATH)で動かす。
//! GitHub にも、本物の通知のキャッシュにも触れない。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(identity_body: &str, scope_json: &str) -> (Output, String) {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    script(&bin, "dotfiles-identity", identity_body);
    fs::write(tmp.path().join("scope.json"), scope_json).unwrap();
    script(
        &bin,
        "notify-tool",
        &format!(
            "echo \"notify-tool $*\" >> {log}\ncase \"$1\" in scope) cat {scope} ;; sync) echo synced-now ;; esac",
            log = tmp.path().join("calls.log").display(),
            scope = tmp.path().join("scope.json").display()
        ),
    );
    let out = Command::new(env!("CARGO_BIN_EXE_desk-github-gate"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("DESK_NOTIFY_TOOL", "notify-tool")
        .env("XDG_CONFIG_HOME", tmp.path().join("no-config"))
        .output()
        .unwrap();
    let calls = fs::read_to_string(tmp.path().join("calls.log")).unwrap_or_default();
    (out, calls)
}

#[test]
fn a_matching_fresh_report_exits_zero_and_prints_the_report() {
    let json = r#"{"side":"personal","stale":false,"synced":true,"other_side":[],"unassigned":[]}"#;
    let (out, calls) = run("echo personal", json);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), json);
    assert!(!calls.contains("sync"), "{calls}");
}

#[test]
fn a_mismatched_side_exits_one_with_the_reason_on_stderr_and_nothing_on_stdout() {
    let json = r#"{"side":"company","stale":false,"synced":true}"#;
    let (out, _) = run("echo personal", json);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("desk-github-gate: 通知ツールの側(company)"),
        "{err}"
    );
}

#[test]
fn a_failed_identity_check_passes_its_own_reason_through_and_exits_one() {
    let json = r#"{"side":"personal","stale":false,"synced":true}"#;
    let (out, calls) = run("echo 'アカウントが違う' >&2; exit 1", json);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("アカウントが違う") && err.contains("identity とログイン中"),
        "{err}"
    );
    assert!(
        calls.is_empty(),
        "identity が通らなければ、通知ツールは呼ばない: {calls}"
    );
}

#[test]
fn the_sync_progress_goes_to_stderr_not_into_the_report() {
    // 古いキャッシュ(stale)は作り直す。ただし偽の scope は同じ応答を返すので、作り直しのあとも失敗する。
    // ここで見るのは、同期の経過が標準出力に混ざらないこと。
    let json = r#"{"side":"personal","stale":true,"synced":true}"#;
    let (out, calls) = run("echo personal", json);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("synced-now"));
    assert!(calls.contains("notify-tool sync"), "{calls}");
}

#[test]
fn the_tool_name_can_come_from_the_marker_file_and_is_required() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    script(&bin, "dotfiles-identity", "echo personal");
    script(&bin, "marker-tool", "cat \"$0.json\"");
    let json = r#"{"side":"personal","stale":false,"synced":true}"#;
    fs::write(bin.join("marker-tool.json"), json).unwrap();
    let conf = tmp.path().join("config/dotfiles");
    fs::create_dir_all(&conf).unwrap();
    let run = |marker: Option<&str>| {
        if let Some(m) = marker {
            fs::write(conf.join("desk-notify-tool"), format!("{m}\n")).unwrap();
        }
        Command::new(env!("CARGO_BIN_EXE_desk-github-gate"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XDG_CONFIG_HOME", tmp.path().join("config"))
            .env_remove("DESK_NOTIFY_TOOL")
            .output()
            .unwrap()
    };
    // 名前が無い: 明示して止まる(通知には触れない)。
    let out = run(None);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("通知ツールの名前が未設定"));
    // 目印ファイルの 1 行目を使う。
    let out = run(Some("marker-tool"));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), json);
}
