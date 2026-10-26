//! Standing allow rules that appear between reviews, and the few calls that stay
//! possible while a real drift blocks everything else.
//!
//! "Yes, and don't ask again" and a sandbox network approval append a rule to
//! the writable `settings.local.json`. That is the owner's own decision, so it
//! must not stop every session; it is recorded as *pending* and reclaimed at a
//! session boundary. Anything outside the closed vocabulary below is a structural
//! drift and stays fail-closed.
use super::inventory::Source;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// Tolerated allow rules found in one writable settings file.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Pending {
    pub path: PathBuf,
    pub rules: Vec<String>,
}

/// The closed vocabulary of allow rules that may wait for reclaim: an exact
/// network host, a web search, one MCP tool, or a Bash rule without a mid-string
/// wildcard (a trailing `:*` or ` *` is the only wildcard form; dotfiles#461).
/// Edit/Write/Read rules, bare tool names and unknown syntax are not tolerated.
pub fn tolerated_rule(rule: &str) -> bool {
    if rule == "WebSearch" {
        return true;
    }
    if let Some(host) = rule
        .strip_prefix("WebFetch(domain:")
        .and_then(|r| r.strip_suffix(')'))
    {
        return !host.is_empty()
            && !host.starts_with(['.', '-'])
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    }
    if let Some(inner) = rule.strip_prefix("Bash(").and_then(|r| r.strip_suffix(')')) {
        let body = inner
            .strip_suffix(":*")
            .or_else(|| inner.strip_suffix(" *"))
            .unwrap_or(inner);
        return !body.trim().is_empty()
            && !body.contains('*')
            && !inner.chars().any(char::is_control);
    }
    rule.len() > "mcp__".len()
        && rule.starts_with("mcp__")
        && rule
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn split_allow(grants: &Value) -> Option<(Vec<String>, Value)> {
    let allow = match grants.pointer("/permissions/allow") {
        None => Vec::new(),
        Some(Value::Array(rules)) => rules
            .iter()
            .map(|r| r.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?,
        Some(_) => return None,
    };
    let mut rest = grants.clone();
    if let Some(permissions) = rest.get_mut("permissions").and_then(Value::as_object_mut) {
        permissions.remove("allow");
        if permissions.is_empty() {
            rest.as_object_mut()?.remove("permissions");
        }
    }
    Some((allow, rest))
}

/// `Some(added rules)` when `new` differs from the recorded `old` only by
/// tolerated additions to `permissions.allow` of a writable Claude settings file.
/// `Some(vec![])` when the two mean the same thing: `allow: []` and a missing
/// `allow` are equivalent, which is what a reclaim leaves behind.
pub(super) fn tolerated_additions(old: &Source, new: &Source) -> Option<Vec<String>> {
    if old.kind != "claude-settings"
        || new.kind != old.kind
        || !old.writable
        || !new.writable
        || old.unknown != new.unknown
    {
        return None;
    }
    let (old_allow, old_rest) = split_allow(&old.grants)?;
    let (new_allow, new_rest) = split_allow(&new.grants)?;
    if old_rest != new_rest || old_allow.iter().any(|r| !new_allow.contains(r)) {
        return None;
    }
    let mut added: Vec<String> = Vec::new();
    for rule in new_allow {
        if !old_allow.contains(&rule) && !added.contains(&rule) {
            added.push(rule);
        }
    }
    added.iter().all(|r| tolerated_rule(r)).then_some(added)
}

/// A writable Claude settings file that the record has never seen, such as the
/// `settings.local.json` Claude Code creates the first time "don't ask again" is
/// chosen in a project. `Some(rules)` when it holds nothing, or only tolerated
/// allow rules, compared with a blank file; `None` when it holds anything else.
pub(super) fn tolerated_new_source(new: &Source) -> Option<Vec<String>> {
    if new.kind != "claude-settings" || !new.writable || !new.unknown.is_empty() {
        return None;
    }
    let (allow, rest) = split_allow(&new.grants)?;
    if !rest.as_object().is_some_and(serde_json::Map::is_empty) {
        return None;
    }
    let mut rules: Vec<String> = Vec::new();
    for rule in allow {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules.iter().all(|r| tolerated_rule(r)).then_some(rules)
}

/// Calls that stay possible while the permission record is not trustworthy, so
/// the agent can still ask the owner and diagnose: tools that act on nothing, and
/// read-only `agent-permissions` invocations. No shell syntax is interpreted; a
/// command that is not one plain word list from a small character set is refused.
pub(super) fn inert_call(tool: Option<&str>, input: &Value) -> bool {
    match tool {
        Some("AskUserQuestion" | "ExitPlanMode") => true,
        Some("Bash") => input["command"].as_str().is_some_and(read_only_cli),
        _ => false,
    }
}

fn read_only_cli(command: &str) -> bool {
    let tokens: Vec<&str> = command
        .split([' ', '\t'])
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.first() != Some(&"agent-permissions")
        || !tokens.iter().all(|t| {
            t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._/=:@+-".contains(&b))
        })
    {
        return false;
    }
    let args = &tokens[1..];
    if args.iter().any(|a| matches!(*a, "--help" | "-h" | "help")) {
        return true;
    }
    match args.first() {
        Some(&"inspect" | &"check") => true,
        // Listing the queue only reads; withdrawing candidates appends.
        Some(&"pending") => !args.contains(&"--drop"),
        // --restore acts before --dry-run is looked at, so it is never read-only.
        Some(&"migrate") => {
            args.contains(&"--dry-run")
                && !args
                    .iter()
                    .any(|a| matches!(*a, "--restore" | "--retire-ownership"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(allow: Value, extra: Value) -> Source {
        let mut grants = json!({"permissions": {"allow": allow}});
        for (k, v) in extra.as_object().unwrap() {
            grants[k] = v.clone();
        }
        Source {
            path: "/p/.claude/settings.local.json".into(),
            kind: "claude-settings".into(),
            writable: true,
            digest: String::new(),
            grants,
            unknown: vec![],
        }
    }

    #[test]
    fn the_vocabulary_is_closed() {
        for ok in [
            "WebFetch(domain:example.com)",
            "WebSearch",
            "mcp__server__tool",
            "mcp__plugin_x_y__query-docs",
            "Bash(git status)",
            "Bash(git status:*)",
            "Bash(npm run *)",
        ] {
            assert!(tolerated_rule(ok), "{ok}");
        }
        for bad in [
            "WebFetch",
            "WebFetch(domain:*.example.com)",
            "WebFetch(domain:)",
            "WebFetch(domain:a b)",
            "WebSearch(x)",
            "Bash",
            "Bash(*)",
            "Bash(:*)",
            "Bash(ps -p * -o pid,cmd)",
            "Bash(* install)",
            "Bash(a\nb)",
            "Edit(/tmp/x)",
            "Write(/tmp/x)",
            "Read(~/.ssh/**)",
            "Skill(x)",
            "mcp__",
            "mcp__server__*",
            "",
        ] {
            assert!(!tolerated_rule(bad), "{bad}");
        }
    }

    #[test]
    fn only_added_tolerated_allows_are_pending() {
        let old = source(json!(["Bash(ls)"]), json!({}));
        let added = source(json!(["Bash(ls)", "WebFetch(domain:a.example)"]), json!({}));
        assert_eq!(
            tolerated_additions(&old, &added),
            Some(vec!["WebFetch(domain:a.example)".into()])
        );
        assert_eq!(
            tolerated_additions(&old, &old),
            Some(vec![]),
            "nothing added means the same thing"
        );
        let emptied = source(json!([]), json!({}));
        let mut missing = emptied.clone();
        missing.grants = json!({});
        assert_eq!(tolerated_additions(&missing, &emptied), Some(vec![]));
        let edit = source(json!(["Bash(ls)", "Edit(/x)"]), json!({}));
        assert_eq!(tolerated_additions(&old, &edit), None);
        let removed = source(json!([]), json!({}));
        assert_eq!(tolerated_additions(&old, &removed), None);
        // Any other field changing makes the whole difference structural.
        let sandbox = source(
            json!(["Bash(ls)", "WebSearch"]),
            json!({"sandbox": {"enabled": false}}),
        );
        assert_eq!(tolerated_additions(&old, &sandbox), None);
        let mut unknown = added.clone();
        unknown.unknown.push("unrecognized permission rule".into());
        assert_eq!(tolerated_additions(&old, &unknown), None);
        let mut shared = added.clone();
        shared.writable = false;
        assert_eq!(tolerated_additions(&old, &shared), None);
    }

    #[test]
    fn a_new_permissions_object_can_hold_pending_rules() {
        let mut old = source(json!([]), json!({}));
        old.grants = json!({});
        let new = source(json!(["WebSearch"]), json!({}));
        assert_eq!(
            tolerated_additions(&old, &new),
            Some(vec!["WebSearch".into()])
        );
        // An ask or deny added next to it is not an allow.
        let mut denied = new.clone();
        denied.grants["permissions"]["deny"] = json!(["Read(x)"]);
        assert_eq!(tolerated_additions(&old, &denied), None);
    }

    #[test]
    fn a_new_local_settings_file_may_hold_only_pending_rules() {
        let blank = source(json!([]), json!({}));
        assert_eq!(tolerated_new_source(&blank), Some(vec![]));
        let one = source(json!(["WebSearch", "WebSearch"]), json!({}));
        assert_eq!(tolerated_new_source(&one), Some(vec!["WebSearch".into()]));
        let edit = source(json!(["Edit(/x)"]), json!({}));
        assert_eq!(tolerated_new_source(&edit), None);
        let sandbox = source(json!([]), json!({"sandbox": {"enabled": false}}));
        assert_eq!(tolerated_new_source(&sandbox), None);
        let mut shared = one.clone();
        shared.writable = false;
        assert_eq!(tolerated_new_source(&shared), None);
        let mut other = one;
        other.kind = "codex-rules".into();
        assert_eq!(tolerated_new_source(&other), None);
    }

    #[test]
    fn only_inert_tools_and_read_only_cli_pass_a_structural_drift() {
        let no = json!({});
        assert!(inert_call(Some("AskUserQuestion"), &no));
        assert!(inert_call(Some("ExitPlanMode"), &no));
        assert!(!inert_call(Some("Read"), &no));
        assert!(!inert_call(None, &no));
        for ok in [
            "agent-permissions inspect",
            "agent-permissions inspect --json",
            "agent-permissions check",
            "agent-permissions pending",
            "agent-permissions pending --json",
            "agent-permissions migrate --approved /tmp/r.json --dry-run",
            "agent-permissions --help",
            "agent-permissions migrate --help",
        ] {
            assert!(inert_call(Some("Bash"), &json!({"command": ok})), "{ok}");
        }
        for bad in [
            "agent-permissions migrate --approved /tmp/r.json",
            "agent-permissions migrate --restore /tmp/j.json --dry-run",
            "agent-permissions migrate --retire-ownership --dry-run",
            "agent-permissions apply",
            "agent-permissions reclaim",
            "agent-permissions pending --drop abc",
            "agent-permissions hook session-start",
            "agent-permissions inspect --json > /tmp/r.json",
            "agent-permissions inspect | jq .",
            "agent-permissions inspect; rm -rf /",
            "agent-permissions inspect && rm x",
            "agent-permissions inspect $(id)",
            "agent-permissions inspect 'x'",
            "agent-permissions inspect\nrm x",
            "/tmp/agent-permissions inspect",
            "~/.local/bin/agent-permissions inspect",
            "env agent-permissions inspect",
            "ls",
            "",
        ] {
            assert!(!inert_call(Some("Bash"), &json!({"command": bad})), "{bad}");
        }
        assert!(!inert_call(Some("Bash"), &json!({})));
        assert!(!inert_call(
            Some("Write"),
            &json!({"command": "agent-permissions inspect"})
        ));
    }
}
