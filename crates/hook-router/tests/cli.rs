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
fn deployed_permissions_cannot_be_bypassed_by_unknown_or_unparseable_input() {
    let home = tempfile::tempdir().unwrap();
    let policy = home.path().join(".config/agent-permissions");
    std::fs::create_dir_all(&policy).unwrap();
    // An unreadable policy must reject every tool, including inputs the Codex
    // apply_patch adapter cannot expand. No real account state is involved.
    std::fs::write(policy.join("policy.json"), "invalid policy").unwrap();
    for args in [
        vec!["pre"],
        vec!["pre", "--agent", "codex"],
        vec!["permission", "--agent", "codex"],
    ] {
        for raw in [
            "",
            "not json",
            "{}",
            r#"{"tool_name":"future_tool"}"#,
            r#"{"tool_name":"apply_patch","tool_input":{}}"#,
        ] {
            let mut child = Command::new(BIN)
                .args(&args)
                .env_clear()
                .env("HOME", home.path())
                .env("XDG_STATE_HOME", home.path().join("state"))
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(raw.as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success());
            let value = json(&out);
            let output = &value["hookSpecificOutput"];
            if args[0] == "permission" {
                assert_eq!(output["decision"]["behavior"], "deny", "{args:?} {raw}");
            } else {
                assert_eq!(output["permissionDecision"], "deny", "{args:?} {raw}");
            }
        }
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
fn codex_permission_does_not_allow_an_arbitrary_command() {
    let (out, _h) = run(
        &["permission", "--agent", "codex"],
        &bash("PermissionRequest", "echo test"),
    );
    assert!(out.status.success() && out.stdout.is_empty());
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
fn codex_permission_allows_only_a_known_worktree_command() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".herdr/worktrees");
    let wt = root.join("repo/unit");
    std::fs::create_dir_all(&wt).unwrap();
    for (command, allowed) in [
        (format!("git -C {} status", wt.display()), true),
        (format!("git -C {} status; echo extra", wt.display()), false),
    ] {
        let mut child = Command::new(BIN)
            .args(["permission", "--agent", "codex"])
            .env_clear()
            .env("HOME", home.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(bash("PermissionRequest", &command).as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        if allowed {
            assert_eq!(
                json(&out)["hookSpecificOutput"]["decision"]["behavior"],
                "allow"
            );
        } else {
            assert!(out.stdout.is_empty());
        }
    }
}

#[test]
fn codex_records_created_issue_and_allows_only_its_edit() {
    let home = tempfile::tempdir().unwrap();
    let invoke = |mode: &str, raw: &str| {
        let mut child = Command::new(BIN)
            .args([mode, "--agent", "codex"])
            .env_clear()
            .env("HOME", home.path())
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(raw.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let mut created: serde_json::Value =
        serde_json::from_str(&bash("PostToolUse", "gh issue create -R example/repo")).unwrap();
    created["tool_response"] = serde_json::json!("https://github.com/example/repo/issues/42\n");
    assert!(invoke("post", &created.to_string()).status.success());
    for (number, allowed) in [(42, true), (43, false)] {
        let out = invoke(
            "permission",
            &bash(
                "PermissionRequest",
                &format!("gh issue edit {number} -R example/repo --body-file /tmp/body.md"),
            ),
        );
        assert!(out.status.success());
        if allowed {
            assert_eq!(
                json(&out)["hookSpecificOutput"]["decision"]["behavior"],
                "allow"
            );
        } else {
            assert!(out.stdout.is_empty());
        }
    }
}

fn permission_home() -> (tempfile::TempDir, std::path::PathBuf) {
    use agent_settings_reconcile::agent_permissions::{inspect, migrate, Policy};
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let config = home.path().join(".config/agent-permissions");
    std::fs::create_dir_all(&config).unwrap();
    let value = serde_json::json!({"version":1,"writable_roots":["workspace","temp"],
        "secret_paths":["~/.ssh"],"domains":["github.com"],"tools":"prompt",
        "projects":[],"project_rules":{}});
    std::fs::write(config.join("policy.json"), value.to_string()).unwrap();
    let policy: Policy = serde_json::from_value(value).unwrap();
    let review = inspect(&policy, home.path(), &[]).unwrap();
    migrate(&policy, home.path(), &[], &review).unwrap();
    (home, workspace)
}

fn permission_call(
    home: &std::path::Path,
    workspace: &std::path::Path,
    tool: &str,
    args: serde_json::Value,
) -> Output {
    let raw = serde_json::json!({"hook_event_name":"PreToolUse","session_id":"permission-test",
        "tool_name":tool,"tool_input":args,"cwd":workspace});
    let mut child = Command::new(BIN)
        .arg("pre")
        .env_clear()
        .env("HOME", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("XDG_STATE_HOME", home.join("state"))
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
    child.wait_with_output().unwrap()
}

#[test]
fn migrated_policy_asks_once_for_outside_writes_and_denies_raw_secrets() {
    let (home, workspace) = permission_home();
    for (tool, path, want) in [
        ("Write", "/var/lib/agent-permissions-uncreated-file", "ask"),
        ("Read", "/var/lib/agent-permissions-uncreated-file", "pass"),
        (
            "Read",
            home.path().join(".ssh/nonexistent").to_str().unwrap(),
            "deny",
        ),
        ("Write", workspace.join("new.txt").to_str().unwrap(), "pass"),
    ] {
        let out = permission_call(
            home.path(),
            &workspace,
            tool,
            serde_json::json!({"file_path":path}),
        );
        assert!(out.status.success());
        if want == "pass" {
            assert!(
                out.stdout.is_empty(),
                "{tool}: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        } else {
            assert_eq!(
                json(&out)["hookSpecificOutput"]["permissionDecision"],
                want,
                "{tool}: {path}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn dangling_symlink_to_outside_workspace_requires_one_time_approval() {
    let (home, workspace) = permission_home();
    let alias = workspace.join("alias");
    std::os::unix::fs::symlink("/var/lib/agent-permissions-uncreated-target", &alias).unwrap();
    let out = permission_call(
        home.path(),
        &workspace,
        "Write",
        serde_json::json!({"file_path":alias}),
    );
    assert!(out.status.success());
    assert_eq!(
        json(&out)["hookSpecificOutput"]["permissionDecision"],
        "ask"
    );
}

#[test]
fn migrated_policy_asks_for_mcp_but_an_existing_guard_deny_still_wins() {
    let (home, workspace) = permission_home();
    for (tool, want) in [
        ("mcp__codex_apps__slack_draft_message", "ask"),
        ("mcp__codex_apps__slack_send_message", "deny"),
    ] {
        let out = permission_call(
            home.path(),
            &workspace,
            tool,
            serde_json::json!({"text":"test"}),
        );
        assert!(out.status.success());
        assert_eq!(
            json(&out)["hookSpecificOutput"]["permissionDecision"],
            want,
            "{tool}"
        );
    }
}

#[test]
fn removing_a_deployed_policy_does_not_disable_the_guard() {
    let (home, workspace) = permission_home();
    std::fs::remove_file(home.path().join(".config/agent-permissions/policy.json")).unwrap();
    let out = permission_call(
        home.path(),
        &workspace,
        "Read",
        serde_json::json!({"file_path":"plain.txt"}),
    );
    assert!(out.status.success());
    assert_eq!(
        json(&out)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}

/// #1060: `hms` deploys an expanded policy.json before anyone migrates. The hook
/// enforces the reviewed record, so the expansion neither takes effect nor stops
/// every tool.
#[test]
fn an_expansion_deployed_before_migrate_does_not_stop_tools_or_take_effect() {
    let (home, workspace) = permission_home();
    let deployed = home.path().join(".config/agent-permissions/policy.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&deployed).unwrap()).unwrap();
    value["domains"] = serde_json::json!(["github.com", "example.org"]);
    value["sandboxed_shell"] = serde_json::json!("auto");
    std::fs::write(&deployed, value.to_string()).unwrap();
    let decision = |tool: &str, args: serde_json::Value| {
        let out = permission_call(home.path(), &workspace, tool, args);
        assert!(out.status.success());
        if out.stdout.is_empty() {
            "pass".to_owned()
        } else {
            json(&out)["hookSpecificOutput"]["permissionDecision"]
                .as_str()
                .unwrap_or("other")
                .to_owned()
        }
    };
    // Before the fix every one of these was denied: "policy changed".
    assert_eq!(
        decision("Read", serde_json::json!({"file_path":"plain.txt"})),
        "pass"
    );
    assert_eq!(
        decision("Bash", serde_json::json!({"command":"ls"})),
        "pass"
    );
    assert_eq!(
        decision(
            "WebFetch",
            serde_json::json!({"url":"https://github.com/x"})
        ),
        "pass"
    );
    // The unreviewed destination is not effective until migrate.
    assert_eq!(
        decision(
            "WebFetch",
            serde_json::json!({"url":"https://example.org/x"})
        ),
        "ask"
    );
}

#[test]
fn pending_allow_rules_do_not_stop_the_session() {
    let (home, workspace) = permission_home();
    std::fs::create_dir_all(workspace.join(".claude")).unwrap();
    std::fs::write(
        workspace.join(".claude/settings.local.json"),
        r#"{"permissions":{"allow":["WebFetch(domain:example.com)","Bash(git status:*)"]}}"#,
    )
    .unwrap();
    for (tool, args) in [
        ("Read", serde_json::json!({"file_path":"plain.txt"})),
        ("Bash", serde_json::json!({"command":"ls"})),
        ("AskUserQuestion", serde_json::json!({})),
    ] {
        let out = permission_call(home.path(), &workspace, tool, args);
        assert!(out.status.success());
        assert!(
            out.stdout.is_empty(),
            "{tool}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

#[test]
fn a_structural_drift_stops_tools_except_asking_and_read_only_diagnosis() {
    let (home, workspace) = permission_home();
    std::fs::create_dir_all(workspace.join(".claude")).unwrap();
    std::fs::write(
        workspace.join(".claude/settings.local.json"),
        r#"{"permissions":{"allow":["Edit(/etc)"]}}"#,
    )
    .unwrap();
    let denied = |tool: &str, args: serde_json::Value| {
        let out = permission_call(home.path(), &workspace, tool, args);
        assert!(out.status.success());
        let v = json(&out);
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecision"], "deny",
            "{tool}"
        );
        let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_owned();
        // The file was never reviewed, so it is reported as a new source.
        assert!(reason.contains("new permission source"), "{reason}");
    };
    let passes = |tool: &str, args: serde_json::Value| {
        let out = permission_call(home.path(), &workspace, tool, args);
        assert!(out.status.success());
        assert!(
            out.stdout.is_empty(),
            "{tool}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    };
    denied("Read", serde_json::json!({"file_path":"plain.txt"}));
    denied("Bash", serde_json::json!({"command":"ls"}));
    denied(
        "Bash",
        serde_json::json!({"command":"agent-permissions inspect --json > /tmp/r.json"}),
    );
    denied(
        "Bash",
        serde_json::json!({"command":"agent-permissions migrate --approved /tmp/r.json"}),
    );
    passes("AskUserQuestion", serde_json::json!({"questions":[]}));
    passes("ExitPlanMode", serde_json::json!({}));
    passes(
        "Bash",
        serde_json::json!({"command":"agent-permissions check"}),
    );
    passes(
        "Bash",
        serde_json::json!({"command":"agent-permissions migrate --approved /tmp/r.json --dry-run"}),
    );
}

/// Review another project into the record, so the record lists its files.
fn review_project(home: &std::path::Path, project: &std::path::Path) {
    use agent_settings_reconcile::agent_permissions::{inspect, migrate, Policy};
    let raw = std::fs::read(home.join(".config/agent-permissions/policy.json")).unwrap();
    let policy: Policy = serde_json::from_slice(&raw).unwrap();
    let projects = [project.to_path_buf()];
    let review = inspect(&policy, home, &projects).unwrap();
    migrate(&policy, home, &projects, &review).unwrap();
}

fn read_plain(home: &std::path::Path, workspace: &std::path::Path) -> (bool, String) {
    let out = permission_call(
        home,
        workspace,
        "Read",
        serde_json::json!({"file_path":"plain.txt"}),
    );
    assert!(out.status.success());
    if out.stdout.is_empty() {
        return (true, String::new());
    }
    let v = json(&out);
    (
        false,
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

/// #1055: another worktree's empty or removed settings file stops no tool here,
/// while the same file in this session's own project still does, by name.
#[test]
fn another_projects_unreadable_or_removed_settings_do_not_stop_this_session() {
    let (home, workspace) = permission_home();
    let other = home.path().join("other");
    std::fs::create_dir_all(other.join(".claude")).unwrap();
    std::fs::write(other.join(".claude/settings.local.json"), "{}").unwrap();
    std::fs::write(other.join(".claude/settings.json"), "{}").unwrap();
    review_project(home.path(), &other);
    assert!(read_plain(home.path(), &workspace).0);

    // The empty, read-only file a sandbox leaves behind (0 bytes).
    std::fs::write(other.join(".claude/settings.local.json"), "").unwrap();
    std::fs::remove_file(other.join(".claude/settings.json")).unwrap();
    assert!(
        read_plain(home.path(), &workspace).0,
        "a project this session does not read must not stop it"
    );

    // The same breakage in the session's own project stops it, naming that one file.
    std::fs::create_dir_all(workspace.join(".claude")).unwrap();
    std::fs::write(workspace.join(".claude/settings.local.json"), "").unwrap();
    let (passed, reason) = read_plain(home.path(), &workspace);
    assert!(!passed);
    assert!(reason.contains("settings.local.json"), "{reason}");
    assert!(reason.contains("unreadable"), "{reason}");
    assert!(
        !reason.contains("other/"),
        "only this session's file is named: {reason}"
    );
    assert!(reason.contains("--project"), "{reason}");
}

/// #1062: a new worktree's committed `.mcp.json` is the checkout's file; the same
/// bytes as a reviewed one are not a new permission source, different bytes are.
#[test]
fn a_worktree_mcp_json_equal_to_a_reviewed_one_is_not_a_new_source() {
    let (home, workspace) = permission_home();
    let main = home.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    std::fs::write(main.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    review_project(home.path(), &main);

    let worktree = home.path().join("worktrees/feature");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    assert!(read_plain(home.path(), &worktree).0);

    std::fs::write(
        worktree.join(".mcp.json"),
        r#"{"mcpServers":{"x":{"command":"x"}}}"#,
    )
    .unwrap();
    let (passed, reason) = read_plain(home.path(), &worktree);
    assert!(!passed);
    assert!(reason.contains("new permission source"), "{reason}");
    assert!(
        reason.contains(&format!("--project {}", worktree.display())),
        "the recovery names the project to inspect: {reason}"
    );
    // The reviewed workspace is untouched.
    assert!(read_plain(home.path(), &workspace).0);
}
