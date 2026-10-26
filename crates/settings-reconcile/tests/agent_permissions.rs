use serde_json::{json, Value};
use settings_reconcile::agent_permissions::{apply, check, inspect, migrate, Policy};
use std::fs;
use std::path::Path;

fn policy() -> Policy {
    serde_json::from_value(json!({"version":1,"writable_roots":["workspace","temp"],
        "secret_paths":["~/.ssh","~/.codex/auth.json"],"domains":["github.com"],"tools":"prompt","projects":[],"project_rules":{}})).unwrap()
}
fn put(home: &Path, path: &str, data: &str) {
    let path = home.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

#[test]
fn migration_preserves_unowned_fields_asks_denies_and_native_trust() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    put(
        h,
        ".claude/settings.json",
        r#"{"model":"keep","hooks":{"Stop":[]},"permissions":{"allow":["Bash(*)"],"ask":["Bash(pkexec *)"],"deny":["Read(secret)"]}}"#,
    );
    put(h,".codex/config.toml","# keep me\nmodel = 'keep'\nsandbox_mode='danger-full-access'\n[hooks.state.example]\ntrusted=true\n[mcp_servers.example]\ncommand='mcp'\ndefault_tools_approval_mode='approve'\n[profiles.risky]\nsandbox_mode='danger-full-access'\n");
    let review = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &review).unwrap();
    check(&p, h, &[]).unwrap();
    let c: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(c["model"], "keep");
    assert_eq!(c["hooks"]["Stop"], json!([]));
    assert_eq!(c["permissions"]["allow"], json!([]));
    assert_eq!(c["permissions"]["ask"], json!(["Bash(pkexec *)"]));
    assert!(c["permissions"]["deny"]
        .as_array()
        .unwrap()
        .contains(&json!("Read(secret)")));
    assert!(c["permissions"]["deny"]
        .as_array()
        .unwrap()
        .contains(&json!("Read(~/.ssh/**)")));
    let c = fs::read_to_string(h.join(".codex/config.toml")).unwrap();
    assert!(c.contains("# keep me"));
    assert!(c.contains("trusted=true"));
    assert!(!c.contains("sandbox_mode"));
    assert!(c.contains("prompt"));
    assert!(c.contains("network_proxy = true"));
    let before = fs::metadata(h.join(".codex/config.toml"))
        .unwrap()
        .modified()
        .unwrap();
    apply(&p, h, &[]).unwrap();
    assert_eq!(
        before,
        fs::metadata(h.join(".codex/config.toml"))
            .unwrap()
            .modified()
            .unwrap()
    );
}

#[test]
fn stale_review_unknown_sources_and_new_grants_stop_without_writing() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    put(h, ".claude/settings.json", "{}");
    let r = inspect(&p, h, &[]).unwrap();
    put(h, ".claude/settings.json", r#"{"model":"changed"}"#);
    assert!(migrate(&p, h, &[], &r).unwrap_err().contains("stale"));
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    put(h, ".claude/settings.local.json", "{}"); // not a loaded project source without that project
    put(
        h,
        ".codex/rules/new.rules",
        "prefix_rule(pattern=['git'],decision='allow')",
    );
    assert!(check(&p, h, &[])
        .unwrap_err()
        .contains("new permission source"));
    let mut r = inspect(&p, h, &[]).unwrap();
    assert!(migrate(&p, h, &[], &r).is_err());
    r.decisions
        .insert(h.join(".codex/rules/new.rules"), "remove".into());
    migrate(&p, h, &[], &r).unwrap();
    assert!(fs::read(h.join(".codex/rules/new.rules"))
        .unwrap()
        .is_empty());
}

#[test]
fn shared_settings_are_retained_but_unknown_changes_are_blocked() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let project = d.path().join("repo");
    fs::create_dir_all(&h).unwrap();
    fs::create_dir_all(&project).unwrap();
    put(
        &project,
        ".claude/settings.json",
        r#"{"permissions":{"allow":["Bash(*)"]},"hooks":{"PreToolUse":[]}}"#,
    );
    let p = policy();
    let projects = vec![project.clone()];
    let r = inspect(&p, &h, &projects).unwrap();
    let before = fs::read(project.join(".claude/settings.json")).unwrap();
    migrate(&p, &h, &projects, &r).unwrap();
    assert_eq!(
        before,
        fs::read(project.join(".claude/settings.json")).unwrap()
    );
    put(
        &project,
        ".claude/settings.json",
        r#"{"permissions":{"allow":["Bash(*)"]},"model":"new"}"#,
    );
    assert!(check(&p, &h, &projects).unwrap_err().contains("drift"));
}

#[test]
fn narrowing_applies_but_expansion_requires_review() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let mut p = policy();
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    p.domains.clear();
    apply(&p, h, &[]).unwrap();
    check(&p, h, &[]).unwrap();
    p.domains.push("example.org".into());
    assert!(apply(&p, h, &[]).unwrap_err().contains("expansion"));
}

#[test]
fn corrupt_record_and_shared_boundary_overrides_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let repo = d.path().join("repo");
    fs::create_dir_all(&h).unwrap();
    fs::create_dir_all(&repo).unwrap();
    put(
        &repo,
        ".claude/settings.json",
        r#"{"sandbox":{"enabled":false}}"#,
    );
    let p = policy();
    let projects = vec![repo];
    let r = inspect(&p, &h, &projects).unwrap();
    assert!(migrate(&p, &h, &projects, &r)
        .unwrap_err()
        .contains("override"));
    assert!(!h.join(".claude/settings.json").exists());
    put(&h, ".local/state/dotfiles/agent-permissions.json", "{}");
    assert!(check(&p, &h, &[]).is_err());
}

#[test]
fn journal_restores_original_bytes_and_rejects_concurrent_edits() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    put(h, ".claude/settings.json", "{\"model\":\"keep\"}");
    let r = inspect(&p, h, &[]).unwrap();
    let journal = migrate(&p, h, &[], &r).unwrap();
    settings_reconcile::agent_permissions::cli(vec![
        "migrate".into(),
        "--home".into(),
        h.to_string_lossy().into(),
        "--restore".into(),
        journal.to_string_lossy().into(),
    ])
    .unwrap();
    assert_eq!(
        fs::read_to_string(h.join(".claude/settings.json")).unwrap(),
        "{\"model\":\"keep\"}"
    );
    assert!(!h.join(".codex/config.toml").exists());
    let r = inspect(&p, h, &[]).unwrap();
    let journal = migrate(&p, h, &[], &r).unwrap();
    put(h, ".claude/settings.json", "{\"model\":\"concurrent\"}");
    assert!(settings_reconcile::agent_permissions::cli(vec![
        "migrate".into(),
        "--home".into(),
        h.to_string_lossy().into(),
        "--restore".into(),
        journal.to_string_lossy().into()
    ])
    .is_err());
}

#[test]
fn local_restrictions_survive_and_reviewed_project_scope_is_remembered() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let project = d.path().join("project");
    fs::create_dir_all(&h).unwrap();
    put(
        &project,
        ".claude/settings.local.json",
        r#"{"sandbox":{"enabled":false,"filesystem":{"denyRead":["/private-data"],"denyWrite":["/private-write"],"allowWrite":["/"]},"network":{"deniedDomains":["blocked.example"]}},"permissions":{"allow":["Bash(*)"],"ask":["Bash(pkexec *)"],"deny":["Read(private)"]},"hooks":{"Stop":[]}}"#,
    );
    let p = policy();
    let r = inspect(&p, &h, std::slice::from_ref(&project)).unwrap();
    migrate(&p, &h, std::slice::from_ref(&project), &r).unwrap();
    check(&p, &h, &[]).unwrap();
    let v: Value =
        serde_json::from_slice(&fs::read(project.join(".claude/settings.local.json")).unwrap())
            .unwrap();
    assert_eq!(
        v["sandbox"]["filesystem"]["denyRead"],
        json!(["/private-data"])
    );
    assert_eq!(
        v["sandbox"]["filesystem"]["denyWrite"],
        json!(["/private-write"])
    );
    assert_eq!(
        v["sandbox"]["network"]["deniedDomains"],
        json!(["blocked.example"])
    );
    assert!(v["sandbox"].get("enabled").is_none());
    assert!(v["sandbox"]["filesystem"].get("allowWrite").is_none());
    assert_eq!(v["permissions"]["ask"], json!(["Bash(pkexec *)"]));
    assert_eq!(v["hooks"]["Stop"], json!([]));
}

#[test]
fn declared_secret_removal_retires_only_owned_denials_after_review() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let mut p = policy();
    put(
        h,
        ".claude/settings.json",
        r#"{"permissions":{"deny":["Read(custom)"]},"sandbox":{"filesystem":{"denyRead":["/keep-private"]}}}"#,
    );
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    p.secret_paths.retain(|s| s != "~/.ssh");
    assert!(apply(&p, h, &[]).is_err());
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    let v: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    let deny = v["permissions"]["deny"].as_array().unwrap();
    assert!(deny.contains(&json!("Read(custom)")));
    assert!(!deny.contains(&json!("Read(~/.ssh)")));
    let deny = v["sandbox"]["filesystem"]["denyRead"].as_array().unwrap();
    assert!(deny.contains(&json!("/keep-private")));
    assert!(!deny.contains(&json!("~/.ssh")));
}

#[test]
fn prompt_defaults_cover_new_servers_without_rewriting_transport_values() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    put(h,".codex/config.toml", "[mcp_servers.example]\ncommand='mcp'\n[mcp_servers.example.env]\napproval_mode='transport-value'\n[plugins.example.mcp_servers.nested]\n[apps.sample.tools.lookup]\napproval_mode='approve'\n[apps.sample.links.link1]\ndefault_tools_approval_mode='auto'\n");
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    let text = fs::read_to_string(h.join(".codex/config.toml")).unwrap();
    let v: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(
        v["mcp_servers"]["example"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        v["mcp_servers"]["example"]["env"]["approval_mode"].as_str(),
        Some("transport-value")
    );
    assert_eq!(
        v["plugins"]["example"]["mcp_servers"]["nested"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        v["apps"]["sample"]["tools"]["lookup"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        v["apps"]["sample"]["links"]["link1"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
}

#[test]
fn invalid_ownership_is_diagnosed_and_requires_digest_bound_retirement() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    put(h, ".local/state/dotfiles/agent-permissions.json", "{}");
    let r = inspect(&p, h, &[]).unwrap();
    assert!(r.ownership_problem.is_some());
    assert!(migrate(&p, h, &[], &r)
        .unwrap_err()
        .contains("--retire-ownership"));
    put(h, "policy.json", &serde_json::to_string(&p).unwrap());
    put(h, "review.json", &serde_json::to_string(&r).unwrap());
    let mut args = vec![
        "migrate".into(),
        "--home".into(),
        h.to_string_lossy().into(),
        "--policy".into(),
        h.join("policy.json").to_string_lossy().into(),
        "--approved".into(),
        h.join("review.json").to_string_lossy().into(),
        "--retire-ownership".into(),
        "--dry-run".into(),
    ];
    settings_reconcile::agent_permissions::cli(args.clone()).unwrap();
    assert_eq!(
        fs::read_to_string(h.join(".local/state/dotfiles/agent-permissions.json")).unwrap(),
        "{}"
    );
    args.pop();
    settings_reconcile::agent_permissions::cli(args).unwrap();
    check(&p, h, &[]).unwrap();
}
