//! `--agent codex`(移植元 `config/codex/hooks/herdr-codex-metadata.sh`)。
//!
//! bash 版 `--selftest` の parse_payload ケース(full / no-model / empty-model /
//! subagent / no-cwd / all-optional-missing)を送信行の比較で検査する。

mod common;
use common::*;

fn line(model: &str, branch: &str, oshi: &str, ttl: bool) -> String {
    expected(
        "codex-hook",
        &[("model", model), ("branch", branch), ("oshi", oshi)],
        ttl,
    )
}

#[test]
fn full_payload() {
    let h = Harness::new();
    let repo = h.git_repo("a", "worktree/x");
    let r = h.run(
        "codex",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionStart","model":"gpt-5","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(&s("gpt-5"), &s("x"), NULL, true)]);
}

#[test]
fn no_model_keeps_branch() {
    let h = Harness::new();
    let repo = h.git_repo("a", "main");
    let r = h.run(
        "codex",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionStart","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(NULL, &s("main"), NULL, true)]);
}

#[test]
fn empty_model() {
    let h = Harness::new();
    let repo = h.git_repo("a", "main");
    let r = h.run(
        "codex",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","model":"","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_eq!(r.lines, vec![line(NULL, &s("main"), NULL, true)]);
}

#[test]
fn subagent_is_ignored() {
    let h = Harness::new();
    let r = h.run(
        "codex",
        &[],
        r#"{"hook_event_name":"Stop","model":"gpt-5","agent_id":"x","cwd":"/tmp/a"}"#,
    );
    assert_quiet(&r);
    assert!(r.lines.is_empty());
}

#[test]
fn session_end_still_carries_payload_model() {
    // bash 版の挙動: SessionEnd では branch/oshi は取らないが、payload の
    // model はそのまま送る(コメントは「model が無い」前提)。
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "main");
    h.write_marks("suisei\tX\n");
    let r = h.run(
        "codex",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionEnd","model":"gpt-5","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(&s("gpt-5"), NULL, NULL, false)]);
}

#[test]
fn session_end_without_model() {
    let h = Harness::new();
    let r = h.run("codex", &[], r#"{"hook_event_name":"SessionEnd"}"#);
    assert_eq!(r.lines, vec![line(NULL, NULL, NULL, false)]);
}

#[test]
fn all_optional_missing() {
    let h = Harness::new();
    let r = h.run("codex", &[], r#"{"hook_event_name":"SessionStart"}"#);
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(NULL, NULL, NULL, true)]);
}

#[test]
fn user_prompt_submit_is_reported_without_debounce() {
    let h = Harness::new();
    let input = r#"{"hook_event_name":"UserPromptSubmit","model":"o3"}"#;
    assert_eq!(
        h.run("codex", &[], input).lines,
        vec![line(&s("o3"), NULL, NULL, true)]
    );
    assert_eq!(
        h.run("codex", &[], input).lines,
        vec![line(&s("o3"), NULL, NULL, true)]
    );
}

#[test]
fn unhandled_events_are_ignored() {
    for ev in ["PreToolUse", "PostToolUse", ""] {
        let h = Harness::new();
        let r = h.run(
            "codex",
            &[],
            &format!(r#"{{"hook_event_name":"{ev}","model":"gpt-5"}}"#),
        );
        assert_quiet(&r);
        assert!(r.lines.is_empty(), "event {ev:?}");
    }
}

#[test]
fn invalid_payload_is_ignored() {
    for input in ["{", ""] {
        let h = Harness::new();
        let r = h.run("codex", &[], input);
        assert_quiet(&r);
        assert!(r.lines.is_empty());
    }
}

#[test]
fn oshi_lookup() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "worktree/feat");
    h.write_marks("# c\nsuisei\t\u{2604}\u{fe0f}\n");
    let r = h.run(
        "codex",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","model":"gpt-5","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_eq!(
        r.lines,
        vec![line(&s("gpt-5"), &s("feat"), r#""\u2604\ufe0f""#, true)]
    );
}

#[test]
fn outside_herdr_is_silent() {
    let h = Harness::new();
    let mut c = h.command("codex", &[]);
    c.env_remove("HERDR_ENV");
    let r = h.run_cmd(c, r#"{"hook_event_name":"Stop","model":"gpt-5"}"#);
    assert_quiet(&r);
    assert!(r.lines.is_empty());
}

#[test]
fn send_failure_is_silent() {
    let h = Harness::new();
    let mut c = h.command("codex", &[]);
    c.env("HERDR_SOCKET_PATH", h.path("none.sock"));
    let r = h.run_cmd(c, r#"{"hook_event_name":"Stop","model":"gpt-5"}"#);
    assert_quiet(&r);
}
