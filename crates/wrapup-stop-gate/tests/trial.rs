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
            .env("CODEX_THREAD_ID", "source")
            .env("TEST_TRIAL_ROOT", self.env.path())
            .env("PATH", self.env.stub_path())
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
fn bounded_continuations_and_next_input_resume() {
    let t = Trial::new();
    assert_eq!(t.hook("source", "t1", false, false).code, 2);
    for _ in 0..2 {
        assert_eq!(t.hook("source", "t1", true, false).code, 2);
    }
    let exhausted = t.hook("source", "t1", true, false);
    assert_eq!(exhausted.code, 0);
    assert!(exhausted.stderr.contains("still pending"));
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
    t.context("t2", "default");
    assert_eq!(t.hook("source", "t2", false, false).code, 2);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
    assert_eq!(t.hook("source", "t2", false, true).code, 0);
    assert_eq!(t.state()["checks"]["source:t1"]["attempts"], 0);
}

#[test]
fn candidate_none_and_outside_window() {
    let t = Trial::new();
    t.configure("2000-01-01T00:00:00Z", "2001-01-01T00:00:00Z");
    assert_eq!(t.hook("source", "t1", false, false).code, 0);
    assert!(t.state()["checks"].as_object().unwrap().is_empty());
    t.configure("2000-01-01T00:00:00Z", "2100-01-01T00:00:00Z");
    t.hook("source", "t1", false, false);
    assert_eq!(t.cli(&["none", "source:t1"]).code, 0);
    assert_eq!(t.hook("source", "t1", true, false).code, 0);
    t.context("t2", "default");
    t.hook("source", "t2", false, false);
    t.configure("2000-01-01T00:00:00Z", "2001-01-01T00:00:00Z");
    assert_eq!(t.hook("source", "t2", true, false).code, 2); // leftovers survive expiry
}

#[test]
fn confirmation_plan_and_changed_draft() {
    let t = Trial::new();
    t.context("t1", "plan");
    assert_eq!(t.hook("source", "t1", false, false).code, 2);
    let body = t.draft();
    assert_eq!(t.cli(&["draft", "source:t1", &body]).code, 0);
    assert_eq!(t.hook("source", "t1", true, false).code, 0);
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 0);
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    fs::write(&body, "Changed wording").unwrap();
    t.cli(&["draft", "source:t1", &body]);
    assert_eq!(t.state()["checks"]["source:t1"]["approval"], "");
    fs::write(&body, "Changed again").unwrap();
    assert_eq!(t.cli(&["approve", "source:t1", &body, "OK"]).code, 1);
}

#[test]
fn post_failure_readback_retry_and_delivery_are_not_ack() {
    let t = Trial::new();
    t.hook("source", "t1", false, false);
    let body = t.draft();
    t.cli(&["draft", "source:t1", &body]);
    t.cli(&["approve", "source:t1", &body, "OK"]);
    fs::write(t.env.path().join("fail-read"), "").unwrap();
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert!(!t.env.path().join("posts").exists());
    fs::remove_file(t.env.path().join("fail-read")).unwrap();
    fs::write(t.env.path().join("fail-post"), "").unwrap();
    assert_eq!(t.cli(&["post", "source:t1"]).code, 1);
    assert_eq!(t.state()["checks"]["source:t1"]["status"], "awaiting_post");
    assert_eq!(t.cli(&["post", "source:t1"]).code, 0); // remote already has marker
    assert_eq!(
        fs::read_to_string(t.env.path().join("posts")).unwrap(),
        "post\n"
    );
    assert_eq!(t.cli(&["delivered", "source:t1"]).code, 0);
    assert_eq!(t.hook("source", "t1", true, false).code, 0);
    assert_eq!(t.state()["checks"]["source:t1"]["applied_commit"], "");
    let report = t.hook("report", "r1", false, true);
    assert!(report.stdout.contains("issuecomment-7"));
    assert_eq!(t.hook("report", "r1", false, false).code, 2);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1); // report never collects
    assert_eq!(t.cli(&["applied", "source:t1", &"a".repeat(40)]).code, 1);
}

#[test]
fn parallel_stops_do_not_lose_checks() {
    let t = Trial::new();
    std::thread::scope(|scope| {
        for i in 0..12 {
            let t = &t;
            scope.spawn(move || {
                assert_eq!(t.hook(&format!("s{i}"), "t1", false, false).code, 2);
            });
        }
    });
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 12);
}

#[test]
fn absent_config_and_other_agents_do_not_collect() {
    let t = Trial::new();
    let out = t
        .cmd(false)
        .env("WRAPUP_TRIAL_AGENT", "claude")
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Trial feedback"));
    fs::remove_file(t.env.path().join("config.json")).unwrap();
    assert_eq!(t.hook("source", "t1", false, false).code, 0);
    assert!(!t
        .env
        .state()
        .join("claude/wrapup/trial/state.json")
        .exists());
}

#[test]
fn saved_script_and_remote_readback_are_required_for_ack() {
    let t = Trial::new();
    t.hook("source", "t1", false, false);
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
    let ack = |commit: &str| -> Run {
        t.cmd(false)
            .env("CODEX_THREAD_ID", "report")
            .args(["--trial", "applied", "source:t1", commit])
            .output()
            .unwrap()
            .into()
    };
    assert_eq!(ack(&sha()).code, 1);
    fs::write(repo.join("script.md"), "A synthesized beat.\n<!-- codex-trial:source:t1 -->\nhttps://github.com/example/notes/issues/1#issuecomment-7\n").unwrap();
    common::git(&repo, &["add", "script.md"]);
    common::git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "apply"],
    );
    fs::write(t.env.path().join("fail-read"), "").unwrap();
    assert_eq!(ack(&sha()).code, 1);
    assert_eq!(t.state()["checks"]["source:t1"]["applied_commit"], "");
    fs::remove_file(t.env.path().join("fail-read")).unwrap();
    assert_eq!(ack(&sha()).code, 0);
    assert_eq!(ack(&sha()).code, 0); // duplicate delivery/application is idempotent
    assert_eq!(t.hook("report", "r1", false, false).code, 0);
}
