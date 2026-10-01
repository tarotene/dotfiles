//! `--agent claude`(移植元 `config/claude/hooks/herdr-claude-metadata.sh`)。
//!
//! bash 版 `--selftest` の parse_payload ケース(full / no-permission_mode /
//! empty-permission_mode / subagent / no-cwd / all-optional-missing /
//! cwd-with-spaces)は、フィールド対応の崩れが socket に送る tokens に
//! そのまま現れるので、送信行の比較で検査する。

mod common;
use common::*;

fn tokens<'a>(
    plan: &'a str,
    default: &'a str,
    accept: &'a str,
    bypass: &'a str,
    branch: &'a str,
    oshi: &'a str,
) -> Vec<(&'static str, &'a str)> {
    vec![
        ("mode_plan", plan),
        ("mode_default", default),
        ("mode_accept", accept),
        ("mode_bypass", bypass),
        ("branch", branch),
        ("oshi", oshi),
    ]
}

fn line(t: &[(&str, &str)], ttl: bool) -> String {
    expected("claude-hook", t, ttl)
}

#[test]
fn full_payload_reports_mode_and_branch() {
    let h = Harness::new();
    let repo = h.git_repo("a", "feat/x");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionStart","permission_mode":"plan","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    let b = s("feat/x");
    assert_eq!(
        r.lines,
        vec![line(&tokens(PLAN, NULL, NULL, NULL, &b, NULL), true)]
    );
    assert_eq!(std::fs::read_to_string(h.mode_state()).unwrap(), "plan");
}

#[test]
fn missing_permission_mode_keeps_branch() {
    // #305 の回帰: mode が無くても cwd(=branch)が生き残る。
    let h = Harness::new();
    let repo = h.git_repo("a", "main");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionStart","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    let b = s("main");
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, NULL), true)]
    );
    assert_eq!(std::fs::read_to_string(h.mode_state()).unwrap(), "");
}

#[test]
fn empty_permission_mode_on_stop() {
    let h = Harness::new();
    let repo = h.git_repo("a", "main");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","permission_mode":"","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    let b = s("main");
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, NULL), true)]
    );
}

#[test]
fn subagent_is_ignored() {
    let h = Harness::new();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"PreToolUse","permission_mode":"default","agent_id":"x","cwd":"/tmp"}"#,
    );
    assert_quiet(&r);
    assert!(r.lines.is_empty());
    assert!(!h.mode_state().exists());
}

#[test]
fn session_end_clears_everything_without_ttl() {
    let h = Harness::new();
    std::fs::write(h.mode_state(), "plan").unwrap();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"SessionEnd","permission_mode":"default"}"#,
    );
    assert_quiet(&r);
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, NULL, NULL), false)]
    );
    assert!(!h.mode_state().exists());
}

#[test]
fn session_end_ignores_branch_even_with_cwd() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "worktree/feat");
    h.write_marks("suisei\t\u{2604}\u{fe0f}\n");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionEnd","permission_mode":"plan","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, NULL, NULL), false)]
    );
}

#[test]
fn all_optional_missing() {
    let h = Harness::new();
    let r = h.run("claude", &[], r#"{"hook_event_name":"SessionStart"}"#);
    assert_quiet(&r);
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, NULL, NULL), true)]
    );
}

#[test]
fn cwd_with_spaces() {
    let h = Harness::new();
    let repo = h.git_repo("a b", "topic");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","permission_mode":"","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    let b = s("topic");
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, NULL), true)]
    );
}

#[test]
fn each_known_mode_has_its_own_token() {
    for (mode, want) in [
        ("plan", tokens(PLAN, NULL, NULL, NULL, NULL, NULL)),
        ("default", tokens(NULL, DEFAULT, NULL, NULL, NULL, NULL)),
        ("acceptEdits", tokens(NULL, NULL, ACCEPT, NULL, NULL, NULL)),
        (
            "bypassPermissions",
            tokens(NULL, NULL, NULL, BYPASS, NULL, NULL),
        ),
    ] {
        let h = Harness::new();
        let r = h.run(
            "claude",
            &[],
            &format!(r#"{{"hook_event_name":"Stop","permission_mode":"{mode}"}}"#),
        );
        assert_quiet(&r);
        assert_eq!(r.lines, vec![line(&want, true)], "mode {mode}");
    }
}

#[test]
fn unknown_mode_goes_to_default_token_with_its_name() {
    let h = Harness::new();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"Stop","permission_mode":"dontAsk"}"#,
    );
    assert_quiet(&r);
    let d = r#""\u25c6 dontAsk""#;
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, d, NULL, NULL, NULL, NULL), true)]
    );
}

#[test]
fn non_ascii_mode_is_escaped_like_python() {
    let h = Harness::new();
    let r = h.run(
        "claude",
        &[],
        "{\"hook_event_name\":\"Stop\",\"permission_mode\":\"\u{e9}\\\"\\\\\u{7f}\\t\u{1f600}\"}",
    );
    assert_quiet(&r);
    let d = r#""\u25c6 \u00e9\"\\\u007f\t\ud83d\ude00""#;
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, d, NULL, NULL, NULL, NULL), true)]
    );
}

#[test]
fn newline_in_mode_becomes_space() {
    let h = Harness::new();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"Stop","permission_mode":"a\r\nb"}"#,
    );
    assert_quiet(&r);
    let d = r#""\u25c6 a  b""#;
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, d, NULL, NULL, NULL, NULL), true)]
    );
    assert_eq!(std::fs::read_to_string(h.mode_state()).unwrap(), "a  b");
}

#[test]
fn unhandled_event_is_ignored() {
    let h = Harness::new();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"PostToolUse","permission_mode":"plan"}"#,
    );
    assert_quiet(&r);
    assert!(r.lines.is_empty());
}

#[test]
fn invalid_or_empty_payload_is_ignored() {
    for input in ["{", "", "[1]", "\"x\""] {
        let h = Harness::new();
        let r = h.run("claude", &[], input);
        assert_quiet(&r);
        assert!(r.lines.is_empty(), "input {input:?}");
    }
}

#[test]
fn pre_tool_use_debounces_on_unchanged_mode() {
    let h = Harness::new();
    let pre =
        |mode: &str| format!(r#"{{"hook_event_name":"PreToolUse","permission_mode":"{mode}"}}"#);
    // 初回(状態ファイル無し)は送る
    let r = h.run("claude", &[], &pre("plan"));
    assert_quiet(&r);
    assert_eq!(
        r.lines,
        vec![line(&tokens(PLAN, NULL, NULL, NULL, NULL, NULL), true)]
    );
    // 同じモードは送らない
    let r = h.run("claude", &[], &pre("plan"));
    assert_quiet(&r);
    assert!(r.lines.is_empty());
    // UserPromptSubmit も同じ扱い
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"UserPromptSubmit","permission_mode":"plan"}"#,
    );
    assert!(r.lines.is_empty());
    // 変われば送り、状態を更新する
    let r = h.run("claude", &[], &pre("acceptEdits"));
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, ACCEPT, NULL, NULL, NULL), true)]
    );
    assert_eq!(
        std::fs::read_to_string(h.mode_state()).unwrap(),
        "acceptEdits"
    );
    // Stop は同じモードでも常に送る(ttl リフレッシュ)
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"Stop","permission_mode":"acceptEdits"}"#,
    );
    assert_eq!(r.lines.len(), 1);
}

#[test]
fn pre_tool_use_first_send_even_with_empty_mode() {
    let h = Harness::new();
    let r = h.run("claude", &[], r#"{"hook_event_name":"PreToolUse"}"#);
    assert_quiet(&r);
    assert_eq!(r.lines.len(), 1);
    // 2 回目は空 == 空で送らない
    let r = h.run("claude", &[], r#"{"hook_event_name":"PreToolUse"}"#);
    assert!(r.lines.is_empty());
}

#[test]
fn state_file_trailing_newline_is_ignored() {
    // `last="$(cat state)"` は末尾改行を落とす
    let h = Harness::new();
    std::fs::write(h.mode_state(), "plan\n\n").unwrap();
    let r = h.run(
        "claude",
        &[],
        r#"{"hook_event_name":"PreToolUse","permission_mode":"plan"}"#,
    );
    assert!(r.lines.is_empty());
}

#[test]
fn send_failure_does_not_write_state() {
    let h = Harness::new();
    let mut c = h.command("claude", &[]);
    c.env("HERDR_SOCKET_PATH", h.path("nonexistent.sock"));
    let r = h.run_cmd(
        c,
        r#"{"hook_event_name":"PreToolUse","permission_mode":"plan"}"#,
    );
    assert_quiet(&r);
    assert!(!h.mode_state().exists());
}

#[test]
fn outside_herdr_is_silent() {
    for (k, v) in [
        ("HERDR_ENV", "0"),
        ("HERDR_PANE_ID", ""),
        ("HERDR_SOCKET_PATH", ""),
    ] {
        let h = Harness::new();
        let mut c = h.command("claude", &[]);
        c.env(k, v);
        let r = h.run_cmd(c, r#"{"hook_event_name":"Stop","permission_mode":"plan"}"#);
        assert_quiet(&r);
        assert!(r.lines.is_empty(), "{k}={v:?}");
        assert!(!h.mode_state().exists());
    }
}

#[test]
fn pane_id_is_sanitized_in_state_file_name() {
    let h = Harness::new();
    let mut c = h.command("claude", &[]);
    c.env("HERDR_PANE_ID", "a/b c");
    let r = h.run_cmd(c, r#"{"hook_event_name":"Stop","permission_mode":"plan"}"#);
    assert_quiet(&r);
    assert_eq!(r.lines.len(), 1);
    assert!(r.lines[0].contains(r#""pane_id": "a/b c""#));
    assert_eq!(
        std::fs::read_to_string(h.path("run/herdr-claude-mode.a_b_c")).unwrap(),
        "plan"
    );
}

#[test]
fn worktree_prefix_is_stripped_and_oshi_is_looked_up() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-marine-ab12", "worktree/feat");
    h.write_marks(
        "# comment\nsuisei\t\u{2604}\u{fe0f}\nmarine\t\u{1f3f4}\u{200d}\u{2620}\u{fe0f}\textra\n",
    );
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"SessionStart","permission_mode":"default","cwd":"{}"}}"#,
            repo.display()
        ),
    );
    assert_quiet(&r);
    let b = s("feat");
    let o = r#""\ud83c\udff4\u200d\u2620\ufe0f""#;
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, DEFAULT, NULL, NULL, &b, o), true)]
    );
}

#[test]
fn oshi_from_subdirectory_uses_toplevel() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-suisei-ab12", "main");
    std::fs::create_dir_all(repo.join("sub/dir")).unwrap();
    h.write_marks("suisei\t\u{2604}\u{fe0f}\n");
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","cwd":"{}"}}"#,
            repo.join("sub/dir").display()
        ),
    );
    let b = s("main");
    let o = r#""\u2604\ufe0f""#;
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, o), true)]
    );
}

#[test]
fn oshi_miss_and_no_marks_file() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-pekora-ab12", "main");
    let input = format!(r#"{{"hook_event_name":"Stop","cwd":"{}"}}"#, repo.display());
    let b = s("main");
    let want = vec![line(&tokens(NULL, NULL, NULL, NULL, &b, NULL), true)];
    // marks ファイル無し
    assert_eq!(h.run("claude", &[], &input).lines, want);
    // 該当なし(コメント行の "#pekora" は拾わない)
    h.write_marks("#pekora\tX\nsuisei\tY\n");
    assert_eq!(h.run("claude", &[], &input).lines, want);
}

#[test]
fn non_worktree_dir_has_no_oshi() {
    let h = Harness::new();
    let repo = h.git_repo("dotfiles", "main");
    h.write_marks("dotfiles\tX\n");
    let r = h.run(
        "claude",
        &[],
        &format!(r#"{{"hook_event_name":"Stop","cwd":"{}"}}"#, repo.display()),
    );
    let b = s("main");
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, NULL), true)]
    );
}

#[test]
fn talent_with_dash_keeps_all_but_last_segment() {
    let h = Harness::new();
    let repo = h.git_repo("worktree-hakos-baelz-ab12", "main");
    h.write_marks("hakos-baelz\tZ\n");
    let r = h.run(
        "claude",
        &[],
        &format!(r#"{{"hook_event_name":"Stop","cwd":"{}"}}"#, repo.display()),
    );
    let b = s("main");
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, &b, r#""Z""#), true)]
    );
}

#[test]
fn non_git_cwd_has_no_branch() {
    let h = Harness::new();
    std::fs::create_dir_all(h.path("plain")).unwrap();
    let r = h.run(
        "claude",
        &[],
        &format!(
            r#"{{"hook_event_name":"Stop","cwd":"{}"}}"#,
            h.path("plain").display()
        ),
    );
    assert_quiet(&r);
    assert_eq!(
        r.lines,
        vec![line(&tokens(NULL, NULL, NULL, NULL, NULL, NULL), true)]
    );
}
