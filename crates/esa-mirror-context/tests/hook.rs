//! esa-mirror-context の統合テスト: 偽のミラー(一時 git repo)・偽のプロジェクト・
//! 設定ファイルを tempdir に作り、実バイナリを走らせる。実 org 名は使わない。

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath="])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_DATE", "2026-10-13T19:11:50Z")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

struct Fx {
    dir: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mirror = dir.path().join("mirror");
        std::fs::create_dir_all(&mirror).unwrap();
        git(&mirror, &["init", "-q", "-b", "main"]);
        std::fs::write(mirror.join("index.md"), "x").unwrap();
        git(&mirror, &["add", "."]);
        git(&mirror, &["commit", "-q", "-m", "dump"]);
        Fx { dir }
    }
    fn mirror(&self) -> std::path::PathBuf {
        self.dir.path().join("mirror")
    }
    fn project(&self, remote: Option<&str>) -> std::path::PathBuf {
        let p = self.dir.path().join("proj");
        std::fs::create_dir_all(&p).unwrap();
        git(&p, &["init", "-q", "-b", "main"]);
        if let Some(r) = remote {
            git(&p, &["remote", "add", "origin", r]);
        }
        p
    }
    fn config(&self, mirror: &Path, orgs: &[&str]) -> std::path::PathBuf {
        let p = self.dir.path().join("config.json");
        let body = serde_json::json!({"mirror_path": mirror, "orgs": orgs});
        std::fs::write(&p, body.to_string()).unwrap();
        p
    }
    fn run(&self, project: &Path, config: &Path) -> (String, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_esa-mirror-context"))
            .env("ESA_INDEX_CONFIG", config)
            .env("CLAUDE_PROJECT_DIR", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        let input = serde_json::json!({"cwd": project, "hook_event_name": "SessionStart"});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook must always exit 0");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

#[test]
fn injects_for_an_allowed_org() {
    let fx = Fx::new();
    let proj = fx.project(Some("https://github.com/Example-Org/x.git"));
    let cfg = fx.config(&fx.mirror(), &["example-org"]);
    let (out, err) = fx.run(&proj, &cfg);
    assert_eq!(err, "");
    let v: Value = serde_json::from_str(&out).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains(fx.mirror().to_str().unwrap()));
    assert!(ctx.contains("Last dump: 2026-10-13"));
    assert!(ctx.contains("source=\"esa-index\""));
}

#[test]
fn silent_when_org_not_listed() {
    let fx = Fx::new();
    let proj = fx.project(Some("https://github.com/other-org/x.git"));
    let cfg = fx.config(&fx.mirror(), &["example-org"]);
    assert_eq!(fx.run(&proj, &cfg), (String::new(), String::new()));
}

#[test]
fn silent_without_config() {
    let fx = Fx::new();
    let proj = fx.project(Some("https://github.com/example-org/x.git"));
    let missing = fx.dir.path().join("nope.json");
    assert_eq!(fx.run(&proj, &missing), (String::new(), String::new()));
}

#[test]
fn silent_without_origin() {
    let fx = Fx::new();
    let proj = fx.project(None);
    let cfg = fx.config(&fx.mirror(), &["example-org"]);
    assert_eq!(fx.run(&proj, &cfg), (String::new(), String::new()));
}

#[test]
fn one_stderr_line_when_mirror_is_missing() {
    let fx = Fx::new();
    let proj = fx.project(Some("https://github.com/example-org/x.git"));
    let cfg = fx.config(&fx.dir.path().join("gone"), &["example-org"]);
    let (out, err) = fx.run(&proj, &cfg);
    assert_eq!(out, "");
    assert_eq!(err.lines().count(), 1);
    assert!(err.starts_with("[esa-mirror-context] mirror path does not exist"));
}

#[test]
fn freshness_unknown_when_mirror_is_not_a_git_repo() {
    let fx = Fx::new();
    let plain = fx.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let proj = fx.project(Some("https://github.com/example-org/x.git"));
    let cfg = fx.config(&plain, &["example-org"]);
    let (out, _) = fx.run(&proj, &cfg);
    assert!(out.contains("Last dump: unknown"));
}
