//! `--agent copilot`(移植元 `config/copilot/hooks/herdr-copilot-metadata.sh`)。
//!
//! Copilot の hook input は sessionId/timestamp/cwd だけなので、action は
//! argv(`report` / `clear`)、model は `$HOME/.copilot/settings.json` から読む。
//! bash 版 `--selftest` の各ケース(valid_action / payload_cwd /
//! settings_model / oshi_for_toplevel)は送信行の比較で検査する。

mod common;
use common::*;

fn line(model: &str, branch: &str, oshi: &str, ttl: bool) -> String {
    expected(
        "copilot-hook",
        &[("model", model), ("branch", branch), ("oshi", oshi)],
        ttl,
    )
}

fn settings(h: &Harness, body: &str) {
    let d = h.path("home/.copilot");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("settings.json"), body).unwrap();
}

fn payload(cwd: &std::path::Path) -> String {
    format!(
        r#"{{"sessionId":"s","timestamp":1,"cwd":"{}"}}"#,
        cwd.display()
    )
}

#[test]
fn report_full() {
    let h = Harness::new();
    let repo = h.git_repo("a", "worktree/x");
    settings(&h, r#"{"model":"gpt-5"}"#);
    let r = h.run("copilot", &["report"], &payload(&repo));
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(&s("gpt-5"), &s("x"), NULL, true)]);
}

#[test]
fn clear_sends_nulls_without_ttl() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "main");
    settings(&h, r#"{"model":"gpt-5"}"#);
    h.write_marks("suisei\tX\n");
    let r = h.run("copilot", &["clear"], &payload(&repo));
    assert_quiet(&r);
    assert_eq!(r.lines, vec![line(NULL, NULL, NULL, false)]);
}

#[test]
fn invalid_actions_are_ignored() {
    for args in [vec![], vec![""], vec!["session"], vec!["Report"]] {
        let h = Harness::new();
        settings(&h, r#"{"model":"gpt-5"}"#);
        let r = h.run("copilot", &args, r#"{"cwd":"/tmp"}"#);
        assert_quiet(&r);
        assert!(r.lines.is_empty(), "args {args:?}");
    }
}

#[test]
fn payload_without_cwd_still_reports_model() {
    for input in [r#"{"sessionId":"s","timestamp":1}"#, "", "{"] {
        let h = Harness::new();
        settings(&h, r#"{"model":"gpt-5"}"#);
        let r = h.run("copilot", &["report"], input);
        assert_quiet(&r);
        assert_eq!(
            r.lines,
            vec![line(&s("gpt-5"), NULL, NULL, true)],
            "input {input:?}"
        );
    }
}

#[test]
fn settings_model_variants() {
    for (body, want) in [
        (Some(r#"{"theme":"dark"}"#), NULL.to_string()),
        (Some("{"), NULL.to_string()),
        (Some(r#"{"model":""}"#), NULL.to_string()),
        (
            Some(r#"{"model":"claude-sonnet-4.5"}"#),
            s("claude-sonnet-4.5"),
        ),
        (None, NULL.to_string()),
    ] {
        let h = Harness::new();
        if let Some(b) = body {
            settings(&h, b);
        }
        let r = h.run("copilot", &["report"], "{}");
        assert_quiet(&r);
        assert_eq!(
            r.lines,
            vec![line(&want, NULL, NULL, true)],
            "settings {body:?}"
        );
    }
}

#[test]
fn oshi_hit() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "main");
    h.write_marks(
        "# comment\nsuisei\t\u{2604}\u{fe0f}\nmarine\t\u{1f3f4}\u{200d}\u{2620}\u{fe0f}\n",
    );
    let r = h.run("copilot", &["report"], &payload(&repo));
    assert_eq!(
        r.lines,
        vec![line(NULL, &s("main"), r#""\u2604\ufe0f""#, true)]
    );
}

#[test]
fn oshi_miss_not_worktree_comment_and_no_file() {
    let h = Harness::new();
    let miss = h.git_repo("worktree-pekora-ab12", "main");
    let plain = h.git_repo("dotfiles", "main");
    let hash = h.git_repo("worktree-#-ab12", "main");
    let want = vec![line(NULL, &s("main"), NULL, true)];
    // marks ファイル無し
    assert_eq!(h.run("copilot", &["report"], &payload(&miss)).lines, want);
    h.write_marks("# comment\nsuisei\tX\n#\tY\ndotfiles\tZ\n");
    assert_eq!(h.run("copilot", &["report"], &payload(&miss)).lines, want);
    assert_eq!(h.run("copilot", &["report"], &payload(&plain)).lines, want);
    assert_eq!(h.run("copilot", &["report"], &payload(&hash)).lines, want);
}

#[test]
fn outside_herdr_is_silent() {
    let h = Harness::new();
    let mut c = h.command("copilot", &["report"]);
    c.env("HERDR_ENV", "");
    let r = h.run_cmd(c, "{}");
    assert_quiet(&r);
    assert!(r.lines.is_empty());
}

#[test]
fn send_failure_is_silent() {
    let h = Harness::new();
    let mut c = h.command("copilot", &["report"]);
    c.env("HERDR_SOCKET_PATH", h.path("none.sock"));
    let r = h.run_cmd(c, "{}");
    assert_quiet(&r);
}
