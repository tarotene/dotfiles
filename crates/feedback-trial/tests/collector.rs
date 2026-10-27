//! Exercise the public CLI and native-exec boundary without touching live state.
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;

struct Trial {
    dir: TempDir,
}
impl Trial {
    fn new() -> Self {
        let s = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(s.path("codex/sessions")).unwrap();
        fs::write(s.path("codex/auth.json"), "{}").unwrap();
        fs::write(s.path("codex/config.toml"), "model = \"fixture-model\"\nmodel_reasoning_effort = \"high\"\n[mcp_servers.private]\ncommand = \"must-not-run\"\n").unwrap();
        fs::write(s.path("config.json"), serde_json::to_vec(&json!({
            "enabled": true, "start": "2020-01-01T00:00:00Z",
            "end_exclusive": "2020-01-02T00:00:00Z", "repository": "OWNER/REPO",
            "issue": 1, "report_thread": "report", "report_worktree": s.path("report"), "script": "script.md"
        })).unwrap()).unwrap();
        fs::write(s.path("fake-codex"), r#"#!/usr/bin/python3
import json, os, pathlib, sys, time
root = pathlib.Path(__file__).parent
prompt = sys.stdin.read()
batch = json.loads(prompt.splitlines()[-1])
home = pathlib.Path(os.environ['CODEX_HOME'])
with (root / 'calls.jsonl').open('a') as f:
    f.write(json.dumps({'argv': sys.argv[1:], 'env': dict(os.environ), 'config': (home / 'config.toml').read_text(), 'batch': batch, 'auth': str((home / 'auth.json').resolve())}) + '\n')
mode = (root / 'mode').read_text() if (root / 'mode').exists() else 'candidate'
if mode == 'block':
    (root / 'started').write_text('ready')
    deadline = time.monotonic() + 10
    while not (root / 'release').exists() and time.monotonic() < deadline:
        time.sleep(0.01)
if mode == 'fail':
    sys.exit(7)
out = pathlib.Path(sys.argv[sys.argv.index('--output-last-message') + 1])
if mode == 'invalid':
    out.write_text('{invalid')
    sys.exit(0)
def decision(s):
    line = s['context_lines'][-1] if mode == 'context' else 1 if mode == 'unrelated-line' else s['first_line']
    obs = [] if mode == 'none' else [{'scene':'owner asks a short question', 'behavior':'assistant answers with context', 'impact':'work can continue', 'kind':'difficult' if 'DIFFICULT' in s['text'] else 'good', 'lines':[line]}]
    return {'id':s['id'], 'observations':obs}
out.write_text(json.dumps({'decisions': [decision(s) for s in batch]}))
"#).unwrap();
        fs::set_permissions(s.path("fake-codex"), fs::Permissions::from_mode(0o700)).unwrap();
        s
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_feedback-trial"));
        c.env_clear()
            .env("HOME", self.dir.path())
            .env("PATH", "/usr/bin:/bin")
            .env("CODEX_HOME", self.path("codex"))
            .env("WRAPUP_STATE_DIR", self.path("state"))
            .env("WRAPUP_TRIAL_CONFIG", self.path("config.json"))
            .env("FEEDBACK_TRIAL_CODEX_BIN", self.path("fake-codex"))
            .env("GH_TOKEN", "must-not-leak")
            .env("GITHUB_TOKEN", "must-not-leak")
            .env("OPENAI_API_KEY", "must-not-leak");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.path("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn state(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.path("state/claude/wrapup/trial/state.json")).unwrap(),
        )
        .unwrap()
    }
    fn log(
        &self,
        file: &str,
        sid: &str,
        at: &str,
        source: Value,
        terminal: Option<&str>,
        body: &str,
    ) {
        let turn = "turn1";
        let mut rows = vec![
            json!({"type":"session_meta", "payload":{"id":sid, "source":source}}),
            json!({"type":"event_msg", "timestamp":at,"payload":{"type":"task_started", "turn_id":turn}}),
            json!({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":body}]}}),
            json!({"type":"response_item", "payload":{"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Here is the answer."}]}}),
        ];
        if let Some(t) = terminal {
            rows.push(json!({"type":"event_msg", "payload":{"type":t, "turn_id":turn}}));
        }
        let path = self.path(&format!("codex/{file}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = rows
            .into_iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>();
        fs::write(path, text).unwrap();
    }
    fn ordinary(&self, sid: &str, body: &str) {
        self.log(
            &format!("sessions/{sid}.jsonl"),
            sid,
            "2020-01-01T00:00:00Z",
            json!("cli"),
            Some("task_complete"),
            body,
        );
    }
}

#[test]
fn differential_collection_preserves_short_answers_and_skips_decided_input() {
    let t = Trial::new();
    t.ordinary("normal", "yes");
    let before = t.ok(&["collect", "--dry-run"]);
    let fragments: Value = serde_json::from_slice(&before.stdout).unwrap();
    assert!(fragments[0]["text"].as_str().unwrap().contains("user: yes"));
    assert!(!t.path("state").exists());
    assert!(t.calls().is_empty());
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 1);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
    let out = t.ok(&["collect"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("model calls=0"));
    assert_eq!(t.calls().len(), 1);
    t.ordinary("later", "DIFFICULT");
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 2);
    assert!(t.state()["checks"]
        .as_object()
        .unwrap()
        .values()
        .any(|v| v["observation"]["kind"] == "difficult"));
}

#[test]
fn native_exec_has_isolated_configuration_and_no_service_credentials() {
    let t = Trial::new();
    t.ordinary("normal", "yes");
    t.ok(&["collect"]);
    let calls = t.calls();
    let call = &calls[0];
    let args = call["argv"].as_array().unwrap();
    assert!(args.contains(&json!("--ephemeral")));
    assert!(args.contains(&json!("--output-schema")));
    assert!(args.contains(&json!("--output-last-message")));
    assert_ne!(
        call["env"]["CODEX_HOME"],
        t.path("codex").to_string_lossy().as_ref()
    );
    for k in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "OPENAI_API_KEY",
        "WRAPUP_TRIAL_CONFIG",
    ] {
        assert!(call["env"].get(k).is_none(), "leaked {k}");
    }
    let config = call["config"].as_str().unwrap();
    for expected in [
        "model = \"fixture-model\"",
        "model_reasoning_effort = \"high\"",
        "approval_policy = \"never\"",
        "sandbox_mode = \"read-only\"",
        "web_search = \"disabled\"",
        "shell_tool = false",
        "apps = false",
        "plugins = false",
        "hooks = false",
    ] {
        assert!(config.contains(expected), "missing {expected}");
    }
    assert!(!config.contains("mcp_servers"));
    assert_eq!(
        call["auth"],
        t.path("codex/auth.json").to_string_lossy().as_ref()
    );
    assert!(fs::read_to_string(t.path("codex/config.toml"))
        .unwrap()
        .contains("must-not-run"));
}

#[test]
fn failures_and_invalid_results_retry_without_consuming_input() {
    let t = Trial::new();
    t.ordinary("normal", "yes");
    for mode in ["fail", "invalid"] {
        fs::write(t.path("mode"), mode).unwrap();
        assert!(!t.run(&["collect"]).status.success());
        let state = t.state();
        assert!(state["collector"]["done"].as_array().unwrap().is_empty());
        assert!(state["checks"].as_object().unwrap().is_empty());
        assert!(!state["collector"]["last_error"]
            .as_str()
            .unwrap()
            .is_empty());
    }
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    assert_eq!(t.state()["collector"]["done"].as_array().unwrap().len(), 1);
    assert!(t.state()["checks"].as_object().unwrap().is_empty());
    assert_eq!(t.state()["collector"]["last_error"], "");
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 3);
}

#[test]
fn expired_trial_collects_only_closed_in_period_root_conversations_including_archives() {
    let t = Trial::new();
    t.ordinary("inclusive-start", "yes");
    for (sid, at, source, terminal, directory) in [
        (
            "archived",
            "2020-01-01T23:59:59Z",
            json!("cli"),
            Some("turn_aborted"),
            "archived_sessions",
        ),
        (
            "before",
            "2019-12-31T23:59:59Z",
            json!("cli"),
            Some("task_complete"),
            "sessions",
        ),
        (
            "exclusive-end",
            "2020-01-02T00:00:00Z",
            json!("cli"),
            Some("task_complete"),
            "sessions",
        ),
        (
            "report",
            "2020-01-01T12:00:00Z",
            json!("cli"),
            Some("task_complete"),
            "sessions",
        ),
        (
            "child",
            "2020-01-01T12:00:00Z",
            json!({"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}),
            Some("task_complete"),
            "sessions",
        ),
        (
            "incomplete",
            "2020-01-01T12:00:00Z",
            json!("cli"),
            None,
            "sessions",
        ),
    ] {
        t.log(
            &format!("{directory}/{sid}.jsonl"),
            sid,
            at,
            source,
            terminal,
            "yes",
        );
    }
    let result = t.ok(&["collect", "--dry-run"]);
    let segments: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
    let mut sessions: Vec<_> = segments
        .iter()
        .map(|v| v["session"].as_str().unwrap())
        .collect();
    sessions.sort_unstable();
    assert_eq!(sessions, ["archived", "inclusive-start"]);
    t.ok(&["collect"]);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 2);
}

#[test]
fn legacy_records_are_not_recollected_and_are_backed_up_unchanged() {
    let t = Trial::new();
    t.ordinary("legacy", "yes");
    t.ordinary("new", "yes");
    let state_path = t.path("state/claude/wrapup/trial/state.json");
    fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    let old = json!({"checks":{"legacy:turn1": {
        "session":"legacy", "turn":"turn1", "transcript":"old.jsonl", "status":"awaiting_post",
        "body":"approved body", "approval":"yes, publish", "comment_url":"", "attempts":0,
        "retry_turn":"", "applied_commit":"", "usage_before":null, "usage_after":null
    }}, "finalized_commit":""});
    let bytes = serde_json::to_vec(&old).unwrap();
    fs::write(&state_path, &bytes).unwrap();
    t.ok(&["collect"]);
    assert_eq!(t.calls()[0]["batch"].as_array().unwrap().len(), 1);
    assert_eq!(t.calls()[0]["batch"][0]["session"], "new");
    assert_eq!(
        fs::read(state_path.with_file_name("state.pre-collector.json")).unwrap(),
        bytes
    );
    let saved = t.state();
    assert_eq!(saved["checks"]["legacy:turn1"]["approval"], "yes, publish");
    assert_eq!(saved["checks"]["legacy:turn1"]["body"], "approved body");
}

#[test]
fn large_unicode_intervals_keep_all_fragments_across_batches() {
    let t = Trial::new();
    let body = "あいうえお".repeat(9000);
    t.ordinary("large", &body);
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    let calls = t.calls();
    assert!(calls.len() >= 2, "fixture must exceed a batch");
    let segments: Vec<_> = calls
        .iter()
        .flat_map(|v| v["batch"].as_array().unwrap())
        .collect();
    let combined: String = segments
        .iter()
        .map(|s| s["text"].as_str().unwrap())
        .collect();
    assert!(
        combined == format!("\n[line 3] user: {body}\n\n[line 4] assistant: Here is the answer.\n"),
        "fragment concatenation changed source text (actual bytes={}, body bytes={})",
        combined.len(),
        body.len()
    );
    assert_eq!(
        t.state()["collector"]["done"].as_array().unwrap().len(),
        segments.len()
    );
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), calls.len());
}

#[test]
fn list_is_read_only_even_when_existing_state_directory_is_not_writable() {
    let t = Trial::new();
    t.ok(&["list"]);
    assert!(!t.path("state").exists());
    let root = t.path("state/claude/wrapup/trial");
    fs::create_dir_all(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    t.ok(&["list"]);
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o500
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn missing_disabled_or_finalized_trial_never_invokes_model() {
    let t = Trial::new();
    t.ordinary("normal", "yes");
    let mut config: Value =
        serde_json::from_slice(&fs::read(t.path("config.json")).unwrap()).unwrap();
    config["enabled"] = json!(false);
    fs::write(t.path("config.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    t.ok(&["collect"]);
    fs::remove_file(t.path("config.json")).unwrap();
    t.ok(&["collect"]);
    assert!(t.calls().is_empty());
    config["enabled"] = json!(true);
    fs::write(t.path("config.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    let root = t.path("state/claude/wrapup/trial");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("state.json"),
        serde_json::to_vec(&json!({"checks":{},"finalized_commit":"a".repeat(40)})).unwrap(),
    )
    .unwrap();
    t.ok(&["collect"]);
    assert!(t.calls().is_empty());
}

#[test]
fn concurrent_collection_runs_one_model_and_keeps_shared_state_readable() {
    let t = Trial::new();
    t.ordinary("normal", "yes");
    fs::write(t.path("mode"), "block").unwrap();
    let mut first = t
        .cmd()
        .arg("collect")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !t.path("started").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !t.path("started").exists() {
        let _ = first.kill();
        panic!("fake model did not start");
    }
    let second = t.ok(&["collect"]);
    assert!(String::from_utf8_lossy(&second.stdout).contains("collector already running"));
    // Inference owns only the collector lock; read-only reporting remains available.
    t.ok(&["list"]);
    assert_eq!(t.calls().len(), 1);
    fs::write(t.path("release"), "continue").unwrap();
    assert!(first.wait().unwrap().success());
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 1);
}

impl Trial {
    fn prepare_report(&self) -> String {
        let report = self.path("report");
        fs::create_dir_all(&report).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&report)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        git(&["init", "-q"]);
        fs::write(report.join("script.md"), "Final saved report.\n").unwrap();
        git(&["add", "script.md"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "save report",
        ]);
        fs::write(self.path("codex/sessions/report.jsonl"), format!("{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"report","source":"cli"}}),
            json!({"type":"turn_context","payload":{"turn_id":"report-turn","collaboration_mode":{"mode":"default"}}})
        )).unwrap();
        git(&["rev-parse", "HEAD"]).trim().into()
    }
    fn finalize(&self, sha: &str) -> Output {
        self.cmd()
            .env("CODEX_THREAD_ID", "report")
            .args(["finalize", sha])
            .output()
            .unwrap()
    }
}

#[test]
fn finalization_requires_drained_backlog_and_no_collection_error() {
    let t = Trial::new();
    let sha = t.prepare_report();
    t.ordinary("unprocessed", "yes");
    let blocked = t.finalize(&sha);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("unread intervals"));
    assert!(!t.path("state/claude/wrapup/trial/state.json").exists());
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    let path = t.path("state/claude/wrapup/trial/state.json");
    let mut state = t.state();
    state["collector"]["last_error"] = json!("previous collection failed");
    fs::write(&path, state.to_string()).unwrap();
    let blocked = t.finalize(&sha);
    assert!(!blocked.status.success());
    assert_eq!(t.state()["finalized_commit"], "");
    state["collector"]["last_error"] = json!("");
    fs::write(&path, state.to_string()).unwrap();
    let finalized = t.finalize(&sha);
    assert!(
        finalized.status.success(),
        "{}",
        String::from_utf8_lossy(&finalized.stderr)
    );
    assert_eq!(t.state()["finalized_commit"], sha);
    let calls = t.calls().len();
    t.ordinary("late-arriving-log", "new input");
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), calls);
}

#[test]
fn finalization_rejects_an_inflight_collector_without_waiting() {
    let t = Trial::new();
    let sha = t.prepare_report();
    t.ordinary("normal", "yes");
    fs::write(t.path("mode"), "block").unwrap();
    let mut collector = t
        .cmd()
        .arg("collect")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !t.path("started").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !t.path("started").exists() {
        let _ = collector.kill();
        panic!("fake model did not start");
    }
    let begin = std::time::Instant::now();
    let blocked = t.finalize(&sha);
    fs::write(t.path("release"), "continue").unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("running"));
    assert!(begin.elapsed() < std::time::Duration::from_secs(3));
    assert!(collector.wait().unwrap().success());
    assert_eq!(t.state()["finalized_commit"], "");
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
}

#[test]
fn unfinished_in_period_turn_blocks_finalization_until_confirmed_interruption() {
    let t = Trial::new();
    let sha = t.prepare_report();
    t.log(
        "sessions/open.jsonl",
        "open",
        "2020-01-01T12:00:00Z",
        json!("cli"),
        None,
        "still working",
    );
    let dry = t.ok(&["collect", "--dry-run"]);
    assert!(serde_json::from_slice::<Vec<Value>>(&dry.stdout)
        .unwrap()
        .is_empty());
    let blocked = t.finalize(&sha);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("unread intervals"));
    use std::io::Write;
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(t.path("codex/sessions/open.jsonl"))
        .unwrap();
    writeln!(
        log,
        "{}",
        json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn1"}})
    )
    .unwrap();
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    let finalized = t.finalize(&sha);
    assert!(
        finalized.status.success(),
        "{}",
        String::from_utf8_lossy(&finalized.stderr)
    );
}

#[test]
fn fractional_timestamps_preserve_trial_boundaries() {
    let t = Trial::new();
    for (sid, at) in [
        ("before", "2019-12-31T23:59:59.999999Z"),
        ("start", "2020-01-01T00:00:00.000001Z"),
        ("last", "2020-01-01T23:59:59.999999Z"),
        ("end", "2020-01-02T00:00:00.000001Z"),
    ] {
        t.log(
            &format!("sessions/{sid}.jsonl"),
            sid,
            at,
            json!("cli"),
            Some("task_complete"),
            "yes",
        );
    }
    let result = t.ok(&["collect", "--dry-run"]);
    let mut sessions: Vec<String> = serde_json::from_slice::<Vec<Value>>(&result.stdout)
        .unwrap()
        .iter()
        .map(|v| v["session"].as_str().unwrap().into())
        .collect();
    sessions.sort();
    assert_eq!(sessions, ["last", "start"]);
}

#[test]
fn short_reply_keeps_previous_turn_context_without_reobserving_it() {
    let t = Trial::new();
    t.ordinary("conversation", "Can you resolve the failed build?");
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    use std::io::Write;
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(t.path("codex/sessions/conversation.jsonl"))
        .unwrap();
    for row in [
        json!({"type":"event_msg","timestamp":"2020-01-01T12:00:00.123Z","payload":{"type":"task_started","turn_id":"short-reply"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"yes"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"short-reply"}}),
    ] {
        writeln!(log, "{row}").unwrap();
    }
    let result = t.ok(&["collect", "--dry-run"]);
    let fragments: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(fragments.len(), 1);
    assert!(fragments[0]["text"].as_str().unwrap().contains("user: yes"));
    assert!(!fragments[0]["text"]
        .as_str()
        .unwrap()
        .contains("Here is the answer"));
    let context = fragments[0]["context"].as_str().unwrap();
    assert!(context.contains("Can you resolve the failed build?"));
    assert!(context.contains("assistant: Here is the answer."));
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 2);
    assert_eq!(t.state()["collector"]["done"].as_array().unwrap().len(), 2);
}

#[test]
fn same_turn_recovery_keeps_all_text_and_uses_fresh_fragment_ids_after_terminal() {
    use std::io::Write;
    let t = Trial::new();
    t.log(
        "sessions/recovered.jsonl",
        "recovered",
        "2020-01-01T00:00:00Z",
        json!("cli"),
        None,
        "before recovery",
    );
    let path = t.path("codex/sessions/recovered.jsonl");
    let append = |rows: &[Value]| {
        let mut log = fs::OpenOptions::new().append(true).open(&path).unwrap();
        for row in rows {
            writeln!(log, "{row}").unwrap();
        }
    };
    let start = json!({"type":"event_msg","timestamp":"2020-01-01T01:00:00Z","payload":{"type":"task_started","turn_id":"turn1"}});
    let message = |body: &str| json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":body}]}});
    let complete = json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn1"}});
    append(&[start.clone(), message("after same-turn restart")]);
    let still_open = t.ok(&["collect", "--dry-run"]);
    assert!(serde_json::from_slice::<Vec<Value>>(&still_open.stdout)
        .unwrap()
        .is_empty());
    append(std::slice::from_ref(&complete));
    let closed = t.ok(&["collect", "--dry-run"]);
    let fragments: Vec<Value> = serde_json::from_slice(&closed.stdout).unwrap();
    assert_eq!(fragments.len(), 1);
    let text = fragments[0]["text"].as_str().unwrap();
    assert!(text.contains("before recovery"));
    assert!(text.contains("after same-turn restart"));
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    append(&[start, message("resumed after terminal event"), complete]);
    let resumed = t.ok(&["collect", "--dry-run"]);
    let fragments: Vec<Value> = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0]["id"], "recovered:turn1:part1");
    assert!(fragments[0]["text"]
        .as_str()
        .unwrap()
        .contains("resumed after terminal event"));
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 2);
    assert_eq!(t.state()["collector"]["done"].as_array().unwrap().len(), 2);
}

#[test]
fn complete_json_without_record_terminator_is_not_consumed_until_newline_arrives() {
    use std::io::Write;
    let t = Trial::new();
    t.ordinary("live", "yes");
    let path = t.path("codex/sessions/live.jsonl");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.strip_suffix('\n').unwrap()).unwrap();
    let incomplete = t.ok(&["collect", "--dry-run"]);
    assert!(serde_json::from_slice::<Vec<Value>>(&incomplete.stdout)
        .unwrap()
        .is_empty());
    t.ok(&["collect"]);
    assert!(t.calls().is_empty());
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    t.ok(&["collect"]);
    assert_eq!(t.calls().len(), 1);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
}

#[test]
fn observations_may_cite_supplied_previous_turn_context_but_not_unrelated_lines() {
    use std::io::Write;
    let t = Trial::new();
    t.ordinary("conversation", "Please investigate this failure.");
    fs::write(t.path("mode"), "none").unwrap();
    t.ok(&["collect"]);
    let initial = t.state()["collector"]["done"].clone();
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(t.path("codex/sessions/conversation.jsonl"))
        .unwrap();
    for row in [
        json!({"type":"event_msg","timestamp":"2020-01-01T12:00:00Z","payload":{"type":"task_started","turn_id":"reply"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"yes"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"reply"}}),
    ] {
        writeln!(log, "{row}").unwrap();
    }
    let dry = t.ok(&["collect", "--dry-run"]);
    let segments: Vec<Value> = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0]["context_lines"], json!([3, 4]));
    assert!(segments[0]["first_line"].as_u64().unwrap() > 4);
    fs::write(t.path("mode"), "unrelated-line").unwrap();
    let unrelated = t.run(&["collect"]);
    assert!(!unrelated.status.success());
    assert!(String::from_utf8_lossy(&unrelated.stderr)
        .contains("invalid observation or source reference"));
    assert_eq!(t.state()["collector"]["done"], initial);
    assert!(t.state()["checks"].as_object().unwrap().is_empty());
    fs::write(t.path("mode"), "context").unwrap();
    t.ok(&["collect"]);
    let state = t.state();
    let checks = state["checks"].as_object().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(
        checks.values().next().unwrap()["observation"]["lines"],
        json!([4])
    );
    assert_eq!(state["collector"]["done"].as_array().unwrap().len(), 2);
    assert_eq!(state["collector"]["last_error"], "");
    assert_eq!(t.calls().len(), 3);
}

#[test]
fn malformed_complete_record_is_diagnosed_without_consuming_any_input() {
    let t = Trial::new();
    t.ordinary("a-valid", "valid input must remain pending");
    t.ordinary("b-malformed", "repairable input");
    let path = t.path("codex/sessions/b-malformed.jsonl");
    let original = fs::read_to_string(&path).unwrap();
    let mut records: Vec<&str> = original.split_inclusive('\n').collect();
    records.insert(2, "{not valid JSON}\n");
    fs::write(&path, records.concat()).unwrap();
    let failed = t.run(&["collect"]);
    assert!(!failed.status.success());
    let diagnostic = String::from_utf8_lossy(&failed.stderr);
    assert!(diagnostic.contains("invalid complete rollout record"));
    assert!(diagnostic.contains("b-malformed.jsonl:3"));
    let state = t.state();
    assert!(state["collector"]["done"].as_array().unwrap().is_empty());
    assert!(state["checks"].as_object().unwrap().is_empty());
    assert!(state["collector"]["last_error"]
        .as_str()
        .unwrap()
        .contains("b-malformed.jsonl:3"));
    assert!(t.calls().is_empty());
    fs::write(&path, original).unwrap();
    t.ok(&["collect"]);
    assert_eq!(t.state()["collector"]["done"].as_array().unwrap().len(), 2);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 2);
    assert_eq!(t.state()["collector"]["last_error"], "");
    assert_eq!(t.calls().len(), 1);
}

#[test]
fn conversational_collection_requires_default_while_plan_dry_run_stays_read_only() {
    let t = Trial::new();
    t.ordinary("source", "yes");
    let report_mode = |mode: &str| {
        fs::write(t.path("codex/sessions/report.jsonl"), format!("{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"report","source":"cli"}}),
            json!({"type":"turn_context","payload":{"turn_id":"report-turn","collaboration_mode":{"mode":mode}}})
        )).unwrap();
    };
    report_mode("plan");
    let blocked = t
        .cmd()
        .env("CODEX_THREAD_ID", "report")
        .arg("collect")
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("requires Default mode"));
    assert!(!t.path("state").exists());
    assert!(t.calls().is_empty());
    let dry = t
        .cmd()
        .env("CODEX_THREAD_ID", "report")
        .args(["collect", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Vec<Value>>(&dry.stdout)
            .unwrap()
            .len(),
        1
    );
    assert!(!t.path("state").exists());
    assert!(t.calls().is_empty());
    let unknown = t
        .cmd()
        .env("CODEX_THREAD_ID", "missing")
        .arg("collect")
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(!t.path("state").exists());
    report_mode("default");
    let collected = t
        .cmd()
        .env("CODEX_THREAD_ID", "report")
        .arg("collect")
        .output()
        .unwrap();
    assert!(
        collected.status.success(),
        "{}",
        String::from_utf8_lossy(&collected.stderr)
    );
    assert_eq!(t.calls().len(), 1);
    assert_eq!(t.state()["checks"].as_object().unwrap().len(), 1);
}
