//! 実バイナリでの end-to-end。実際の検査(`git-stash-guard` など)が、ルータ経由で動く。

use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_hook-router");

/// 台帳・記録・設定は、テスト用の一時ディレクトリに向ける(本物の HOME を汚さない)。
fn run(args: &[&str], stdin: &str) -> (Output, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(BIN)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // 使い方の誤りで即終了するときは stdin を読まない。その BrokenPipe だけは正常。
    match child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => panic!("stdin: {e}"),
        _ => {}
    }
    (child.wait_with_output().unwrap(), home)
}

fn bash(event: &str, command: &str) -> String {
    serde_json::json!({
        "hook_event_name": event,
        "session_id": "s1",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": "/tmp",
    })
    .to_string()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout が JSON でない: {e}: {:?}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

#[test]
fn a_bare_git_stash_is_denied_through_the_router() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "git stash"));
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("git stash"), "{reason}");
}

#[test]
fn a_pkill_whose_pattern_matches_its_own_command_line_is_denied_through_the_router() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "pkill -f trial.sh"));
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("pkill -f '[t]rial.sh'"), "{reason}");
    // 括弧の技で書けば通る(無出力)。
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "pkill -f '[t]rial.sh'"));
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn a_harmless_command_produces_no_output() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "ls -la"));
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn worktree_add_is_denied_by_the_other_guard_in_the_same_run() {
    let (out, _h) = run(&["pre"], &bash("PreToolUse", "git worktree add ../x"));
    let v = json(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .contains("worktree"));
}

#[test]
fn two_guards_denying_at_once_both_reasons_survive() {
    // `git stash` は git-stash-guard、`git worktree add` は worktree-create-guard が拒否する。
    let (out, _h) = run(
        &["pre"],
        &bash("PreToolUse", "git stash && git worktree add ../x"),
    );
    let v = json(&out);
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(
        reason.contains("git stash") && reason.contains("worktree"),
        "理由がどちらも残る: {reason}"
    );
    assert!(reason.contains("\n\n"), "理由は空行で区切られる");
}

#[test]
fn a_tool_without_applicable_rules_is_a_silent_pass() {
    let raw = serde_json::json!({
        "hook_event_name": "PreToolUse", "session_id": "s", "tool_name": "WebFetch",
        "tool_input": {"url": "https://example.com"}, "cwd": "/tmp"
    })
    .to_string();
    let (out, _h) = run(&["pre"], &raw);
    assert!(out.status.success() && out.stdout.is_empty());
}

#[test]
fn unreadable_input_is_a_silent_pass() {
    for input in ["", "not json", "{}", r#"{"tool_name":""}"#] {
        let (out, _h) = run(&["pre"], input);
        assert!(out.status.success(), "{input:?}");
        assert!(out.stdout.is_empty(), "{input:?}");
    }
}

#[test]
fn post_event_runs_the_recorders_and_stays_silent_when_there_is_nothing_to_say() {
    let (out, _h) = run(&["post"], &bash("PostToolUse", "ls"));
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let (out, _h) = run(&["stop"], "{}");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: hook-router pre|post"));
    let (out, _h) = run(&[], "{}");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn codex_uses_its_own_attribution_and_all_shared_guards() {
    let (out, _h) = run(
        &["pre", "--agent", "codex"],
        &bash("PreToolUse", "gh pr comment 1 --body 'hello'"),
    );
    assert!(out.status.success());
    let reason = json(&out)["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(reason.contains("Codex CLI"), "{reason}");
}

#[test]
fn codex_patch_cannot_modify_the_main_checkout() {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(repo.path())
        .status()
        .unwrap()
        .success());
    let raw = serde_json::json!({
        "hook_event_name": "PreToolUse", "session_id": "patch-test",
        "tool_name": "apply_patch", "cwd": repo.path(),
        "tool_input": {"command": "*** Begin Patch\n*** Add File: file.txt\n+text\n*** End Patch"}
    })
    .to_string();
    let (out, _h) = run(&["pre", "--agent", "codex"], &raw);
    assert!(out.status.success());
    assert_eq!(
        json(&out)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert!(!repo.path().join("file.txt").exists());
}

#[test]
fn codex_patch_whitespace_headers_cannot_bypass_checkout_guard() {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(repo.path())
        .status()
        .unwrap()
        .success());
    for patch in [
        "*** Begin Patch\n  *** Add File: file.txt  \n+text\n*** End Patch",
        "*** Begin Patch\n\t*** Delete File: file.txt\t\n*** End Patch",
        "*** Begin Patch\n  *** Update File: file.txt  \n@@\n-old\n+new\n*** End Patch",
        "*** Begin Patch\n*** Update File: file.txt\n*** Move to: renamed.txt  \n@@\n-old\n+new\n*** End Patch",
    ] {
        let raw = serde_json::json!({
            "hook_event_name": "PreToolUse", "session_id": "patch-whitespace-test",
            "tool_name": "apply_patch", "cwd": repo.path(),
            "tool_input": {"command": patch}
        }).to_string();
        let (out, _h) = run(&["pre", "--agent", "codex"], &raw);
        assert!(out.status.success());
        assert_eq!(json(&out)["hookSpecificOutput"]["permissionDecision"], "deny", "{patch}");
    }
}

#[test]
fn codex_connected_slack_send_is_denied_but_draft_is_not() {
    for (tool, denied) in [
        ("mcp__codex_apps__slack_send_message", true),
        ("mcp__codex_apps__slack_draft_message", false),
    ] {
        let raw = serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":tool,
            "tool_input":{"text":"test"}, "cwd":"/tmp"})
        .to_string();
        let (out, _h) = run(&["pre", "--agent", "codex"], &raw);
        assert!(out.status.success());
        if denied {
            assert_eq!(
                json(&out)["hookSpecificOutput"]["permissionDecision"],
                "deny"
            );
        } else {
            assert!(out.stdout.is_empty());
        }
    }
}

#[test]
fn local_ask_becomes_a_codex_deny() {
    let command = "printf '%s' '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"run local check\"}}'";
    let (out, _h) = run(&["compat", command], &bash("PreToolUse", "echo test"));
    assert!(out.status.success());
    assert_eq!(
        json(&out)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(
        json(&out)["hookSpecificOutput"]["permissionDecisionReason"],
        "run local check"
    );
}

#[test]
fn empty_settings_deleted_worktrees_and_stale_ownership_do_not_gate_tools() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    let other = home.path().join("other/.claude");
    std::fs::create_dir_all(workspace.join(".claude")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(home.path().join(".config/agent-permissions")).unwrap();
    std::fs::create_dir_all(home.path().join(".local/state/dotfiles")).unwrap();
    std::fs::write(
        home.path().join(".config/agent-permissions/policy.json"),
        "invalid obsolete policy",
    )
    .unwrap();
    std::fs::write(
        home.path()
            .join(".local/state/dotfiles/agent-permissions.json"),
        "invalid obsolete ownership",
    )
    .unwrap();
    std::fs::write(workspace.join(".claude/settings.local.json"), "").unwrap();
    std::fs::write(other.join("settings.local.json"), "").unwrap();
    for delete in [false, true] {
        if delete {
            std::fs::remove_dir_all(home.path().join("other")).unwrap();
        }
        for args in [vec!["pre"], vec!["pre", "--agent", "codex"]] {
            for (tool, input) in [
                ("Read", serde_json::json!({"file_path":"/tmp/readme"})),
                ("Bash", serde_json::json!({"command":"git status"})),
                ("ToolSearch", serde_json::json!({"query":"lookup"})),
            ] {
                let raw = serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":tool,
                    "tool_input":input, "cwd":workspace, "session_id":"regression"});
                let mut child = Command::new(BIN)
                    .args(&args)
                    .env_clear()
                    .env("HOME", home.path())
                    .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                    .current_dir(&workspace)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(raw.to_string().as_bytes())
                    .unwrap();
                let out = child.wait_with_output().unwrap();
                assert!(out.status.success());
                assert!(
                    out.stdout.is_empty(),
                    "{args:?} {tool}: {}",
                    String::from_utf8_lossy(&out.stdout)
                );
            }
        }
    }
}

#[test]
fn removed_codex_permission_allow_entry_point_is_rejected() {
    let (out, _) = run(
        &["permission", "--agent", "codex"],
        &bash("PermissionRequest", "git status"),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

/// 宣言(`.config/record-layers.toml`)のあるリポジトリへの Write を、ルータ経由で裁く(#1310)。
/// 作業ツリーは本物の checkout なので `main-checkout-guard` も deny を返し、理由は連結される。
/// ここでは、記録の層の拒否が出るか(出ないか)だけを理由の文面で確かめる。
#[test]
fn a_write_to_an_undeclared_or_outside_git_path_is_denied_by_record_layers() {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(repo.path())
        .status()
        .unwrap()
        .success());
    std::fs::create_dir_all(repo.path().join(".config")).unwrap();
    std::fs::write(
        repo.path().join(".config/record-layers.toml"),
        "[[path]]\nglob = \"docs/**\"\nlayer = \"L1\"\n[[path]]\nglob = \"state/**\"\nlayer = \"L3\"\n",
    )
    .unwrap();
    let write = |rel: &str| {
        serde_json::json!({
            "hook_event_name": "PreToolUse", "session_id": "record-layers-test",
            "tool_name": "Write", "cwd": repo.path(),
            "tool_input": {"file_path": repo.path().join(rel), "content": "x"},
        })
        .to_string()
    };
    let reason = |rel: &str| {
        let (out, _h) = run(&["pre"], &write(rel));
        assert!(out.status.success());
        if out.stdout.is_empty() {
            return String::new();
        }
        json(&out)["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    assert!(reason("src/new.rs").contains("記録の層の宣言"), "未宣言");
    let l3 = reason("state/today.md");
    assert!(l3.contains("記録の層の宣言") && l3.contains("L3"), "{l3}");
    assert!(!reason("docs/a.md").contains("記録の層の宣言"), "L1 は通る");
}

#[test]
fn a_write_in_a_repository_without_a_declaration_is_not_judged_by_record_layers() {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(repo.path())
        .status()
        .unwrap()
        .success());
    let raw = serde_json::json!({
        "hook_event_name": "PreToolUse", "session_id": "record-layers-none",
        "tool_name": "Write", "cwd": repo.path(),
        "tool_input": {"file_path": repo.path().join("src/new.rs"), "content": "x"},
    })
    .to_string();
    let (out, _h) = run(&["pre"], &raw);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("記録の層の宣言"), "{text}");
}

// ---- reply-coverage(返信の下書きの関所と、会話の全文の保存)

mod reply_coverage_through_the_router {
    use super::*;
    use reply_coverage::store::{Record, Store};
    use reply_coverage::text;
    use serde_json::{json, Value};
    use std::path::Path;

    const CREATE: &str = "mcp__claude_ai_Gmail__create_draft";
    const GET_THREAD: &str = "mcp__claude_ai_Gmail__get_thread";
    const DRAFT: &str = "水曜日 15 時でお願いします。";

    fn run_in(home: &Path, env: &[(&str, &str)], args: &[&str], stdin: &str) -> Output {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CONFIG_HOME", home.join("config"));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn enable(home: &Path) {
        let dir = home.join("config/dotfiles");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("reply-coverage"), "enabled\n").unwrap();
    }

    fn store(home: &Path) -> Store {
        Store::at(home.join("state/claude/reply-coverage"))
    }

    fn post_get_thread() -> String {
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": GET_THREAD,
            "tool_input": {"threadId": "t1"},
            "tool_response": {"content": [{"type": "text", "text": json!({"messages":[
                {"id":"m1","sender":"a@example.invalid","date":"2026-11-01","labelIds":["INBOX"],
                 "plaintextBody":"来週の火曜日 10 時、または水曜日 15 時はいかがでしょう。"}]}).to_string()}]},
            "cwd": "/tmp"
        })
        .to_string()
    }

    fn pre_create(reply_to: Option<&str>, body: &str) -> String {
        let mut input = json!({"body": body, "to": ["a@example.invalid"]});
        if let Some(m) = reply_to {
            input["replyToMessageId"] = Value::String(m.to_string());
        }
        json!({"hook_event_name":"PreToolUse","session_id":"s1","tool_name":CREATE,"tool_input":input,"cwd":"/tmp"})
            .to_string()
    }

    fn denied(out: &Output) -> Option<String> {
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        (v["hookSpecificOutput"]["permissionDecision"] == "deny").then(|| {
            v["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .to_string()
        })
    }

    #[test]
    fn a_reply_is_denied_until_the_thread_was_read_then_until_checked_then_allowed() {
        let home = tempfile::tempdir().unwrap();
        enable(home.path());

        // 会話を読んでいない(get_thread を呼んでいない)返信の下書きは、拒否される。
        let out = run_in(home.path(), &[], &["pre"], &pre_create(Some("m1"), DRAFT));
        assert!(out.status.success());
        let reason = denied(&out).expect("deny");
        assert!(reason.contains("get_thread"), "{reason}");

        // get_thread の応答が保存される(PostToolUse。出力は無い)。
        let out = run_in(home.path(), &[], &["post"], &post_get_thread());
        assert!(out.status.success());
        let t = store(home.path()).load_thread("gmail:t1").expect("saved");
        assert!(t.text.contains("水曜日 15 時はいかがでしょう"));
        assert_eq!(t.message_ids, ["m1"]);

        // 会話は読んだが、検査していない → 拒否(次の一手が書かれている)。
        let out = run_in(home.path(), &[], &["pre"], &pre_create(Some("m1"), DRAFT));
        let reason = denied(&out).expect("deny");
        assert!(reason.contains("reply-coverage check"), "{reason}");

        // 検査に合格した最終稿 → 通る。
        store(home.path())
            .append_record(&Record {
                ts_unix: reply_coverage::store::now(),
                thread: t.id.clone(),
                thread_sha: text::sha(&t.text),
                draft_sha: text::sha(DRAFT),
                verdict: "pass".to_string(),
                model: "m".to_string(),
                asks: 1,
                unanswered: vec![],
                apology_missing: false,
            })
            .unwrap();
        let out = run_in(home.path(), &[], &["pre"], &pre_create(Some("m1"), DRAFT));
        assert!(
            denied(&out).is_none(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );

        // 検査のあとに 1 字直した本文は、拒否に戻る。
        let out = run_in(
            home.path(),
            &[],
            &["pre"],
            &pre_create(Some("m1"), "水曜日 15 時でお願いします!"),
        );
        assert!(denied(&out).is_some());
    }

    #[test]
    fn new_drafts_and_machines_without_the_marker_and_the_escape_hatch_are_left_alone() {
        let home = tempfile::tempdir().unwrap();
        // 目印が無い機体では、返信でも何もしない。保存もしない。
        let out = run_in(home.path(), &[], &["pre"], &pre_create(Some("m1"), DRAFT));
        assert!(denied(&out).is_none());
        run_in(home.path(), &[], &["post"], &post_get_thread());
        assert!(store(home.path()).load_thread("gmail:t1").is_none());

        enable(home.path());
        // 返信でない新規の下書きは通す。
        let out = run_in(home.path(), &[], &["pre"], &pre_create(None, DRAFT));
        assert!(denied(&out).is_none());
        // 逃げ道(セッションの環境変数)で、返信も通る。
        let out = run_in(
            home.path(),
            &[("SKIP_REPLY_COVERAGE_GATE", "1")],
            &["pre"],
            &pre_create(Some("m1"), DRAFT),
        );
        assert!(denied(&out).is_none());
        // 同じ呼び出しでも、逃げ道が無ければ拒否される(逃げ道が効いているのであって、素通りではない)。
        let out = run_in(home.path(), &[], &["pre"], &pre_create(Some("m1"), DRAFT));
        assert!(denied(&out).is_some());
    }
}

// ---- Antigravity CLI(agy)。入力は camelCase、出力は `decision`・`reason`。 ----

fn agy_command(event: &str, command: &str) -> String {
    serde_json::json!({
        "hookEventName": event,
        "conversationId": "c1",
        "cwd": "/tmp",
        "toolCall": {"id": "t1", "name": "run_command", "args": {"CommandLine": command}},
    })
    .to_string()
}

#[test]
fn antigravity_a_bare_git_stash_is_denied_in_its_own_shape() {
    let (out, _h) = run(
        &["pre", "--agent", "antigravity"],
        &agy_command("PreToolUse", "git stash"),
    );
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["decision"], "deny");
    assert!(v["reason"].as_str().is_some_and(|r| !r.is_empty()), "{v}");
    assert!(v.get("hookSpecificOutput").is_none(), "{v}");
}

#[test]
fn antigravity_uses_its_own_attribution() {
    let (out, _h) = run(
        &["pre", "--agent", "antigravity"],
        &agy_command("PreToolUse", "gh pr comment 1 --body 'hello'"),
    );
    let reason = json(&out)["reason"].as_str().unwrap().to_owned();
    assert!(reason.contains("Antigravity CLI"), "{reason}");
}

#[test]
fn antigravity_a_harmless_command_gets_no_opinion() {
    let (out, _h) = run(
        &["pre", "--agent", "antigravity"],
        &agy_command("PreToolUse", "ls"),
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
}

#[test]
fn antigravity_a_file_write_cannot_modify_the_main_checkout() {
    let repo = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(repo.path())
        .status()
        .unwrap()
        .success());
    let raw = serde_json::json!({
        "hookEventName": "PreToolUse", "conversationId": "c-write", "cwd": repo.path(),
        "toolCall": {"name": "write_file", "args": {
            "Path": repo.path().join("file.txt"), "Content": "text"}}
    })
    .to_string();
    let (out, _h) = run(&["pre", "--agent", "antigravity"], &raw);
    assert!(out.status.success());
    assert_eq!(json(&out)["decision"], "deny");
    assert!(!repo.path().join("file.txt").exists());
}

#[test]
fn antigravity_post_tool_use_answers_an_empty_object() {
    let (out, _h) = run(
        &["post", "--agent", "antigravity"],
        &agy_command("PostToolUse", "ls"),
    );
    assert!(out.status.success());
    assert_eq!(json(&out), serde_json::json!({}));
}

#[test]
fn antigravity_wrap_runs_a_claude_shaped_command() {
    let raw =
        r#"{"hookEventName":"Stop","conversationId":"c1","cwd":"/tmp","continuationActive":false}"#;
    let (out, _h) = run(
        &[
            "wrap",
            r#"cat >/dev/null; printf '%s' '{"decision":"block","reason":"未完"}'"#,
        ],
        raw,
    );
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["decision"], "continue");
    assert_eq!(v["reason"], "未完");
}

#[test]
fn antigravity_wrap_rejects_bad_usage() {
    let (out, _h) = run(&["wrap"], "{}");
    assert_eq!(out.status.code(), Some(1));
    let (out, _h) = run(&["wrap", "true", "--timeout", "x"], "{}");
    assert_eq!(out.status.code(), Some(1));
}
