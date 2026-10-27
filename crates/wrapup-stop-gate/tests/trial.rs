mod common;
use common::{write_exec, Env, Run};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

struct Trial {
    env: Env,
}
impl Trial {
    fn new() -> Self {
        let t = Self { env: Env::new() };
        t.configure("2000-01-01T00:00:00Z", "2100-01-01T00:00:00Z");
        t.context("t1", "default");
        fs::write(t.env.path().join("comments.json"), "[[]]").unwrap();
        // Persist the remote comment before an optional transport failure.
        write_exec(
            &t.env.bin_dir().join("gh"),
            r#"#!/usr/bin/env python3
import json,os,sys,pathlib
root=pathlib.Path(os.environ['TEST_TRIAL_ROOT'])
if (root/'fail-read').exists() and sys.argv[1]=='api': sys.exit(1)
if sys.argv[1]=='api': print((root/'comments.json').read_text())
else:
 body=pathlib.Path(sys.argv[sys.argv.index('--body-file')+1]).read_text()
 (root/'comments.json').write_text(json.dumps([[{'body':body,'html_url':'https://github.com/example/notes/issues/1#issuecomment-7'}]]))
 with (root/'posts').open('a') as f: f.write('post\n')
 if (root/'fail-post').exists(): sys.exit(1)
 print('posted')
"#,
        );
        t
    }
    fn configure(&self, start: &str, end: &str) {
        fs::write(self.env.path().join("config.json"), json!({"enabled":true,"start":start,"end_exclusive":end,"repository":"example/notes","issue":1,"report_thread":"report","report_worktree":self.env.path().join("report"),"script":"script.md"}).to_string()).unwrap();
    }
    fn context(&self, turn: &str, mode: &str) {
        fs::write(self.env.path().join("rollout"), format!("{}\n{}\n", json!({"type":"turn_context","payload":{"turn_id":turn,"collaboration_mode":{"mode":mode}}}), json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":2}}}}))).unwrap();
        let dir = self.env.path().join("codex/sessions/2026/10/27");
        fs::create_dir_all(&dir).unwrap();
        let text = fs::read_to_string(self.env.path().join("rollout")).unwrap();
        fs::write(
            dir.join("report.jsonl"),
            format!(
                "{}\n{text}",
                json!({"type":"session_meta","payload":{"id":"report"}})
            ),
        )
        .unwrap();
    }
    fn seed(&self) {
        let root = self.env.state().join("claude/wrapup/trial");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("state.json"), json!({
            "checks":{"source:t1":{
                "session":"source", "turn":"t1", "transcript":self.env.path().join("rollout"),
                "status":"unchecked", "body":"", "approval":"", "comment_url":"",
                "attempts":0,"retry_turn":"", "applied_commit":"", "usage_before":null,"usage_after":null
            }}, "finalized_commit":""
        }).to_string()).unwrap();
    }
    fn cmd(&self, start: bool) -> Command {
        let mut c = Command::new(if start {
            common::session_start_path()
        } else {
            common::gate_path()
        });
        c.env("HOME", self.env.home())
            .env("WRAPUP_STATE_DIR", self.env.state())
            .env("WRAPUP_TRIAL_CONFIG", self.env.path().join("config.json"))
            .env("WRAPUP_TRIAL_AGENT", "codex")
            .env("CODEX_THREAD_ID", "report")
            .env("CODEX_HOME", self.env.path().join("codex"))
            .env("TEST_TRIAL_ROOT", self.env.path())
            .env("PATH", self.env.stub_path())
            // 実機の vault を読み書きしない(宣言はテストが明示したときだけ)。
            .env_remove("OBSIDIAN_VAULT")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("CLAUDE_PROJECT_DIR");
        c
    }
    fn hook(&self, sid: &str, turn: &str, recursive: bool, start: bool) -> Run {
        let mut p = self
            .cmd(start)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(p.stdin.take().unwrap(), "{}", json!({"session_id":sid,"turn_id":turn,"cwd":self.env.path(),"transcript_path":self.env.path().join("rollout"),"stop_hook_active":recursive,"source":"resume"})).unwrap();
        p.wait_with_output().unwrap().into()
    }
    fn cli(&self, args: &[&str]) -> Run {
        self.cmd(false)
            .arg("--trial")
            .args(args)
            .output()
            .unwrap()
            .into()
    }
    fn state(&self) -> Value {
        serde_json::from_str(&self.cli(&["list"]).stdout).unwrap()
    }
    fn draft(&self) -> String {
        let p = self.env.path().join("body.md");
        fs::write(&p, "Observed scene and impact. Owner impression pending.").unwrap();
        p.to_str().unwrap().into()
    }
}

#[test]
fn ordinary_hooks_never_collect_or_inject_even_with_pending_checks() {
    let t = Trial::new();
    for mode in ["default", "plan"] {
        t.context("short-answer", mode);
        for start in [true, false] {
            for recursive in [true, false] {
                let out = t.hook("source", "short-answer", recursive, start);
                assert_eq!(out.code, 0, "{}", out.stderr);
                assert!(!out.stdout.contains("Trial report"));
                assert!(!out.stderr.contains("trial"));
            }
        }
    }
    let root = t.env.state().join("claude/wrapup/trial");
    assert!(!root.exists());
    t.seed();
    let before = fs::read(root.join("state.json")).unwrap();
    for start in [true, false] {
        let out = t.hook("source", "t2", false, start);
        assert_eq!(out.code, 0);
        assert!(!out.stdout.contains("Trial report"));
        assert!(!out.stderr.contains("Trial report"));
    }
    assert_eq!(fs::read(root.join("state.json")).unwrap(), before);
    assert!(!root.join("state.lock").exists());
}

#[test]
fn list_and_report_hooks_are_pure_reads_and_procedure_is_explicit() {
    use std::os::unix::fs::PermissionsExt;
    let t = Trial::new();
    let root = t.env.state().join("claude/wrapup/trial");
    assert_eq!(t.cli(&["list"]).code, 0);
    assert!(!root.exists());
    t.seed();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    fs::set_permissions(root.join("state.json"), fs::Permissions::from_mode(0o400)).unwrap();
    let before = fs::read(root.join("state.json")).unwrap();
    assert_eq!(t.cli(&["list"]).code, 0);
    for mode in ["default", "plan"] {
        t.context("r1", mode);
        let start = t.hook("report", "r1", false, true);
        assert_eq!(start.code, 0);
        assert!(start.stdout.contains("Trial report: 1 pending"));
        assert!(!start.stdout.contains("Trial feedback: inspect"));
        let stop = t.hook("report", "r1", true, false);
        assert_eq!(stop.code, 0);
        assert!(stop.stderr.contains("Trial report: 1 pending"));
    }
    assert_eq!(fs::read(root.join("state.json")).unwrap(), before);
    assert!(!root.join("state.lock").exists());
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o500
    );
    let full = t.cli(&["procedure"]);
    assert_eq!(full.code, 0);
    assert!(full.stdout.contains("Trial feedback: inspect"));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn only_report_thread_in_default_can_mutate_legacy_observations() {
    let t = Trial::new();
    t.seed();
    let body = t.draft();
    let source: Run = t
        .cmd(false)
        .env("CODEX_THREAD_ID", "source")
        .args(["--trial", "draft", "source:t1", &body])
        .output()
        .unwrap()
        .into();
    assert_eq!(source.code, 1);
    t.context("r1", "plan");
    let before = t.state();
    for args in [
        vec!["draft", "source:t1", &body],
        vec!["none", "source:t1"],
        vec!["report-wait"],
    ] {
        assert_eq!(t.cli(&args).code, 1);
    }
    assert_eq!(t.state(), before);
    assert!(!t
        .env
        .state()
        .join("claude/wrapup/trial/state.lock")
        .exists());
    t.context("r2", "default");
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 0);
    fs::write(&body, "Changed wording").unwrap();
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    assert_eq!(t.state()["checks"]["source:t1"]["approval"], "");
    fs::write(&body, "Changed again").unwrap();
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 1);
}

#[test]
fn report_mode_uses_archived_report_rollout_and_fails_closed_if_missing() {
    let t = Trial::new();
    t.seed();
    let body = t.draft();
    let sessions = t.env.path().join("codex/sessions");
    fs::rename(&sessions, t.env.path().join("codex/archived_sessions")).unwrap();
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    fs::remove_dir_all(t.env.path().join("codex/archived_sessions")).unwrap();
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 1);
}

#[test]
fn post_failure_readback_retry_does_not_ack_application() {
    let t = Trial::new();
    t.seed();
    let body = t.draft();
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 0);
    // The source rollout is now Plan; the report's confirmed Default mode owns posting.
    fs::write(
        t.env.path().join("rollout"),
        "{\"type\":\"turn_context\",\"payload\":{\"collaboration_mode\":{\"mode\":\"plan\"}}}\n",
    )
    .unwrap();
    fs::write(t.env.path().join("fail-read"), "").unwrap();
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert!(!t.env.path().join("posts").exists());
    fs::remove_file(t.env.path().join("fail-read")).unwrap();
    fs::write(t.env.path().join("fail-post"), "").unwrap();
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert_eq!(t.state()["checks"]["source:t1"]["status"], "awaiting_post");
    assert_eq!(t.cli(&["post", "source:t1"]).code, 0);
    assert_eq!(
        fs::read_to_string(t.env.path().join("posts")).unwrap(),
        "post\n"
    );
    assert_eq!(t.state()["checks"]["source:t1"]["applied_commit"], "");
    assert_eq!(t.hook("report", "r1", false, false).code, 0);
}

#[test]
fn saved_script_and_remote_readback_are_required_for_ack() {
    let t = Trial::new();
    t.seed();
    let body = t.draft();
    t.cli(&["draft", "source:t1", &body]);
    t.cli(&["approve", "source:t1", &body, "OK"]);
    assert_eq!(t.cli(&["post", "source:t1"]).code, 0);
    let repo = t.env.path().join("report");
    common::repo(&repo, None);
    common::git(&repo, &["config", "user.name", "Test"]);
    common::git(&repo, &["config", "user.email", "test@example.invalid"]);
    fs::write(repo.join("script.md"), "unrelated script").unwrap();
    common::git(&repo, &["add", "script.md"]);
    common::git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "first"],
    );
    let sha = || {
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string()
    };
    assert_eq!(t.cli(&["applied", "source:t1", &sha()]).code, 1);
    fs::write(repo.join("script.md"), "A synthesized beat.\n<!-- codex-trial:source:t1 -->\nhttps://github.com/example/notes/issues/1#issuecomment-7\n").unwrap();
    common::git(&repo, &["add", "script.md"]);
    common::git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "apply"],
    );
    fs::write(t.env.path().join("fail-read"), "").unwrap();
    assert_eq!(t.cli(&["applied", "source:t1", &sha()]).code, 1);
    assert_eq!(t.state()["checks"]["source:t1"]["applied_commit"], "");
    fs::remove_file(t.env.path().join("fail-read")).unwrap();
    assert_eq!(t.cli(&["applied", "source:t1", &sha()]).code, 0);
    assert_eq!(t.cli(&["applied", "source:t1", &sha()]).code, 0);
}

#[test]
fn standing_authorization_is_preserved_but_plan_never_posts() {
    let t = Trial::new();
    t.seed();
    let config_path = t.env.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["posting_authorization"] = json!("Post trial observations without asking each time.");
    fs::write(&config_path, config.to_string()).unwrap();
    let body = t.draft();
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    assert_eq!(t.state()["checks"]["source:t1"]["status"], "awaiting_post");
    t.context("r2", "plan");
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert!(!t.env.path().join("posts").exists());
    t.context("r3", "default");
    assert_eq!(t.cli(&["post", "source:t1"]).code, 0);
    assert!(t.state()["checks"]["source:t1"]["approval"]
        .as_str()
        .unwrap()
        .starts_with("Standing authorization:"));
    config["posting_authorization"] = json!("  ");
    fs::write(config_path, config.to_string()).unwrap();
    assert_eq!(t.cli(&["list"]).code, 1);
}

#[test]
fn corrupt_trial_state_cannot_interrupt_ordinary_conversations() {
    let t = Trial::new();
    t.seed();
    fs::write(
        t.env.state().join("claude/wrapup/trial/state.json"),
        "broken",
    )
    .unwrap();
    for start in [true, false] {
        let out = t.hook("source", "t1", false, start);
        assert_eq!(out.code, 0);
        assert!(!out.stderr.contains("trial:"));
    }
    assert_eq!(t.hook("report", "r1", false, false).code, 0);
}
