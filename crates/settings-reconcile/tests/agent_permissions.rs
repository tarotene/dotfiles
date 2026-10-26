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
fn missing_git_is_reported_as_an_executable_failure_not_a_missing_setting() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let project = d.path().join("project");
    let empty_bin = d.path().join("empty-bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&empty_bin).unwrap();
    // A non-empty .git directory triggers the tracked-file query. No git
    // process or inherited global git configuration is needed to arrange it.
    put(&project, ".git/HEAD", "ref: refs/heads/main\n");
    put(&project, ".claude/settings.local.json", "{}");
    let policy_file = d.path().join("policy.json");
    fs::write(&policy_file, serde_json::to_vec(&policy()).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-permissions"))
        .args(["inspect", "--policy"])
        .arg(&policy_file)
        .arg("--home")
        .arg(&home)
        .arg("--project")
        .arg(&project)
        .env("PATH", &empty_bin)
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("cannot run git to determine whether local settings are tracked"));
    assert_eq!(
        fs::read_to_string(project.join(".claude/settings.local.json")).unwrap(),
        "{}"
    );
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

#[test]
fn every_drifted_source_is_listed_in_one_report() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let (a, b) = (d.path().join("repo-a"), d.path().join("repo-b"));
    fs::create_dir_all(&h).unwrap();
    for repo in [&a, &b] {
        put(repo, ".claude/settings.local.json", "{}");
    }
    let p = policy();
    let projects = vec![a.clone(), b.clone()];
    let r = inspect(&p, &h, &projects).unwrap();
    migrate(&p, &h, &projects, &r).unwrap();
    check(&p, &h, &projects).unwrap();
    for repo in [&a, &b] {
        put(
            repo,
            ".claude/settings.local.json",
            r#"{"permissions":{"allow":["Bash(ls)"]}}"#,
        );
    }
    let report = check(&p, &h, &projects).unwrap_err();
    for repo in [&a, &b] {
        let line = format!(
            "permission drift: {}",
            repo.join(".claude/settings.local.json").display()
        );
        assert!(report.lines().any(|l| l == line), "{report}");
    }
    assert!(report.lines().last().unwrap().contains("inspect --json"));
}

#[test]
fn help_needs_no_policy_and_names_every_command() {
    let bin = env!("CARGO_BIN_EXE_agent-permissions");
    let d = tempfile::tempdir().unwrap();
    for args in [vec!["--help"], vec!["-h"], vec!["help"]] {
        let out = std::process::Command::new(bin)
            .args(&args)
            .env("HOME", d.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        for word in ["inspect", "check", "apply", "migrate", "--dry-run"] {
            assert!(text.contains(word), "{args:?} lacks {word}");
        }
    }
    let out = std::process::Command::new(bin)
        .args(["migrate", "--help"])
        .env("HOME", d.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("usage: agent-permissions migrate"));
    let out = std::process::Command::new(bin)
        .args(["nonsense", "--help"])
        .env("HOME", d.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn migration_journals_stay_small_and_old_ones_are_pruned() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    // A large file that every migration leaves unchanged.
    put(
        h,
        ".claude.json",
        &format!("{{\"padding\":\"{}\"}}", "x".repeat(200_000)),
    );
    put(h, ".claude/settings.json", "{\"model\":\"keep\"}");
    let mut last = None;
    for round in 0..8 {
        put(
            h,
            ".claude/settings.json",
            &format!("{{\"model\":\"m{round}\"}}"),
        );
        let r = inspect(&p, h, &[]).unwrap();
        last = Some(migrate(&p, h, &[], &r).unwrap());
    }
    let dir = last.unwrap().parent().unwrap().to_path_buf();
    let mut sizes: Vec<u64> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .map(|e| e.metadata().unwrap().len())
        .collect();
    sizes.sort();
    assert_eq!(sizes.len(), 5, "only the newest journals are kept");
    // The 200 KB unchanged file is not recorded, so a journal is a few KB.
    assert!(*sizes.last().unwrap() < 20_000, "{sizes:?}");
}
