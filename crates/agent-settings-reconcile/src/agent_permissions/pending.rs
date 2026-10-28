//! Standing allow rules that appear between reviews, and the few calls that stay
//! possible while a real drift blocks everything else.
//!
//! "Yes, and don't ask again" and a sandbox network approval append a rule to
//! the writable `settings.local.json` (Codex: `default.rules` and `config.toml`). That is the owner's own decision, so it
//! must not stop every session; it is recorded as *pending* and reclaimed at a
//! session boundary. Anything outside the closed vocabulary below is a structural
//! drift and stays fail-closed.
use super::inventory::{Source, OPAQUE_RULES};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// How to put a settings file back to what the ownership record says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revert {
    /// Claude Code: remove the pending rules from `permissions.allow`.
    Allow,
    /// Codex rules file: write back the recorded text (the file only grew).
    Rules(String),
    /// Codex `config.toml`: restore tool approvals to what they were.
    Tools(Vec<ToolRevert>),
}

/// One Codex tool approval: the key path of `approval_mode`, and its recorded
/// value (`None` = the key did not exist).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRevert {
    pub path: Vec<String>,
    pub was: Option<String>,
}

/// Tolerated changes found in one writable permission file.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Pending {
    pub path: PathBuf,
    pub rules: Vec<String>,
    #[serde(skip)]
    pub revert: Revert,
}

/// What a tolerated difference consists of, and how to undo it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Tolerated {
    pub rules: Vec<String>,
    pub revert: Revert,
}

fn exact_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with(['.', '-'])
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// The closed vocabulary of Claude allow rules that may wait for reclaim: an exact
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
        return exact_host(host);
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
fn claude_additions(old: &Source, new: &Source) -> Option<Vec<String>> {
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
fn claude_new_source(new: &Source) -> Option<Vec<String>> {
    if new.kind != "claude-settings" || !new.writable || !new.unknown.is_empty() {
        return None;
    }
    let (allow, rest) = split_allow(&new.grants)?;
    if !rest.as_object().is_some_and(Map::is_empty) {
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

/// Parse one JSON string at the start of `text`; return it and the rest.
fn json_string(text: &str) -> Option<(String, &str)> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<String>();
    let value = stream.next()?.ok()?;
    Some((value, &text[stream.byte_offset()..]))
}

/// The two line shapes Codex appends to `default.rules` when a command prefix or a
/// host is approved for good (OpenAI, Codex rust-v0.164.0, `execpolicy/src/amend.rs`
/// L65-L124 and `execpolicy/src/rule.rs` L138-L145, retrieved 2026-10-26):
///
/// ```text
/// prefix_rule(pattern=["a", "b"], decision="allow")
/// network_rule(host="h", protocol="https", decision="allow"[, justification="j"])
/// ```
///
/// Starlark is never interpreted: a line is tolerated only when it is exactly
/// what Codex writes, with an exact host and an `allow` decision.
fn tolerated_codex_line(line: &str) -> bool {
    if let Some(tokens) = line
        .strip_prefix("prefix_rule(pattern=[")
        .and_then(|r| r.strip_suffix("], decision=\"allow\")"))
    {
        let mut parsed: Vec<String> = Vec::new();
        let mut rest = tokens;
        while let Some((token, after)) = json_string(rest) {
            parsed.push(token);
            if after.is_empty() {
                break;
            }
            let Some(next) = after.strip_prefix(", ") else {
                return false;
            };
            rest = next;
        }
        let again = parsed
            .iter()
            .map(|t| serde_json::to_string(t).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(", ");
        return !parsed.is_empty() && again == tokens;
    }
    let Some(rest) = line.strip_prefix("network_rule(host=") else {
        return false;
    };
    let Some((host, rest)) = json_string(rest) else {
        return false;
    };
    let Some((protocol, rest)) = rest.strip_prefix(", protocol=").and_then(json_string) else {
        return false;
    };
    let Some((decision, rest)) = rest.strip_prefix(", decision=").and_then(json_string) else {
        return false;
    };
    let justified = match rest {
        ")" => true,
        _ => rest
            .strip_prefix(", justification=")
            .and_then(json_string)
            .is_some_and(|(why, tail)| {
                tail == ")" && !why.trim().is_empty() && !why.chars().any(char::is_control)
            }),
    };
    justified
        && exact_host(&host)
        && ["http", "https", "socks5_tcp", "socks5_udp"].contains(&protocol.as_str())
        && decision == "allow"
}

/// A Codex `default.rules` that only grew since the record, by lines of the shape
/// above. `old_text` is the recorded content (empty for a file never reviewed).
fn codex_rules_growth(old_text: &str, new: &Source) -> Option<Tolerated> {
    if new.kind != "codex-rules"
        || !new.writable
        || new.path.file_name().is_none_or(|n| n != "default.rules")
        || new.unknown.iter().any(|u| u != OPAQUE_RULES)
    {
        return None;
    }
    let text = new.grants.get("rules")?.as_str()?;
    let appended = text.strip_prefix(old_text)?;
    let mut rules: Vec<String> = Vec::new();
    for line in appended.lines().filter(|l| !l.trim().is_empty()) {
        if !tolerated_codex_line(line) {
            return None;
        }
        if !rules.iter().any(|r| r == line) {
            rules.push(line.to_owned());
        }
    }
    Some(Tolerated {
        rules,
        revert: Revert::Rules(old_text.to_owned()),
    })
}

fn is_tool_policy(path: &[String]) -> bool {
    match path {
        [root, _, tools, _] => (root == "mcp_servers" || root == "apps") && tools == "tools",
        [plugins, _, servers, _, tools, _] => {
            plugins == "plugins" && servers == "mcp_servers" && tools == "tools"
        }
        _ => false,
    }
}

/// Walk two `grants` trees. Only a tool whose `approval_mode` became `approve`
/// (from absent or `prompt`), with everything else about the tool unchanged, is
/// tolerated; any other difference makes the whole comparison fail.
fn walk_tools(
    path: &mut Vec<String>,
    old: Option<&Value>,
    new: &Value,
    out: &mut Vec<ToolRevert>,
) -> bool {
    if old == Some(new) {
        return true;
    }
    let Some(new_fields) = new.as_object() else {
        return false;
    };
    let old_fields = match old {
        None => Map::new(),
        Some(Value::Object(fields)) => fields.clone(),
        Some(_) => return false,
    };
    if is_tool_policy(path) {
        let (mut rest_new, mut rest_old) = (new_fields.clone(), old_fields);
        let (mode, was) = (
            rest_new.remove("approval_mode"),
            rest_old.remove("approval_mode"),
        );
        let was_ok = matches!(was.as_ref().map(Value::as_str), None | Some(Some("prompt")));
        if rest_new != rest_old
            || !was_ok
            || mode.as_ref().and_then(Value::as_str) != Some("approve")
        {
            return false;
        }
        let mut key = path.clone();
        key.push("approval_mode".into());
        out.push(ToolRevert {
            path: key,
            was: was.and_then(|v| v.as_str().map(str::to_owned)),
        });
        return true;
    }
    if old_fields.keys().any(|k| !new_fields.contains_key(k)) {
        return false;
    }
    for (key, value) in new_fields {
        path.push(key.clone());
        let ok = walk_tools(path, old_fields.get(key), value, out);
        path.pop();
        if !ok {
            return false;
        }
    }
    true
}

fn codex_tool_approvals(old: &Source, new: &Source) -> Option<Tolerated> {
    if old.kind != "codex-config"
        || new.kind != old.kind
        || !old.writable
        || !new.writable
        || old.unknown != new.unknown
    {
        return None;
    }
    let mut changes = Vec::new();
    if !walk_tools(
        &mut Vec::new(),
        Some(&old.grants),
        &new.grants,
        &mut changes,
    ) {
        return None;
    }
    let rules = changes
        .iter()
        .map(|c| format!("{}=approve", c.path.join(".")))
        .collect();
    Some(Tolerated {
        rules,
        revert: Revert::Tools(changes),
    })
}

/// Tolerated changes of a source already in the record. `Some` with no rules means
/// "nothing to reclaim" (the two sides mean the same thing); `None` is a
/// structural drift.
pub(super) fn tolerated_changes(old: &Source, new: &Source) -> Option<Tolerated> {
    if old.kind != new.kind || !old.writable || !new.writable {
        return None;
    }
    match old.kind.as_str() {
        "claude-settings" => claude_additions(old, new).map(|rules| Tolerated {
            rules,
            revert: Revert::Allow,
        }),
        "codex-rules" if old.unknown.iter().all(|u| u == OPAQUE_RULES) => {
            codex_rules_growth(old.grants.get("rules")?.as_str()?, new)
        }
        "codex-config" => codex_tool_approvals(old, new),
        _ => None,
    }
}

/// Tolerated content of a writable source that the record has never seen: a file
/// Claude Code or Codex created on the first approval saved in a project or home.
pub(super) fn tolerated_new_source(new: &Source) -> Option<Tolerated> {
    match new.kind.as_str() {
        "claude-settings" => claude_new_source(new).map(|rules| Tolerated {
            rules,
            revert: Revert::Allow,
        }),
        "codex-rules" => codex_rules_growth("", new),
        _ => None,
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
            claude_additions(&old, &added),
            Some(vec!["WebFetch(domain:a.example)".into()])
        );
        assert_eq!(
            claude_additions(&old, &old),
            Some(vec![]),
            "nothing added means the same thing"
        );
        let emptied = source(json!([]), json!({}));
        let mut missing = emptied.clone();
        missing.grants = json!({});
        assert_eq!(claude_additions(&missing, &emptied), Some(vec![]));
        let edit = source(json!(["Bash(ls)", "Edit(/x)"]), json!({}));
        assert_eq!(claude_additions(&old, &edit), None);
        let removed = source(json!([]), json!({}));
        assert_eq!(claude_additions(&old, &removed), None);
        // Any other field changing makes the whole difference structural.
        let sandbox = source(
            json!(["Bash(ls)", "WebSearch"]),
            json!({"sandbox": {"enabled": false}}),
        );
        assert_eq!(claude_additions(&old, &sandbox), None);
        let mut unknown = added.clone();
        unknown.unknown.push("unrecognized permission rule".into());
        assert_eq!(claude_additions(&old, &unknown), None);
        let mut shared = added.clone();
        shared.writable = false;
        assert_eq!(claude_additions(&old, &shared), None);
    }

    #[test]
    fn a_new_permissions_object_can_hold_pending_rules() {
        let mut old = source(json!([]), json!({}));
        old.grants = json!({});
        let new = source(json!(["WebSearch"]), json!({}));
        assert_eq!(claude_additions(&old, &new), Some(vec!["WebSearch".into()]));
        // An ask or deny added next to it is not an allow.
        let mut denied = new.clone();
        denied.grants["permissions"]["deny"] = json!(["Read(x)"]);
        assert_eq!(claude_additions(&old, &denied), None);
    }

    #[test]
    fn a_new_local_settings_file_may_hold_only_pending_rules() {
        let blank = source(json!([]), json!({}));
        assert_eq!(claude_new_source(&blank), Some(vec![]));
        let one = source(json!(["WebSearch", "WebSearch"]), json!({}));
        assert_eq!(claude_new_source(&one), Some(vec!["WebSearch".into()]));
        let edit = source(json!(["Edit(/x)"]), json!({}));
        assert_eq!(claude_new_source(&edit), None);
        let sandbox = source(json!([]), json!({"sandbox": {"enabled": false}}));
        assert_eq!(claude_new_source(&sandbox), None);
        let mut shared = one.clone();
        shared.writable = false;
        assert_eq!(claude_new_source(&shared), None);
        let mut other = one;
        other.kind = "codex-rules".into();
        assert_eq!(claude_new_source(&other), None);
    }

    fn rules_source(text: &str) -> Source {
        Source {
            path: "/h/.codex/rules/default.rules".into(),
            kind: "codex-rules".into(),
            writable: true,
            digest: String::new(),
            grants: json!({"opaque_rules": !text.trim().is_empty(), "rules": text}),
            unknown: if text.trim().is_empty() {
                vec![]
            } else {
                vec![OPAQUE_RULES.to_owned()]
            },
        }
    }

    #[test]
    fn codex_lines_are_tolerated_only_in_the_shape_codex_writes() {
        for ok in [
            r#"prefix_rule(pattern=["ls"], decision="allow")"#,
            r#"prefix_rule(pattern=["git", "status"], decision="allow")"#,
            r#"prefix_rule(pattern=["echo", "Hello, world!"], decision="allow")"#,
            r#"network_rule(host="example.com", protocol="https", decision="allow")"#,
            r#"network_rule(host="a.example.com", protocol="socks5_tcp", decision="allow", justification="needed for a build")"#,
        ] {
            assert!(tolerated_codex_line(ok), "{ok}");
        }
        for bad in [
            r#"prefix_rule(pattern=[], decision="allow")"#,
            r#"prefix_rule(pattern=["ls"], decision="prompt")"#,
            r#"prefix_rule(pattern=["ls"], decision="forbidden")"#,
            r#"prefix_rule(pattern=["ls"],decision="allow")"#,
            r#"prefix_rule(pattern=['ls'], decision="allow")"#,
            r#"prefix_rule(pattern=["ls"], decision="allow", justification="x")"#,
            r#"prefix_rule(pattern=["ls"], decision="allow") # comment"#,
            r#"prefix_rule(pattern=["a"], decision="allow"); prefix_rule(pattern=["b"], decision="allow")"#,
            r#"network_rule(host="*.example.com", protocol="https", decision="allow")"#,
            r#"network_rule(host="example.com", protocol="ftp", decision="allow")"#,
            r#"network_rule(host="example.com", protocol="https", decision="deny")"#,
            r#"network_rule(host="example.com", protocol="https", decision="allow", justification=" ")"#,
            r#"network_rule(host="example.com", protocol="https", decision="allow") "#,
            r#"x = 1"#,
            r#"import_module("x")"#,
            "",
        ] {
            assert!(!tolerated_codex_line(bad), "{bad}");
        }
    }

    #[test]
    fn a_rules_file_that_only_grew_by_codex_lines_is_pending() {
        let old = rules_source("");
        let one = r#"prefix_rule(pattern=["ls"], decision="allow")"#;
        let two = r#"network_rule(host="example.com", protocol="https", decision="allow")"#;
        let new = rules_source(&format!("{one}\n{two}\n"));
        let t = tolerated_changes(&old, &new).unwrap();
        assert_eq!(t.rules, [one, two]);
        assert_eq!(t.revert, Revert::Rules(String::new()));
        // A file never reviewed is compared with a blank one.
        assert_eq!(tolerated_new_source(&new).unwrap().rules, [one, two]);
        // Appending after existing recorded text keeps that text.
        let recorded = rules_source("# kept\n");
        let grown = rules_source(&format!("# kept\n{one}\n"));
        let t = tolerated_changes(&recorded, &grown).unwrap();
        assert_eq!(t.revert, Revert::Rules("# kept\n".into()));
        // Any line outside the grammar, an edit of the recorded text, or another
        // file name is a structural drift.
        let other = rules_source(&format!("{one}\nx = 1\n"));
        assert_eq!(tolerated_changes(&old, &other), None);
        let edited = rules_source(&format!("# changed\n{one}\n"));
        assert_eq!(tolerated_changes(&recorded, &edited), None);
        let mut renamed = new.clone();
        renamed.path = "/h/.codex/rules/other.rules".into();
        assert_eq!(tolerated_changes(&old, &renamed), None);
        let mut shared = new;
        shared.writable = false;
        assert_eq!(tolerated_changes(&old, &shared), None);
    }

    fn config_source(grants: Value) -> Source {
        Source {
            path: "/h/.codex/config.toml".into(),
            kind: "codex-config".into(),
            writable: true,
            digest: String::new(),
            grants,
            unknown: vec![],
        }
    }

    #[test]
    fn only_a_tool_approval_changing_to_approve_is_pending_in_codex_config() {
        let old = config_source(json!({"approval_policy": "on-request",
            "mcp_servers": {"srv": {"default_tools_approval_mode": "prompt",
                "tools": {"seen": {"approval_mode": "prompt"}}}}}));
        let new = config_source(json!({"approval_policy": "on-request",
            "mcp_servers": {"srv": {"default_tools_approval_mode": "prompt",
                "tools": {"seen": {"approval_mode": "approve"},
                          "fresh": {"approval_mode": "approve"}}}},
            "apps": {"app": {"tools": {"lookup": {"approval_mode": "approve"}}}}}));
        let t = tolerated_changes(&old, &new).unwrap();
        let mut rules = t.rules.clone();
        rules.sort();
        assert_eq!(
            rules,
            [
                "apps.app.tools.lookup.approval_mode=approve",
                "mcp_servers.srv.tools.fresh.approval_mode=approve",
                "mcp_servers.srv.tools.seen.approval_mode=approve",
            ]
        );
        let Revert::Tools(items) = &t.revert else {
            panic!("tool approvals revert by key");
        };
        let seen = items
            .iter()
            .find(|i| i.path.contains(&"seen".to_owned()))
            .unwrap();
        assert_eq!(seen.was.as_deref(), Some("prompt"));
        let fresh = items
            .iter()
            .find(|i| i.path.contains(&"fresh".to_owned()))
            .unwrap();
        assert_eq!(fresh.was, None);
        // Everything else is structural.
        for changed in [
            json!({"approval_policy": "never", "mcp_servers": old.grants["mcp_servers"]}),
            json!({"approval_policy": "on-request",
                "mcp_servers": {"srv": {"default_tools_approval_mode": "approve",
                    "tools": {"seen": {"approval_mode": "prompt"}}}}}),
            json!({"approval_policy": "on-request",
                "mcp_servers": {"srv": {"default_tools_approval_mode": "prompt",
                    "tools": {"seen": {"approval_mode": "auto"}}}}}),
            json!({"approval_policy": "on-request",
                "mcp_servers": {"srv": {"default_tools_approval_mode": "prompt",
                    "tools": {"seen": {"approval_mode": "approve", "enabled": false}}}}}),
            json!({"approval_policy": "on-request", "mcp_servers": {"srv": {"enabled": true}}}),
            json!({"approval_policy": "on-request"}),
        ] {
            assert_eq!(
                tolerated_changes(&old, &config_source(changed.clone())),
                None,
                "{changed}"
            );
        }
    }
}
