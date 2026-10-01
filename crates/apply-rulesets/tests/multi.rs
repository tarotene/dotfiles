//! `scripts/github-rulesets-apply --selftest` の全 6 ケースを移した統合テスト
//! (#414)。bash selftest は `apply-rulesets.sh` をスタブして呼ばれ方を記録して
//! いた。ここでも `GITHUB_RULESETS_APPLY_SELF_SCRIPT` で実行可能なスタブへ
//! 差し替える(直接 exec されるので shebang が要る)。

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

const STUB: &str = r#"#!/bin/sh
if [ -n "${STUB_FAIL_REPO:-}" ] && [ "$1" = "$STUB_FAIL_REPO" ]; then
  printf 'CALLED SELF(fail): %s\n' "$*" >>"$STUB_LOG"
  exit 1
fi
printf 'CALLED SELF: %s\n' "$*" >>"$STUB_LOG"
"#;

struct Fx {
    dir: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("self-apply-rulesets.sh");
        std::fs::write(&s, STUB).unwrap();
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.path().join("calls.log"), "").unwrap();
        Fx { dir }
    }

    fn run_with(&self, args: &[&str], fail_repo: Option<&str>, script: Option<PathBuf>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_github-rulesets-apply"));
        c.args(args)
            .env(
                "GITHUB_RULESETS_APPLY_SELF_SCRIPT",
                script.unwrap_or_else(|| self.dir.path().join("self-apply-rulesets.sh")),
            )
            .env("STUB_LOG", self.dir.path().join("calls.log"));
        if let Some(r) = fail_repo {
            c.env("STUB_FAIL_REPO", r);
        }
        c.output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, None, None)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap()
    }
}

fn rc(o: &Output) -> i32 {
    o.status.code().unwrap()
}

#[test]
fn t1_single_repo() {
    let fx = Fx::new();
    assert_eq!(rc(&fx.run(&["tarotene/example-repo"])), 0);
    assert!(
        fx.log().contains("CALLED SELF: tarotene/example-repo"),
        "{}",
        fx.log()
    );
}

#[test]
fn t2_multiple_repos_call_once_each() {
    let fx = Fx::new();
    assert_eq!(rc(&fx.run(&["tarotene/a", "tarotene/b", "tarotene/c"])), 0);
    let log = fx.log();
    for r in ["a", "b", "c"] {
        assert!(log.contains(&format!("CALLED SELF: tarotene/{r}")), "{log}");
    }
    assert_eq!(log.lines().count(), 3, "{log}");
}

#[test]
fn t3_common_flags_are_forwarded() {
    let fx = Fx::new();
    let o = fx.run(&[
        "--ref",
        "feat",
        "--reconcile",
        "--dry-run",
        "--unverified-contexts",
        "tarotene/example-repo",
    ]);
    assert_eq!(rc(&o), 0);
    assert_eq!(
        fx.log(),
        "CALLED SELF: tarotene/example-repo --ref feat --reconcile --dry-run --unverified-contexts\n"
    );
}

#[test]
fn t4_args_after_double_dash_pass_through() {
    let fx = Fx::new();
    assert_eq!(
        rc(&fx.run(&["tarotene/example-repo", "--", "--verify-sha", "deadbeef"])),
        0
    );
    assert!(fx.log().contains("--verify-sha deadbeef"), "{}", fx.log());
}

#[test]
fn t5_argument_errors_fail() {
    let fx = Fx::new();
    let o = fx.run(&["example-repo"]);
    assert_eq!(rc(&o), 2);
    assert!(String::from_utf8_lossy(&o.stderr).contains("'example-repo' is not owner/repo"));
    let o = fx.run(&[]);
    assert_eq!(rc(&o), 2);
    assert!(String::from_utf8_lossy(&o.stderr).contains("at least one owner/repo is required"));
    let o = fx.run(&["--bogus", "tarotene/a"]);
    assert_eq!(rc(&o), 2);
    assert_eq!(fx.log(), "");
}

#[test]
fn t6_one_failing_repo_does_not_stop_the_others_but_fails_overall() {
    let fx = Fx::new();
    let o = fx.run_with(
        &["tarotene/a", "tarotene/b", "tarotene/c"],
        Some("tarotene/b"),
        None,
    );
    assert_eq!(rc(&o), 1);
    let log = fx.log();
    assert!(log.contains("CALLED SELF: tarotene/a"), "{log}");
    assert!(log.contains("CALLED SELF(fail): tarotene/b"), "{log}");
    assert!(log.contains("CALLED SELF: tarotene/c"), "{log}");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("\nFAILED (1):\n  tarotene/b\n"), "{e}");
}

#[test]
fn missing_apply_script_is_reported_as_failure_for_each_repo() {
    let fx = Fx::new();
    let o = fx.run_with(
        &["tarotene/a", "tarotene/b"],
        None,
        Some(fx.dir.path().join("nope")),
    );
    assert_eq!(rc(&o), 1);
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("not found or not executable (home-manager 未配備の可能性)"),
        "{e}"
    );
    assert!(e.contains("FAILED (2):"), "{e}");
}
