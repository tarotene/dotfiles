use agent_settings_reconcile::agent_permissions::reclaim::{
    candidates, covered, drop_candidates, drop_covered, promotion_target, reclaim, render_note,
    sync_note, NOTE_REL,
};
use agent_settings_reconcile::agent_permissions::{apply, assess, check, inspect, migrate, Policy};
use serde_json::{json, Value};
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
fn github_connectivity_and_masking_are_generated_migrated_and_checked() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let mut p = policy();
    p.domains = vec!["api.github.com".into()];
    let g = p.generated();
    assert_eq!(
        g["claude"]["sandbox"]["network"]["allowedDomains"],
        json!(["api.github.com"])
    );
    assert_eq!(
        g["codex"]["permissions"]["dotfiles"]["network"]["domains"]["api.github.com"],
        "allow"
    );
    assert_eq!(g["codex"]["features"]["credential_masking"], true);
    put(h, ".codex/config.toml", "model='keep'\n[features]\ncredential_masking=false\nunrelated=true\n[profiles.other.features]\ncredential_masking=false\n");
    let r = inspect(&p, h, &[]).unwrap();
    assert!(r
        .sources
        .iter()
        .any(|s| s.grants.to_string().contains("credential_masking")));
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();
    let path = h.join(".codex/config.toml");
    let c = fs::read_to_string(&path).unwrap();
    assert!(c.contains("model='keep'"));
    assert!(c.contains("unrelated=true"));
    assert_eq!(c.matches("credential_masking").count(), 1);
    fs::write(
        &path,
        c.replace("credential_masking = true", "credential_masking = false"),
    )
    .unwrap();
    assert!(check(&p, h, &[]).is_err());
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();
    let c = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("{c}\n[profiles.drift.features]\ncredential_masking=false\n"),
    )
    .unwrap();
    assert!(check(&p, h, &[]).is_err());
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
        // 実機の vault に書かない(宣言は、テストが明示したときだけ)。
        .env_remove("OBSIDIAN_VAULT")
        .env_remove("XDG_CONFIG_HOME")
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
    assert!(c.contains("credential_masking = true"));
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

fn claude_auto_allow(h: &Path) -> Value {
    let c: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    c["sandbox"]["autoAllowBashIfSandboxed"].clone()
}

#[test]
fn a_policy_without_the_field_was_reviewed_under_prompt_and_unknown_values_are_refused() {
    // The fixture predates `sandboxed_shell`, like every record written before it existed.
    let old = policy();
    assert_eq!(old.sandboxed_shell, "prompt");
    assert_eq!(
        old.generated()["claude"]["sandbox"]["autoAllowBashIfSandboxed"],
        false
    );
    let mut auto = policy();
    auto.sandboxed_shell = "auto".into();
    auto.validate().unwrap();
    assert_eq!(
        auto.generated()["claude"]["sandbox"]["autoAllowBashIfSandboxed"],
        true
    );
    auto.sandboxed_shell = "yes".into();
    assert!(auto.validate().unwrap_err().contains("sandboxed_shell"));
}

#[test]
fn sandboxed_shell_auto_is_an_expansion_that_needs_review_and_prompt_narrows_back() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    put(h, ".claude/settings.json", "{}");
    let mut p = policy();
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    assert_eq!(claude_auto_allow(h), false);

    p.sandboxed_shell = "auto".into();
    assert!(apply(&p, h, &[]).unwrap_err().contains("expansion"));
    assert_eq!(
        claude_auto_allow(h),
        false,
        "a refused apply writes nothing"
    );
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();
    assert_eq!(claude_auto_allow(h), true);

    p.sandboxed_shell = "prompt".into();
    apply(&p, h, &[]).unwrap();
    check(&p, h, &[]).unwrap();
    assert_eq!(claude_auto_allow(h), false);
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
    agent_settings_reconcile::agent_permissions::cli(vec![
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
    assert!(agent_settings_reconcile::agent_permissions::cli(vec![
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
    agent_settings_reconcile::agent_permissions::cli(args.clone()).unwrap();
    assert_eq!(
        fs::read_to_string(h.join(".local/state/dotfiles/agent-permissions.json")).unwrap(),
        "{}"
    );
    args.pop();
    agent_settings_reconcile::agent_permissions::cli(args).unwrap();
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
            r#"{"permissions":{"allow":["Edit(/x)"]}}"#,
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

/// A reviewed home with one project whose local settings exist and are owned.
fn reviewed_project() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let project = d.path().join("project");
    fs::create_dir_all(&h).unwrap();
    put(&project, ".claude/settings.local.json", "{}");
    let p = policy();
    let projects = vec![project.clone()];
    let r = inspect(&p, &h, &projects).unwrap();
    migrate(&p, &h, &projects, &r).unwrap();
    check(&p, &h, &projects).unwrap();
    (d, h, project)
}

fn local(project: &Path) -> std::path::PathBuf {
    project.join(".claude/settings.local.json")
}

#[test]
fn tolerated_allow_rules_are_pending_and_the_strict_check_still_rejects_them() {
    let (_d, h, project) = reviewed_project();
    let p = policy();
    let projects = vec![project.clone()];
    let rules = [
        "WebFetch(domain:example.com)",
        "mcp__srv__tool",
        "WebSearch",
        "Bash(git status:*)",
    ];
    fs::write(
        local(&project),
        json!({"permissions": {"allow": rules}}).to_string(),
    )
    .unwrap();
    let waiting = assess(&p, &h, &projects).unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].path, local(&project).canonicalize().unwrap());
    assert_eq!(waiting[0].rules, rules);
    let strict = check(&p, &h, &projects).unwrap_err();
    assert!(strict.contains("pending permission grants"), "{strict}");
    assert!(strict.contains("WebFetch(domain:example.com)"), "{strict}");
    // The ordinary apply refuses to proceed over unreclaimed rules.
    assert!(apply(&p, &h, &projects).is_err());
}

#[test]
fn rules_outside_the_vocabulary_or_other_changes_stay_drift() {
    let (_d, h, project) = reviewed_project();
    let p = policy();
    let projects = vec![project.clone()];
    for body in [
        json!({"permissions": {"allow": ["Edit(/etc)"]}}),
        json!({"permissions": {"allow": ["WebSearch", "Write(/x)"]}}),
        json!({"permissions": {"allow": ["Bash(*)"]}}),
        json!({"permissions": {"allow": ["Bash(ps -p * -o pid)"]}}),
        json!({"permissions": {"allow": ["WebFetch(domain:*.example.com)"]}}),
        json!({"permissions": {"allow": ["WebSearch"], "deny": ["Read(x)"]}}),
        json!({"permissions": {"allow": ["WebSearch"]}, "sandbox": {"enabled": false}}),
    ] {
        fs::write(local(&project), body.to_string()).unwrap();
        let report = assess(&p, &h, &projects).unwrap_err();
        assert!(report.contains("permission drift"), "{body}: {report}");
    }
}

#[test]
fn a_first_local_settings_file_with_pending_rules_is_not_a_new_source() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let project = d.path().join("project");
    fs::create_dir_all(&h).unwrap();
    fs::create_dir_all(&project).unwrap();
    let p = policy();
    let projects = vec![project.clone()];
    let r = inspect(&p, &h, &projects).unwrap();
    migrate(&p, &h, &projects, &r).unwrap();
    // Claude Code creates the file the first time an approval is saved.
    fs::create_dir_all(project.join(".claude")).unwrap();
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebSearch"]}}"#,
    )
    .unwrap();
    assert_eq!(assess(&p, &h, &projects).unwrap()[0].rules, ["WebSearch"]);
    // An empty file is nothing to report; a restriction-free blank is not drift.
    fs::write(local(&project), r#"{"permissions":{"allow":[]}}"#).unwrap();
    assert!(assess(&p, &h, &projects).unwrap().is_empty());
    // Other content in a never-reviewed file is still a new permission source.
    fs::write(local(&project), r#"{"sandbox":{"enabled":false}}"#).unwrap();
    assert!(assess(&p, &h, &projects)
        .unwrap_err()
        .contains("new permission source"));
}

#[test]
fn empty_mcp_lists_are_not_grants_even_in_records_that_contain_them() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let project = d.path().join("project");
    fs::create_dir_all(&h).unwrap();
    fs::create_dir_all(&project).unwrap();
    let key = project.to_str().unwrap();
    put(
        &h,
        ".claude.json",
        &json!({"projects": {key: {"disabledMcpjsonServers": []}}}).to_string(),
    );
    let p = policy();
    let projects = vec![project.clone()];
    let r = inspect(&p, &h, &projects).unwrap();
    migrate(&p, &h, &projects, &r).unwrap();
    // A record written by the earlier inventory tracked the empty list.
    let record_path = h.join(".local/state/dotfiles/agent-permissions.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    for source in record["sources"].as_array_mut().unwrap() {
        if source["kind"] == "claude-state" {
            source["grants"] = json!({"projects": {key: {"disabledMcpjsonServers": []}}});
        }
    }
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(assess(&p, &h, &projects).unwrap().is_empty());
    // Starting Claude in another directory adds another empty entry.
    let state = fs::read_to_string(h.join(".claude.json")).unwrap();
    let mut state: Value = serde_json::from_str(&state).unwrap();
    state["projects"][project.join("sub").to_str().unwrap()] =
        json!({"disabledMcpjsonServers": [], "enabledMcpjsonServers": []});
    fs::write(h.join(".claude.json"), state.to_string()).unwrap();
    assert!(assess(&p, &h, &projects).unwrap().is_empty());
    // A server that is actually enabled is a grant.
    state["projects"][key]["enabledMcpjsonServers"] = json!(["srv"]);
    fs::write(h.join(".claude.json"), state.to_string()).unwrap();
    assert!(assess(&p, &h, &projects).is_err());
}

#[test]
fn the_check_command_reports_pending_rules_without_failing() {
    let (d, h, project) = reviewed_project();
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebSearch"]}}"#,
    )
    .unwrap();
    let policy_file = d.path().join("policy.json");
    fs::write(&policy_file, serde_json::to_vec(&policy()).unwrap()).unwrap();
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_agent-permissions"))
            .args(["check", "--policy"])
            .arg(&policy_file)
            .arg("--home")
            .arg(&h)
            .arg("--project")
            .arg(&project)
            .args(extra)
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            // 実機の vault に書かない(宣言は、テストが明示したときだけ)。
            .env_remove("OBSIDIAN_VAULT")
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap()
    };
    let out = run(&[]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("pending grants awaiting reclaim"), "{text}");
    let out = run(&["--json"]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pending"][0]["rules"], json!(["WebSearch"]));
}

#[test]
fn reclaim_queues_pending_rules_then_clears_the_file_and_keeps_the_rest() {
    let (_d, h, project) = reviewed_project();
    let p = policy();
    let projects = vec![project.clone()];
    fs::write(
        local(&project),
        json!({"model": "keep", "permissions": {"allow": ["WebFetch(domain:example.com)", "Bash(ls)"]}})
            .to_string(),
    )
    .unwrap();
    let done = reclaim(&p, &h, &projects).unwrap();
    assert_eq!((done.queued, done.removed, done.files), (2, 2, 1));
    let v: Value = serde_json::from_slice(&fs::read(local(&project)).unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"], json!([]));
    assert_eq!(v["model"], "keep", "fields that are not permissions stay");
    check(&p, &h, &projects).unwrap();
    let queue = candidates(&h).unwrap();
    let rules: Vec<_> = queue.iter().map(|c| c.rule.as_str()).collect();
    assert_eq!(rules.len(), 2);
    assert!(rules.contains(&"WebFetch(domain:example.com)") && rules.contains(&"Bash(ls)"));
    assert!(queue
        .iter()
        .all(|c| c.project == project.canonicalize().unwrap()));
    // Nothing left: a second run is a no-op.
    assert_eq!(reclaim(&p, &h, &projects).unwrap().removed, 0);
    // The same approval again is removed again but not queued twice.
    fs::write(local(&project), r#"{"permissions":{"allow":["Bash(ls)"]}}"#).unwrap();
    let again = reclaim(&p, &h, &projects).unwrap();
    assert_eq!((again.queued, again.removed), (0, 1));
    assert_eq!(candidates(&h).unwrap().len(), 2);
}

#[test]
fn withdrawn_candidates_leave_the_queue_and_can_be_queued_again() {
    let (_d, h, project) = reviewed_project();
    let p = policy();
    let projects = vec![project.clone()];
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebSearch"]}}"#,
    )
    .unwrap();
    reclaim(&p, &h, &projects).unwrap();
    let id = candidates(&h).unwrap()[0].id.clone();
    assert!(drop_candidates(&h, &["nope".into()]).is_err());
    assert_eq!(drop_candidates(&h, std::slice::from_ref(&id)).unwrap(), 1);
    assert!(candidates(&h).unwrap().is_empty());
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebSearch"]}}"#,
    )
    .unwrap();
    assert_eq!(reclaim(&p, &h, &projects).unwrap().queued, 1);
    assert_eq!(candidates(&h).unwrap()[0].id, id);
}

#[test]
fn a_structural_drift_is_not_reclaimed() {
    let (_d, h, project) = reviewed_project();
    let p = policy();
    let projects = vec![project.clone()];
    let body = r#"{"permissions":{"allow":["WebSearch","Edit(/etc)"]}}"#;
    fs::write(local(&project), body).unwrap();
    assert!(reclaim(&p, &h, &projects).is_err());
    assert_eq!(fs::read_to_string(local(&project)).unwrap(), body);
    assert!(candidates(&h).unwrap().is_empty());
}

#[test]
fn session_hooks_reclaim_and_tell_the_session_only_what_it_needs() {
    let (d, h, project) = reviewed_project();
    fs::create_dir_all(h.join(".config/agent-permissions")).unwrap();
    fs::write(
        h.join(".config/agent-permissions/policy.json"),
        serde_json::to_vec(&policy()).unwrap(),
    )
    .unwrap();
    let run = |args: &[&str]| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_agent-permissions"))
            .args(args)
            .env("HOME", &h)
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            // 実機の vault に書かない(宣言は、テストが明示したときだけ)。
            .env_remove("OBSIDIAN_VAULT")
            .env_remove("XDG_CONFIG_HOME")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(json!({"cwd": project}).to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let rule = r#"{"permissions":{"allow":["WebFetch(domain:example.com)"]}}"#;
    // The end of a session reclaims quietly.
    fs::write(local(&project), rule).unwrap();
    assert_eq!(run(&["hook", "session-end"]), "");
    assert!(fs::read_to_string(local(&project))
        .unwrap()
        .contains("\"allow\": []"));
    assert_eq!(candidates(&h).unwrap().len(), 1);
    // The start of the next one reports the queue.
    let start = run(&["hook", "session-start"]);
    assert!(!start.contains("<hook-directive"), "{start}");
    assert!(start.starts_with("[agent-permissions] "), "{start}");
    assert!(start.contains("1 件"), "{start}");
    assert!(start.contains("/promote-permissions"), "{start}");
    // Rules left behind by a crashed session are reclaimed and reported.
    fs::write(local(&project), rule).unwrap();
    let start = run(&["hook", "session-start"]);
    assert!(start.contains("恒久の許可 1 件を取り除き"), "{start}");
    // A structural drift is described, not hidden, and still never fails the hook.
    fs::write(local(&project), r#"{"permissions":{"allow":["Edit(/x)"]}}"#).unwrap();
    let start = run(&["hook", "session-start"]);
    assert!(start.contains("権限の記録とずれています"), "{start}");
    assert!(start.contains("permission drift"), "{start}");
    // Without a deployed policy there is nothing to do.
    fs::remove_file(h.join(".config/agent-permissions/policy.json")).unwrap();
    assert_eq!(run(&["hook", "session-start"]), "");
    drop(d);
}

#[test]
fn the_pending_command_lists_and_withdraws() {
    let (d, h, project) = reviewed_project();
    let p = policy();
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebSearch"]}}"#,
    )
    .unwrap();
    reclaim(&p, &h, std::slice::from_ref(&project)).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_agent-permissions"))
            .args(args)
            .arg("--home")
            .arg(&h)
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            // 実機の vault に書かない(宣言は、テストが明示したときだけ)。
            .env_remove("OBSIDIAN_VAULT")
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap()
    };
    let listed = run(&["pending", "--json"]);
    assert!(listed.status.success());
    let v: Value = serde_json::from_slice(&listed.stdout).unwrap();
    let id = v["candidates"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(v["candidates"][0]["rule"], "WebSearch");
    let text = String::from_utf8_lossy(&run(&["pending"]).stdout).into_owned();
    assert!(text.contains(&id) && text.contains("WebSearch"), "{text}");
    let dropped = run(&["pending", "--drop", &id]);
    assert!(dropped.status.success());
    assert!(String::from_utf8_lossy(&run(&["pending"]).stdout).contains("no pending"));
    assert!(!run(&["pending", "--drop", "missing"]).status.success());
    drop(d);
}

const CODEX_PREFIX: &str = r#"prefix_rule(pattern=["ls"], decision="allow")"#;
const CODEX_HOST: &str = r#"network_rule(host="example.com", protocol="https", decision="allow")"#;

#[test]
fn codex_rules_appended_by_an_approval_are_reclaimed_and_other_content_is_drift() {
    for existed in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        let p = policy();
        if existed {
            put(h, ".codex/rules/default.rules", "");
        }
        let r = inspect(&p, h, &[]).unwrap();
        migrate(&p, h, &[], &r).unwrap();
        check(&p, h, &[]).unwrap();
        // Codex appends one line per approval: a created file, or a grown one.
        put(
            h,
            ".codex/rules/default.rules",
            &format!("{CODEX_PREFIX}\n{CODEX_HOST}\n"),
        );
        let waiting = assess(&p, h, &[]).unwrap();
        assert_eq!(waiting.len(), 1, "existed={existed}");
        assert_eq!(waiting[0].rules, [CODEX_PREFIX, CODEX_HOST]);
        let done = reclaim(&p, h, &[]).unwrap();
        assert_eq!((done.queued, done.removed, done.files), (2, 2, 1));
        assert_eq!(
            fs::read_to_string(h.join(".codex/rules/default.rules")).unwrap(),
            ""
        );
        check(&p, h, &[]).unwrap();
        let queued: Vec<_> = candidates(h).unwrap().into_iter().map(|c| c.rule).collect();
        assert!(
            queued.contains(&CODEX_PREFIX.to_owned()) && queued.contains(&CODEX_HOST.to_owned())
        );
        // Executable Starlark, a wildcard host or a deny is not what Codex writes for
        // an approval: the file stays as it is and the drift is reported.
        for body in [
            format!("{CODEX_PREFIX}\nx = 1\n"),
            r#"network_rule(host="*.example.com", protocol="https", decision="allow")"#.to_owned(),
            r#"prefix_rule(pattern=["ls"], decision="prompt")"#.to_owned(),
        ] {
            put(h, ".codex/rules/default.rules", &body);
            let report = assess(&p, h, &[]).unwrap_err();
            assert!(
                report.contains("permission drift") || report.contains("new permission source"),
                "{body}: {report}"
            );
            assert!(reclaim(&p, h, &[]).is_err());
            assert_eq!(
                fs::read_to_string(h.join(".codex/rules/default.rules")).unwrap(),
                body
            );
        }
    }
}

#[test]
fn codex_tool_approvals_are_reverted_key_by_key_without_touching_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    let p = policy();
    put(
        h,
        ".codex/config.toml",
        "# keep me\nmodel = 'keep'\n[mcp_servers.srv]\ncommand = 'mcp'\n[mcp_servers.srv.tools.seen]\napproval_mode = 'prompt'\n[hooks.state.example]\ntrusted = true\n",
    );
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();
    // "Always allow" for an existing tool and for a tool seen for the first time.
    let config = fs::read_to_string(h.join(".codex/config.toml")).unwrap();
    // Only that tool's table: "default_tools_approval_mode" contains the same words.
    let approved = config.replace(
        "[mcp_servers.srv.tools.seen]\napproval_mode = \"prompt\"",
        "[mcp_servers.srv.tools.seen]\napproval_mode = \"approve\"",
    ) + "\n[mcp_servers.srv.tools.fresh]\napproval_mode = \"approve\"\n";
    put(h, ".codex/config.toml", &approved);
    let waiting = assess(&p, h, &[]).unwrap();
    let mut rules = waiting[0].rules.clone();
    rules.sort();
    assert_eq!(
        rules,
        [
            "mcp_servers.srv.tools.fresh.approval_mode=approve",
            "mcp_servers.srv.tools.seen.approval_mode=approve"
        ]
    );
    let done = reclaim(&p, h, &[]).unwrap();
    assert_eq!((done.queued, done.removed), (2, 2));
    let after = fs::read_to_string(h.join(".codex/config.toml")).unwrap();
    let doc: toml_edit::DocumentMut = after.parse().unwrap();
    assert_eq!(
        doc["mcp_servers"]["srv"]["tools"]["seen"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert!(
        doc["mcp_servers"]["srv"]["tools"].get("fresh").is_none(),
        "{after}"
    );
    assert!(after.contains("# keep me") && after.contains("model = 'keep'"));
    assert!(after.contains("command = 'mcp'") && after.contains("trusted = true"));
    check(&p, h, &[]).unwrap();
    // A new default for every tool of a server is not a single tool's approval.
    put(
        h,
        ".codex/config.toml",
        &after.replace(
            "command = 'mcp'\ndefault_tools_approval_mode = \"prompt\"",
            "command = 'mcp'\ndefault_tools_approval_mode = \"approve\"",
        ),
    );
    assert!(assess(&p, h, &[]).unwrap_err().contains("permission drift"));
}

/// #1055: one unreadable file is one `review` source, not a failure of the inventory.
#[test]
fn an_unreadable_settings_file_is_reported_as_one_review_source() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path().join("home");
    let broken = d.path().join("broken");
    let fine = d.path().join("fine");
    fs::create_dir_all(&h).unwrap();
    put(&broken, ".claude/settings.json", "");
    put(&broken, ".claude/settings.local.json", "{not json");
    put(&fine, ".claude/settings.local.json", "{}");
    let p = policy();
    let projects = vec![broken.clone(), fine.clone()];
    let review = inspect(&p, &h, &projects).unwrap();
    let reviewed: Vec<_> = review
        .decisions
        .iter()
        .filter(|(_, d)| *d == "review")
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(reviewed.len(), 2, "{:?}", review.decisions);
    for path in &reviewed {
        let source = review.sources.iter().find(|s| s.path == *path).unwrap();
        assert!(source.unknown[0].starts_with("unreadable configuration:"));
        assert!(path.starts_with(broken.canonicalize().unwrap()));
    }
    // The readable source next to them is still inventoried, and can be managed.
    let fine_local = fine
        .join(".claude/settings.local.json")
        .canonicalize()
        .unwrap();
    assert_eq!(review.decisions[&fine_local], "manage");
    // Nothing is migrated over an unreadable file; the owner fixes or removes it.
    let err = migrate(&p, &h, &projects, &review).unwrap_err();
    assert!(err.contains("requires review"), "{err}");
}

fn with_mcp_allow(tools: &[&str]) -> Policy {
    let mut p = policy();
    p.mcp_allow = tools.iter().map(|t| (*t).to_owned()).collect();
    p
}

#[test]
fn mcp_allow_lists_single_tools_only_and_stays_out_of_an_old_shaped_record() {
    with_mcp_allow(&["mcp__esa__esa_get_post", "mcp__plugin_a_b__tool__x"])
        .validate()
        .unwrap();
    for bad in [
        "mcp__esa__*",
        "mcp__*",
        "mcp__esa",
        "mcp__esa__",
        "mcp____tool",
        "Bash(git status:*)",
        "mcp__es a__x",
    ] {
        let err = with_mcp_allow(&[bad]).validate().unwrap_err();
        assert!(err.contains("mcp_allow"), "{bad}: {err}");
    }
    let twice = with_mcp_allow(&["mcp__a__b", "mcp__a__b"]);
    assert!(twice.validate().unwrap_err().contains("twice"));
    // An empty list is not serialized: a record without the field is the same policy,
    // and an older binary can still read what this one writes.
    assert!(!serde_json::to_string(&policy())
        .unwrap()
        .contains("mcp_allow"));
    assert!(serde_json::to_string(&with_mcp_allow(&["mcp__a__b"]))
        .unwrap()
        .contains("mcp_allow"));
}

#[test]
fn mcp_allow_maps_one_to_one_to_claude_allow_and_codex_tool_approval() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    put(
        h,
        ".claude/settings.json",
        r#"{"permissions":{"allow":["mcp__human__added"]}}"#,
    );
    put(
        h,
        ".codex/config.toml",
        "[mcp_servers.esa]\ncommand = 'esa'\n[mcp_servers.esa.tools.esa_get_post]\napproval_mode = 'prompt'\n[mcp_servers.other]\ncommand = 'o'\n[mcp_servers.other.tools.kept]\napproval_mode = 'approve'\n",
    );
    let p = with_mcp_allow(&["mcp__esa__esa_get_post", "mcp__claude_ai_Drive__read"]);
    let r = inspect(&p, h, &[]).unwrap();
    assert_eq!(
        r.generated["claude"]["permissions"]["allow"],
        json!(["mcp__esa__esa_get_post", "mcp__claude_ai_Drive__read"])
    );
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();
    let claude: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(
        claude["permissions"]["allow"],
        json!(["mcp__esa__esa_get_post", "mcp__claude_ai_Drive__read"]),
        "a human-added allow is replaced by the declaration"
    );
    assert_eq!(
        approval(h, "esa", "esa_get_post").as_deref(),
        Some("approve")
    );
    assert_eq!(
        approval(h, "other", "kept").as_deref(),
        Some("prompt"),
        "an undeclared tool goes back to prompt"
    );
    assert!(
        !has_server(h, "claude_ai_Drive"),
        "a server the config does not define is never created: Codex rejects it as an invalid transport"
    );
}

/// The `approval_mode` under `mcp_servers.<server>.tools.<tool>` of the Codex config.
fn approval(home: &Path, server: &str, tool: &str) -> Option<String> {
    let doc: toml_edit::DocumentMut = fs::read_to_string(home.join(".codex/config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    doc["mcp_servers"][server]["tools"][tool]["approval_mode"]
        .as_str()
        .map(str::to_owned)
}

fn has_server(home: &Path, server: &str) -> bool {
    let doc: toml_edit::DocumentMut = fs::read_to_string(home.join(".codex/config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    doc["mcp_servers"].get(server).is_some()
}

#[test]
fn adding_an_mcp_tool_is_an_expansion_and_removing_one_narrows_back_to_prompt() {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    put(h, ".claude/settings.json", "{}");
    put(
        h,
        ".codex/config.toml",
        "[mcp_servers.esa]\ncommand = 'esa'\n",
    );
    let mut p = with_mcp_allow(&["mcp__esa__a"]);
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();

    p.mcp_allow.push("mcp__esa__b".into());
    assert!(apply(&p, h, &[]).unwrap_err().contains("expansion"));
    let after_refusal: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(
        after_refusal["permissions"]["allow"],
        json!(["mcp__esa__a"])
    );
    let r = inspect(&p, h, &[]).unwrap();
    migrate(&p, h, &[], &r).unwrap();
    check(&p, h, &[]).unwrap();

    p.mcp_allow.retain(|t| t != "mcp__esa__a");
    apply(&p, h, &[]).unwrap();
    check(&p, h, &[]).unwrap();
    let claude: Value =
        serde_json::from_slice(&fs::read(h.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(claude["permissions"]["allow"], json!(["mcp__esa__b"]));
    assert_eq!(approval(h, "esa", "a").as_deref(), Some("prompt"));
    assert_eq!(approval(h, "esa", "b").as_deref(), Some("approve"));
}

// ---------------------------------------------------------------- 共有ポリシーにある規則の取り下げ

fn granting_policy() -> Policy {
    serde_json::from_value(json!({"version":1,"writable_roots":["workspace","temp"],
        "secret_paths":["~/.ssh"],"domains":["a.example"],"tools":"prompt",
        "mcp_allow":["mcp__srv__tool"],"projects":[],
        "project_rules":{"/work/proj":{"domains":["p.example"]}}}))
    .unwrap()
}

#[test]
fn covered_follows_the_permission_hooks_own_test() {
    let p = granting_policy();
    let any = Path::new("/elsewhere");
    let in_proj = Path::new("/work/proj/sub");
    // 通信先: 全体の domains、または、その場所を含む project_rules の domains。
    assert!(covered(&p, any, "WebFetch(domain:a.example)"));
    assert!(covered(&p, in_proj, "WebFetch(domain:p.example)"));
    assert!(!covered(&p, any, "WebFetch(domain:p.example)"));
    assert!(!covered(&p, in_proj, "WebFetch(domain:other.example)"));
    // Codex の network_rule は、同じ通信先の宣言で足りる。
    assert!(covered(
        &p,
        any,
        r#"network_rule(host="a.example", protocol="https", decision="allow")"#
    ));
    // MCP ツール: mcp_allow に 1 つずつ。Codex のツールの approval_mode も、同じ宣言に写る。
    assert!(covered(&p, any, "mcp__srv__tool"));
    assert!(covered(
        &p,
        any,
        "mcp_servers.srv.tools.tool.approval_mode=approve"
    ));
    assert!(!covered(&p, any, "mcp__srv__other"));
    assert!(!covered(
        &p,
        any,
        "mcp_servers.srv.tools.other.approval_mode=approve"
    ));
    assert!(!covered(&p, any, "apps.app.tools.t.approval_mode=approve"));
    // サーバ全体(`mcp__srv`)・WebSearch・Bash・prefix_rule には、宣言が無い。
    for rule in [
        "mcp__srv",
        "WebSearch",
        "Bash(ls)",
        r#"prefix_rule(pattern=["ls"], decision="allow")"#,
    ] {
        assert!(!covered(&p, any, rule), "{rule}");
    }
}

#[test]
fn promotion_targets_split_into_declarable_and_discard() {
    assert!(promotion_target("WebFetch(domain:x.example)").contains("domains"));
    assert!(promotion_target(
        r#"network_rule(host="x.example", protocol="https", decision="allow")"#
    )
    .contains("domains"));
    assert!(promotion_target("mcp__srv__tool").contains("mcp_allow"));
    assert!(
        promotion_target("mcp_servers.srv.tools.t.approval_mode=approve").contains("mcp_allow")
    );
    for rule in [
        "WebSearch",
        "Bash(ls)",
        "mcp__srv",
        "apps.a.tools.t.approval_mode=approve",
    ] {
        assert!(promotion_target(rule).contains("宣言なし"), "{rule}");
    }
}

#[test]
fn a_covered_rule_is_removed_from_the_file_but_never_queued() {
    let (_d, h, project) = reviewed_project();
    let p = policy(); // domains: ["github.com"]
    let projects = vec![project.clone()];
    fs::write(
        local(&project),
        json!({"permissions": {"allow": ["WebFetch(domain:github.com)", "WebSearch"]}}).to_string(),
    )
    .unwrap();
    let done = reclaim(&p, &h, &projects).unwrap();
    // どちらも設定ファイルから取り除くが、キューに積むのは、本人の裁定が要る方だけ。
    assert_eq!((done.queued, done.removed, done.dropped), (1, 2, 0));
    let rules: Vec<_> = candidates(&h)
        .unwrap()
        .into_iter()
        .map(|c| c.rule)
        .collect();
    assert_eq!(rules, ["WebSearch"]);
    check(&p, &h, &projects).unwrap();
}

#[test]
fn a_queue_filled_before_a_rule_joined_the_policy_is_tidied_with_nothing_waiting() {
    let (_d, h, project) = reviewed_project();
    let projects = vec![project.clone()];
    let file = local(&project).canonicalize().unwrap();
    let line = |rule: &str| {
        json!({"v": 1, "ts": 1_793_059_200u64, "rule": rule, "file": file}).to_string()
    };
    fs::create_dir_all(h.join(".local/state/dotfiles")).unwrap();
    fs::write(
        h.join(".local/state/dotfiles/agent-permissions-pending.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            line("WebFetch(domain:github.com)"),
            line("WebFetch(domain:later.example)"),
            line("WebSearch")
        ),
    )
    .unwrap();
    assert_eq!(candidates(&h).unwrap().len(), 3);
    // 設定ファイルに待機の許可は無い。それでも、既に許可済みのものは取り下げる。
    let done = reclaim(&policy(), &h, &projects).unwrap();
    assert_eq!((done.queued, done.removed, done.dropped), (0, 0, 1));
    let mut rules: Vec<_> = candidates(&h)
        .unwrap()
        .into_iter()
        .map(|c| c.rule)
        .collect();
    rules.sort();
    assert_eq!(rules, ["WebFetch(domain:later.example)", "WebSearch"]);
    // 冪等。
    assert_eq!(drop_covered(&policy(), &h).unwrap(), 0);
    // 方針が広がったら(審査済みの記録に入ったら)、次の回収で、残りも取り下がる。
    let mut wider = policy();
    wider.domains.push("later.example".into());
    let review = inspect(&wider, &h, &projects).unwrap();
    migrate(&wider, &h, &projects, &review).unwrap();
    assert_eq!(reclaim(&wider, &h, &projects).unwrap().dropped, 1);
    assert_eq!(candidates(&h).unwrap().len(), 1);
}

// ---------------------------------------------------------------- vault の裁定ノート

fn vault_at(d: &Path) -> vault_note::VaultNotes {
    vault_note::VaultNotes::at(&d.join("vault"), &d.join("lock"))
}

#[test]
fn the_decision_note_mirrors_the_queue_and_disappears_when_it_is_empty() {
    let (d, h, project) = reviewed_project();
    let projects = vec![project.clone()];
    let vault = vault_at(d.path());
    // 空のキューでは、ノートを作らない。
    assert!(!sync_note(&h, &vault).unwrap());
    assert!(vault.lock().unwrap().read(NOTE_REL).unwrap().is_none());

    fs::write(
        local(&project),
        json!({"permissions": {"allow": ["WebFetch(domain:later.example)", "mcp__srv__tool", "WebSearch"]}}).to_string(),
    )
    .unwrap();
    reclaim(&policy(), &h, &projects).unwrap();
    assert!(sync_note(&h, &vault).unwrap());
    let text = vault.lock().unwrap().read(NOTE_REL).unwrap().unwrap();
    assert!(text.starts_with("---\ncount: \"3\"\n---\n"), "{text}");
    assert!(text.contains("/promote-permissions"), "{text}");
    for c in candidates(&h).unwrap() {
        assert!(text.contains(&format!("`{}`", c.id)), "{text}");
        assert!(text.contains(&format!("`{}`", c.rule)), "{text}");
    }
    assert!(text.contains("| 宣言なし(捨てる) |"), "{text}");
    assert!(text.contains("mcp_allow(拡大"), "{text}");
    // プロジェクトは `$HOME` 配下でないので、そのまま書く。
    assert!(
        text.contains(&project.canonicalize().unwrap().display().to_string()),
        "{text}"
    );
    // 内容は、キューだけで決まる。もう一度写しても書き直さない。
    assert!(!sync_note(&h, &vault).unwrap());
    assert_eq!(render_note(&h, &candidates(&h).unwrap()), text);

    // 全件を取り下げると、ノートが消える。
    let ids: Vec<String> = candidates(&h).unwrap().into_iter().map(|c| c.id).collect();
    drop_candidates(&h, &ids).unwrap();
    assert!(sync_note(&h, &vault).unwrap());
    assert!(vault.lock().unwrap().read(NOTE_REL).unwrap().is_none());
    assert!(!sync_note(&h, &vault).unwrap());
}

#[test]
fn the_hooks_and_the_pending_command_keep_the_vault_note_in_step() {
    let (d, h, project) = reviewed_project();
    fs::create_dir_all(h.join(".config/agent-permissions")).unwrap();
    fs::write(
        h.join(".config/agent-permissions/policy.json"),
        serde_json::to_vec(&policy()).unwrap(),
    )
    .unwrap();
    let vault_dir = d.path().join("vault");
    fs::create_dir_all(vault_dir.join(".obsidian")).unwrap();
    fs::write(vault_dir.join(".obsidian/app.json"), "{}").unwrap();
    let note = vault_dir.join(NOTE_REL);
    let run = |args: &[&str], stdin: &str| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_agent-permissions"))
            .args(args)
            .env("HOME", &h)
            .env("WRAPUP_STATE_DIR", d.path().join("wrapup"))
            .env("OBSIDIAN_VAULT", &vault_dir)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let cwd = json!({"cwd": project}).to_string();
    fs::write(
        local(&project),
        r#"{"permissions":{"allow":["WebFetch(domain:later.example)"]}}"#,
    )
    .unwrap();
    assert_eq!(run(&["hook", "session-end"], &cwd), "");
    let text = fs::read_to_string(&note).unwrap();
    assert!(text.contains("`WebFetch(domain:later.example)`"), "{text}");

    // 注入文は、キューが記録の正本であることと、終わりに報告・起票しないことを言う。
    let start = run(&["hook", "session-start"], &cwd);
    assert!(start.contains("キューが記録の正本"), "{start}");
    assert!(
        start.contains("報告・起票・wrap-up inbox への追加はしない"),
        "{start}"
    );

    // 取り下げると、ノートも消える。
    let id = candidates(&h).unwrap()[0].id.clone();
    run(
        &["pending", "--drop", &id, "--home", h.to_str().unwrap()],
        "",
    );
    assert!(!note.exists());
}
