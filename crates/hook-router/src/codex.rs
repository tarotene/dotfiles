//! Codex 0.164.0 hook boundary. The rule table consumes the shared Bash/Write/Edit
//! vocabulary; apply_patch retains its canonical name in upstream hook payloads.
//! See docs/claude/codex-hook-parity.md for the pinned wire-format evidence.

use hook_io::{Decision, HookInput};
use serde_json::{json, Value};
use std::path::Path;

/// Expand every affected path, including both ends of a move. This extracts
/// patch headers, not hunks: upstream remains responsible for applying/validating
/// the patch. Hunk lines begin with space/+/-, so cannot impersonate a header.
pub fn inputs(raw: &str) -> Vec<String> {
    let Ok(input) = serde_json::from_str::<Value>(raw) else {
        return vec![];
    };
    if input["tool_name"] != "apply_patch" {
        return vec![raw.to_owned()];
    }
    let Some(patch) = input["tool_input"]["command"].as_str() else {
        return vec![];
    };
    let cwd = Path::new(input["cwd"].as_str().unwrap_or("."));
    let mut calls: Vec<Value> = Vec::new();
    let mut source = None;
    let mut updating = false;
    for line in patch.lines() {
        // Upstream streaming_parser trims both ends outside update hunks, but
        // only the right end within one: a leading space there is file context.
        // See rust-v0.164.0 streaming_parser.rs:176-199,228-255.
        let header_line = if updating {
            line.trim_end()
        } else {
            line.trim()
        };
        let header = [
            ("*** Add File: ", "Write"),
            ("*** Update File: ", "Edit"),
            ("*** Delete File: ", "Edit"),
            ("*** Move to: ", "Write"),
        ]
        .into_iter()
        .find_map(|(prefix, tool)| header_line.strip_prefix(prefix).map(|p| (prefix, tool, p)));
        if let Some((prefix, tool, path)) = header {
            updating = matches!(prefix, "*** Update File: " | "*** Move to: ");
            let path = cwd.join(path);
            let content = if prefix == "*** Move to: " {
                source
                    .as_ref()
                    .and_then(|p: &std::path::PathBuf| std::fs::read_to_string(p).ok())
                    .unwrap_or_default()
            } else {
                source = Some(path.clone());
                String::new()
            };
            let mut call = input.clone();
            call["tool_name"] = json!(tool);
            call["tool_input"] = json!({"file_path": path, "content": content});
            calls.push(call);
        } else if let Some(content) = line.strip_prefix('+') {
            if let Some(call) = calls.last_mut() {
                let text = call["tool_input"]["content"].as_str().unwrap_or_default();
                call["tool_input"]["content"] = json!(format!("{text}{content}\n"));
            }
        }
    }
    calls.into_iter().map(|v| v.to_string()).collect()
}

/// Codex ignores PreToolUse allow. Use PermissionRequest for the existing narrow
/// allow engines, and let the normal prompt handle every non-matching command.
pub fn permission(raw: &str) -> Option<Value> {
    let input = HookInput::parse(raw)?;
    if input.hook_event_name != "PermissionRequest" {
        return None;
    }
    let command = input.bash_command()?;
    let worktree = git_worktree_allow::worktrees_root()
        .and_then(|root| git_worktree_allow::decide(command, &root));
    let edit = || {
        if gh_edit_allow::skipped() {
            return None;
        }
        let ledger = gh_edit_allow::default_ledger()?;
        let origin = input
            .project_dir()
            .and_then(|d| hook_io::git::origin_nwo(&d));
        gh_edit_allow::check(&input, &ledger, origin.as_deref())
    };
    if worktree.is_some() || edit().is_some_and(|d| d.decision == Decision::Allow) {
        Some(
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}),
        )
    } else {
        None
    }
}

/// Bridge a locally owned command hook without copying its private rules into
/// dotfiles. Codex cannot honour PreToolUse ask/allow: ask must block with the
/// original remediation, while allow leaves the ordinary permission flow intact.
pub fn compatibility(command: &str, raw: &str) -> Option<Value> {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command]);
    let (status, bytes) = hook_io::proc::output_with_timeout(
        &mut cmd,
        std::time::Duration::from_secs(30),
        Some(raw.as_bytes()),
    )?;
    if status.code() == Some(2) {
        return Some(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
            "permissionDecision":"deny", "permissionDecisionReason":"Local hook rejected the operation."}}));
    }
    let mut out: Value = serde_json::from_slice(&bytes).ok()?;
    match out["hookSpecificOutput"]["permissionDecision"].as_str() {
        Some("ask") => out["hookSpecificOutput"]["permissionDecision"] = json!("deny"),
        Some("allow") => {
            out["hookSpecificOutput"]
                .as_object_mut()?
                .remove("permissionDecision");
        }
        _ => {}
    }
    Some(out)
}

/// Atuin 18.15.2 supports Codex, but defaults a missing exitCode to success.
/// Unified exec 0.164.0 supplies only stdout: preserve that uncertainty as -1.
pub fn atuin_input(raw: &str) -> Option<Value> {
    let mut input: Value = serde_json::from_str(raw).ok()?;
    if input["tool_name"] != "Bash" {
        return None;
    }
    if input["hook_event_name"] == "PostToolUse" && !input["tool_response"]["exitCode"].is_i64() {
        input["tool_response"] = json!({"exitCode": -1});
    }
    Some(input)
}

pub fn atuin(raw: &str) {
    let Some(input) = atuin_input(raw) else {
        return;
    };
    let mut cmd = std::process::Command::new("atuin");
    cmd.args(["hook", "codex"]);
    if let Some(cwd) = input["cwd"].as_str() {
        cmd.current_dir(cwd).env("PWD", cwd);
    }
    let _ = hook_io::proc::output_with_timeout(
        &mut cmd,
        std::time::Duration::from_secs(8),
        Some(input.to_string().as_bytes()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_exit_status_is_not_recorded_as_success() {
        let raw =
            json!({"tool_name":"Bash", "hook_event_name":"PostToolUse", "tool_response":"stdout"});
        assert_eq!(
            atuin_input(&raw.to_string()).unwrap()["tool_response"]["exitCode"],
            -1
        );
        let mut raw = raw;
        raw["tool_response"] = json!({"exitCode":7});
        assert_eq!(
            atuin_input(&raw.to_string()).unwrap()["tool_response"]["exitCode"],
            7
        );
    }
    #[test]
    fn patch_covers_add_delete_update_and_both_move_paths() {
        let calls = inputs(&json!({"tool_name":"apply_patch", "cwd":"/repo", "tool_input":{"command":
            "*** Begin Patch\n*** Add File: new\n+text\n*** Delete File: gone\n*** Update File: old\n*** Move to: moved\n@@\n-x\n+y\n*** End Patch"}}).to_string());
        let calls: Vec<Value> = calls
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            calls
                .iter()
                .map(|v| v["tool_input"]["file_path"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["/repo/new", "/repo/gone", "/repo/old", "/repo/moved"]
        );
        assert_eq!(calls[0]["tool_input"]["content"], "text\n");
        assert_eq!(calls[3]["tool_name"], "Write");
    }

    #[test]
    fn patch_header_whitespace_matches_upstream_without_reinterpreting_context() {
        let calls = inputs(&json!({"tool_name":"apply_patch", "cwd":"/repo", "tool_input":{"command":
            "*** Begin Patch\n  *** Add File: new  \n+text\n\t*** Delete File: gone\t\n  *** Update File: old  \n*** Move to: moved  \n@@\n *** Add File: this-is-context\n-x\n+y\n*** End Patch"}}).to_string());
        let paths: Vec<String> = calls
            .iter()
            .map(|s| {
                let v: Value = serde_json::from_str(s).unwrap();
                v["tool_input"]["file_path"].as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(
            paths,
            ["/repo/new", "/repo/gone", "/repo/old", "/repo/moved"]
        );
    }
}
