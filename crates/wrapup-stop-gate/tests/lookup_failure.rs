//! A failed lookup retains work without repeatedly stopping the same session.
mod common;
use common::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const LINE: &str = r#"{"ts":"now","title":"dup title","detail":"original"}"#;

fn setup() -> (Env, PathBuf, String) {
    let e = Env::new();
    let project = repo(
        &e.path().join("repo"),
        Some("https://github.com/example/failures.git"),
    );
    let inbox = run_args(e.gate(), &["--inbox-path", project.to_str().unwrap()]).stdout;
    assert_eq!(run_args(e.gate(), &["--add", &inbox, LINE]).code, 0);
    (e, project, inbox)
}

fn check(e: &Env, inbox: &str, session: &str, fail: bool) -> Run {
    let mut command = e.gate();
    command.env("WRAPUP_STUB_FAIL", if fail { "1" } else { "0" });
    run_args(
        command,
        &[
            "--check-dup",
            "dup title",
            "--inbox",
            inbox,
            "--session-id",
            session,
        ],
    )
}

fn stop(e: &Env, project: &Path, session: &str) -> Run {
    run(
        e.gate(),
        &serde_json::json!({"cwd": project, "session_id": session}).to_string(),
    )
}

#[test]
fn failed_rows_are_retained_and_only_unchanged_rows_in_that_session_are_deferred() {
    let (e, project, inbox) = setup();
    assert_eq!(stop(&e, &project, "a/b").code, 2);
    assert_eq!(check(&e, &inbox, "a/b", true).code, 3);
    assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n"));
    assert_eq!(stop(&e, &project, "a/b").code, 0);
    assert_eq!(stop(&e, &project, "other").code, 2);
    // SessionLedger filename sanitization must not alias distinct sessions.
    assert_eq!(stop(&e, &project, "a_b").code, 2);

    let changed = LINE.replace("original", "changed");
    std::fs::write(&inbox, format!("{changed}\n")).unwrap();
    assert_eq!(stop(&e, &project, "a/b").code, 2);
    std::fs::write(&inbox, format!("{LINE}\n{changed}\n")).unwrap();
    let output = stop(&e, &project, "a/b");
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("1 unfiled item(s)"));
    assert!(output.stderr.contains("--session-id a/b"));
    let procedure = run_args(e.gate(), &["--procedure", &inbox, "--session-id", "a/b"]);
    assert!(procedure.stdout.contains(&changed));
    assert!(!procedure.stdout.contains(LINE));
    assert!(procedure.stdout.contains("--inbox"));
}

#[test]
fn explicit_retry_always_runs_and_success_or_duplicate_clears_deferral() {
    let (e, project, inbox) = setup();
    for duplicate in [false, true] {
        assert_eq!(check(&e, &inbox, "retry", true).code, 3);
        let before = read(&e.gh_log()).lines().count();
        assert_eq!(check(&e, &inbox, "retry", true).code, 3);
        assert_eq!(read(&e.gh_log()).lines().count(), before + 1);
        let mut command = e.gate();
        command.env("WRAPUP_STUB_DUP", if duplicate { "1" } else { "0" });
        let result = run_args(
            command,
            &[
                "--check-dup",
                "dup title",
                "--inbox",
                &inbox,
                "--session-id",
                "retry",
            ],
        );
        assert_eq!(result.code, i32::from(duplicate));
        assert_eq!(stop(&e, &project, "retry").code, 2);
        assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n"));
    }
}

#[test]
fn failure_snapshot_excludes_rows_added_during_the_lookup() {
    let (e, project, inbox) = setup();
    let newer = LINE.replace("original", "arrived-during-lookup");
    write_exec(
        &e.bin_dir().join("gh"),
        "#!/bin/sh\nprintf '%s\\n' \"$NEW_LINE\" >> \"$INBOX\"\nexit 1\n",
    );
    let mut command = e.gate();
    command.env("NEW_LINE", &newer).env("INBOX", &inbox);
    let result = run_args(
        command,
        &[
            "--check-dup",
            "dup title",
            "--inbox",
            &inbox,
            "--session-id",
            "snapshot",
        ],
    );
    assert_eq!(result.code, 3);
    let result = stop(&e, &project, "snapshot");
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("1 unfiled item(s)"));
    assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n{newer}\n"));
}

#[test]
fn no_context_or_unwritable_ledger_cannot_suppress_stop() {
    let (e, project, inbox) = setup();
    let mut command = e.gate();
    command.env("WRAPUP_STUB_FAIL", "1");
    assert_eq!(run_args(command, &["--check-dup", "dup title"]).code, 3);
    assert_eq!(stop(&e, &project, "no-context").code, 2);
    std::fs::write(
        e.state().join("claude/wrapup/lookup-failures"),
        "not a directory",
    )
    .unwrap();
    let failure = check(&e, &inbox, "unwritable", true);
    assert_eq!(failure.code, 3);
    assert!(failure
        .stderr
        .contains("cannot update session failure ledger"));
    assert_eq!(stop(&e, &project, "unwritable").code, 2);
    assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n"));
}

#[test]
fn concurrent_failed_lookups_keep_every_snapshot() {
    let (e, project, inbox) = setup();
    let rows: Vec<_> = (0..12)
        .map(|n| serde_json::json!({"title":format!("t{n}"), "detail":n}).to_string())
        .collect();
    std::fs::write(&inbox, format!("{}\n", rows.join("\n"))).unwrap();
    let mut children = Vec::new();
    for n in 0..12 {
        let mut command = e.gate();
        command
            .env("WRAPUP_STUB_FAIL", "1")
            .args([
                "--check-dup",
                &format!("t{n}"),
                "--inbox",
                &inbox,
                "--session-id",
                "parallel",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        children.push(command.spawn().unwrap());
    }
    for mut child in children {
        assert_eq!(child.wait().unwrap().code(), Some(3));
    }
    assert_eq!(stop(&e, &project, "parallel").code, 0);
    assert_eq!(read(Path::new(&inbox)), format!("{}\n", rows.join("\n")));
}

#[test]
fn timeout_is_bounded_retains_inbox_and_records_failure() {
    let (e, project, inbox) = setup();
    write_exec(&e.bin_dir().join("gh"), "#!/bin/sh\nexec sleep 30\n");
    let started = Instant::now();
    let result = check(&e, &inbox, "timeout", false);
    assert_eq!(result.code, 3);
    assert!(result.stderr.contains("timed out after 10 seconds"));
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(stop(&e, &project, "timeout").code, 0);
    assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n"));
}

#[test]
fn deferred_inbox_does_not_suppress_feedback_check() {
    let (e, project, inbox) = setup();
    assert_eq!(check(&e, &inbox, "feedback", true).code, 3);
    assert_eq!(
        run_args(e.gate(), &["--stamp-feedback-session", "feedback"]).code,
        0
    );
    let stamp = e.stamp_dir().join("feedback.stamp");
    let old = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(stamp)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let memory = e.memory_dir().join("p/memory/policy.md");
    std::fs::create_dir_all(memory.parent().unwrap()).unwrap();
    std::fs::write(memory, "---\nmetadata:\n  type: feedback\n---\npolicy\n").unwrap();
    let output = stop(&e, &project, "feedback");
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("feedback-memory"));
    assert!(!output.stderr.contains("unfiled item(s)"));
}

#[test]
fn authentication_and_network_failure_are_unknown_and_preserve_diagnostics() {
    for diagnostic in [
        "HTTP 401 Requires authentication",
        "Could not resolve host api.github.com",
    ] {
        let (e, project, inbox) = setup();
        write_exec(
            &e.bin_dir().join("gh"),
            &format!("#!/bin/sh\necho '{diagnostic}' >&2\nexit 1\n"),
        );
        let result = check(&e, &inbox, "network", false);
        assert_eq!(result.code, 3);
        assert!(result.stderr.contains(diagnostic));
        assert_eq!(read(Path::new(&inbox)), format!("{LINE}\n"));
        assert_eq!(stop(&e, &project, "network").code, 0);
    }
}

#[test]
fn repository_context_and_inbox_path_isolate_failure_snapshots() {
    let (e, project, inbox) = setup();
    let routed = r#"{"title":"dup title","repo":"example/elsewhere","detail":"routed"}"#;
    assert_eq!(run_args(e.gate(), &["--add", &inbox, routed]).code, 0);
    let mut command = e.gate();
    command.env("WRAPUP_STUB_FAIL", "1");
    assert_eq!(
        run_args(
            command,
            &[
                "--check-dup",
                "dup title",
                "example/elsewhere",
                "--inbox",
                &inbox,
                "--session-id",
                "routing"
            ]
        )
        .code,
        3
    );
    assert_eq!(stop(&e, &project, "routing").code, 2);
    assert_eq!(check(&e, &inbox, "routing", true).code, 3);
    assert_eq!(stop(&e, &project, "routing").code, 0);

    let other = repo(
        &e.path().join("another"),
        Some("https://github.com/example/another.git"),
    );
    let other_inbox = run_args(e.gate(), &["--inbox-path", other.to_str().unwrap()]).stdout;
    assert_eq!(run_args(e.gate(), &["--add", &other_inbox, LINE]).code, 0);
    assert_eq!(stop(&e, &other, "routing").code, 2);

    let ledger = e
        .state()
        .join("claude/wrapup/lookup-failures/routing.jsonl");
    std::fs::write(ledger, "corrupt state\n").unwrap();
    let result = stop(&e, &project, "routing");
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("cannot read session failure ledger"));
}
