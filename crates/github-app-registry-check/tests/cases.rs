//! bash 版 `scripts/github-app-registry-check --selftest` の写し(#414)。
//! bash 版の各検査(`bash#N`)を、バイナリの入出力(引数・stdout・終了コード)に
//! 言い換えて 1 対 1 で固定する。fixture は selftest の heredoc そのまま。

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const RELEASER: &str = r#"{"name": "tarotene-releaser", "url": "https://example.invalid", "hook_attributes": {"url": "https://example.invalid", "active": false}, "default_permissions": {"contents": "write", "issues": "write", "pull_requests": "write", "metadata": "read"}, "default_events": []}"#;
const DRIFTED: &str = r#"{"name": "tarotene-drifted", "url": "https://example.invalid", "hook_attributes": {"url": "https://example.invalid", "active": false}, "default_permissions": {"contents": "write", "metadata": "read"}, "default_events": []}"#;
const UNREGISTERED: &str = r#"{"name": "tarotene-unregistered", "url": "https://example.invalid", "hook_attributes": {"url": "https://example.invalid", "active": false}, "default_permissions": {"metadata": "read"}, "default_events": []}"#;
const SNAPSHOT: &str = r#"{"schema": 1, "generated_at": "2026-09-30T00:00:00Z", "apps": [
  {"name": "releaser", "id": 1, "slug": "tarotene-releaser", "app_name": "tarotene-releaser", "permissions": {"contents": "write", "issues": "write", "pull_requests": "write", "metadata": "read"}, "events": []},
  {"name": "drifted", "id": 2, "slug": "tarotene-drifted", "app_name": "tarotene-drifted", "permissions": {"contents": "write", "issues": "write", "metadata": "read"}, "events": ["push"]}
]}"#;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let o = Command::new(env!("CARGO_BIN_EXE_github-app-registry-check"))
        .args(args)
        .env_remove("GITHUB_APP_REGISTRY_CHECK_MANIFESTS_DIR")
        .env_remove("GITHUB_APP_REGISTRY_CHECK_SNAPSHOT_FILE")
        .output()
        .unwrap();
    Out {
        code: o.status.code().unwrap(),
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
    }
}

fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let m = tmp.path().join("manifests");
    fs::create_dir(&m).unwrap();
    fs::write(m.join("releaser.json"), format!("{RELEASER}\n")).unwrap();
    fs::write(m.join("drifted.json"), format!("{DRIFTED}\n")).unwrap();
    fs::write(m.join("unregistered.json"), format!("{UNREGISTERED}\n")).unwrap();
    fs::write(tmp.path().join("snapshot.json"), format!("{SNAPSHOT}\n")).unwrap();
    tmp
}

fn args<'a>(m: &'a Path, s: &'a Path, extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        "--manifests-dir",
        m.to_str().unwrap(),
        "--snapshot",
        s.to_str().unwrap(),
    ];
    v.extend_from_slice(extra);
    v
}

/// bash#1: snapshot ファイルが無ければ全 manifest が app-snapshot-missing。
#[test]
fn no_snapshot_marks_every_manifest_missing() {
    let tmp = fixture();
    let m = tmp.path().join("manifests");
    let s = tmp.path().join("no-such-file.json");
    let o = run(&args(&m, &s, &["--json"]));
    assert_eq!(
        o.stdout,
        concat!(
            r#"[{"app":"tarotene-drifted","verdict":"drifted","missing":["app-snapshot-missing"]},"#,
            r#"{"app":"tarotene-releaser","verdict":"drifted","missing":["app-snapshot-missing"]},"#,
            r#"{"app":"tarotene-unregistered","verdict":"drifted","missing":["app-snapshot-missing"]}]"#,
            "\n"
        )
    );
    assert_eq!(o.code, 1);
}

/// bash#2〜#5: 一致は ok、permissions+events の両 drift、snapshot に無い App、
/// drift があれば any_drift は終了コード 1。
#[test]
fn snapshot_present_judges_each_app() {
    let tmp = fixture();
    let m = tmp.path().join("manifests");
    let s = tmp.path().join("snapshot.json");
    let o = run(&args(&m, &s, &["--json"]));
    assert_eq!(
        o.stdout,
        concat!(
            r#"[{"app":"tarotene-drifted","verdict":"drifted","missing":["app-registry-permissions-drift","app-registry-events-drift"]},"#,
            r#"{"app":"tarotene-releaser","verdict":"ok","missing":[]},"#,
            r#"{"app":"tarotene-unregistered","verdict":"drifted","missing":["app-registry-missing-in-snapshot"]}]"#,
            "\n"
        )
    );
    assert_eq!(o.code, 1);
    let o = run(&args(&m, &s, &[]));
    assert_eq!(
        o.stdout,
        "app=tarotene-drifted verdict=drifted missing=app-registry-permissions-drift,app-registry-events-drift\n\
         app=tarotene-releaser verdict=ok\n\
         app=tarotene-unregistered verdict=drifted missing=app-registry-missing-in-snapshot\n\
         total: 3 app(s), ok=1 drifted=2\n"
    );
    assert_eq!(o.code, 1);
}

/// bash#6: 全部 ok なら any_drift は 0。
#[test]
fn all_ok_exits_zero() {
    let tmp = fixture();
    let m = tmp.path().join("manifests");
    fs::remove_file(m.join("drifted.json")).unwrap();
    fs::remove_file(m.join("unregistered.json")).unwrap();
    let s = tmp.path().join("snapshot.json");
    let o = run(&args(&m, &s, &[]));
    assert_eq!(
        o.stdout,
        "app=tarotene-releaser verdict=ok\ntotal: 1 app(s), ok=1 drifted=0\n"
    );
    assert_eq!(o.code, 0);
}

/// bash#7: manifests ディレクトリが無ければ空の結果で、エラーにしない。
#[test]
fn missing_manifests_dir_is_empty_not_error() {
    let tmp = fixture();
    let m = tmp.path().join("no-such-dir");
    let s = tmp.path().join("snapshot.json");
    let o = run(&args(&m, &s, &["--json"]));
    assert_eq!((o.stdout.as_str(), o.code), ("[]\n", 0));
    let o = run(&args(&m, &s, &[]));
    assert_eq!(
        (o.stdout.as_str(), o.code),
        ("total: 0 app(s), ok=0 drifted=0\n", 0)
    );
}

/// 引数処理(selftest の外。bash 版 main の挙動)。
#[test]
fn usage_and_help() {
    let o = run(&["--help"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.starts_with("usage: github-app-registry-check "));
    let o = run(&["--bogus"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.starts_with("usage: github-app-registry-check "));
    assert_eq!(run(&["--snapshot"]).code, 1);
}

/// events は並び順に依らず比べる(jq の `sort`)。
#[test]
fn events_compare_ignores_order() {
    let tmp = fixture();
    let m = tmp.path().join("manifests");
    fs::remove_file(m.join("drifted.json")).unwrap();
    fs::remove_file(m.join("unregistered.json")).unwrap();
    fs::write(
        m.join("releaser.json"),
        r#"{"name":"tarotene-releaser","default_permissions":{"a":"read"},"default_events":["push","issues"]}"#,
    )
    .unwrap();
    let s = tmp.path().join("snap.json");
    fs::write(
        &s,
        r#"{"apps":[{"app_name":"tarotene-releaser","permissions":{"a":"read"},"events":["issues","push"]}]}"#,
    )
    .unwrap();
    let o = run(&args(&m, &s, &[]));
    assert_eq!(o.code, 0, "{}", o.stdout);
}
