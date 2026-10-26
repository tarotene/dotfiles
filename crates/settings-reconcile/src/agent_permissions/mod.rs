//! Shared permission policy, conservative inventory and reviewed migration.
//! Native sandboxes enforce command boundaries; no shell command parser lives here.
pub mod inventory;
pub mod pending;
pub mod reclaim;
mod transaction;

use inventory::{without_non_grants, Source};
use pending::{inert_call, tolerated_changes, tolerated_new_source, Pending};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectRule {
    pub domains: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub writable_roots: Vec<String>,
    pub secret_paths: Vec<String>,
    pub domains: Vec<String>,
    pub tools: String,
    pub projects: Vec<PathBuf>,
    pub project_rules: BTreeMap<String, ProjectRule>,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self, String> {
        let p: Self =
            serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.writable_roots != ["workspace", "temp"]
            || self.tools != "prompt"
        {
            return Err("unsupported policy: version=1, writable_roots=[workspace,temp], tools=prompt required".into());
        }
        for path in &self.secret_paths {
            if !(path.starts_with("~/") || Path::new(path).is_absolute())
                || path.contains(['*', '?', '\n', '(', ')', '[', ']', '{', '}', '\\'])
                || path.split('/').any(|p| p == "..")
            {
                return Err(format!("invalid secret path: {path}"));
            }
        }
        for domain in self
            .domains
            .iter()
            .chain(self.project_rules.values().flat_map(|p| &p.domains))
        {
            if domain.is_empty()
                || domain.contains(['/', ':', '*'])
                || !domain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return Err(format!("domain must be an exact hostname: {domain}"));
            }
        }
        for path in self
            .projects
            .iter()
            .map(PathBuf::as_path)
            .chain(self.project_rules.keys().map(Path::new))
        {
            if !path.is_absolute() {
                return Err(format!("project path must be absolute: {}", path.display()));
            }
        }
        Ok(())
    }

    pub fn hash(&self) -> String {
        digest(&serde_json::to_vec(self).expect("serializable policy"))
    }

    /// Native fields only. This does not produce any prefix allow rules.
    pub fn generated(&self) -> Value {
        let domains: BTreeMap<_, _> = self.domains.iter().map(|d| (d, "allow")).collect();
        let mut fs = serde_json::Map::new();
        fs.insert(":root".into(), json!("read"));
        for path in &self.secret_paths {
            fs.insert(path.clone(), json!("deny"));
        }
        json!({
            "claude": {
                "disableAllHooks": false,
                "permissions": {"allow": [], "additionalDirectories": [],
                    // Claude // is an absolute filesystem path; / is project-relative.
                    "deny": self.secret_paths.iter().flat_map(|p| {
                        let native = if p.starts_with('/') { format!("/{p}") } else { p.clone() };
                        [format!("Read({native})"), format!("Read({native}/**)")]
                    }).collect::<Vec<_>>()},
                "autoMode": {"classifyAllShell": true},
                "sandbox": {
                    "enabled": true, "failIfUnavailable": true,
                    "autoAllowBashIfSandboxed": false,
                    "allowUnsandboxedCommands": true, "excludedCommands": [],
                    "filesystem": {"allowWrite": [], "denyRead": self.secret_paths},
                    "network": {"allowedDomains": self.domains}
                }
            },
            "codex": {
                "approval_policy": "on-request", "approvals_reviewer": "auto_review",
                "default_permissions": "dotfiles", "features": {"network_proxy": true},
                "permissions": {"dotfiles": {"extends": ":workspace", "filesystem": fs,
                    "network": {"enabled": true, "domains": domains}}},
                "apps": {"_default": {"default_tools_approval_mode": "prompt"}}
            }
        })
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn state_path(home: &Path) -> PathBuf {
    home.join(".local/state/dotfiles/agent-permissions.json")
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub version: u32,
    pub policy: Policy,
    pub generated: Value,
    pub ownership_digest: Option<String>,
    pub ownership_problem: Option<String>,
    pub projects: Vec<PathBuf>,
    pub sources: Vec<Source>,
    /// review must be changed explicitly to remove/retain for opaque input.
    pub decisions: BTreeMap<PathBuf, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    policy: Policy,
    projects: Vec<PathBuf>,
    sources: Vec<Source>,
}

pub fn inspect(policy: &Policy, home: &Path, projects: &[PathBuf]) -> Result<Review, String> {
    let mut projects = projects.to_vec();
    let mut ownership_problem = None;
    if state_path(home).exists() {
        match load_record(home) {
            Ok(record) => projects.extend(record.projects),
            Err(e) => ownership_problem = Some(e),
        }
    }
    projects.sort();
    projects.dedup();
    let sources = inventory::discover(home, &all_projects(policy, &projects))?;
    let decisions = sources
        .iter()
        .map(|s| {
            let decision = if !s.unknown.is_empty() {
                "review"
            } else if !s.writable {
                "retain"
            } else if s.kind == "codex-rules" {
                "remove"
            } else {
                "manage"
            };
            (s.path.clone(), decision.into())
        })
        .collect();
    Ok(Review {
        version: 1,
        policy: policy.clone(),
        generated: policy.generated(),
        ownership_digest: match std::fs::read(state_path(home)) {
            Ok(bytes) => Some(digest(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        },
        projects,
        ownership_problem,
        sources,
        decisions,
    })
}

fn all_projects(policy: &Policy, projects: &[PathBuf]) -> Vec<PathBuf> {
    let mut p = policy.projects.clone();
    p.extend(policy.project_rules.keys().map(PathBuf::from));
    p.extend_from_slice(projects);
    p.sort();
    p.dedup();
    p
}

fn load_record(home: &Path) -> Result<Record, String> {
    let path = state_path(home);
    let r: Record = serde_json::from_slice(&read(&path)?).map_err(|e| {
        format!(
            "{}: {e}; run agent-permissions inspect, then reviewed migrate",
            path.display()
        )
    })?;
    if r.version != 1 {
        return Err("unsupported permissions ownership record".into());
    }
    r.policy.validate()?;
    Ok(r)
}

/// Compare the sources with the ownership record. A structural drift is an
/// error; tolerated allow rules added since the last review (see [`pending`])
/// are returned for reclaim instead of blocking.
pub fn assess(policy: &Policy, home: &Path, projects: &[PathBuf]) -> Result<Vec<Pending>, String> {
    transaction::check_pending(home)?;
    let record = load_record(home)?;
    if record.policy != *policy {
        return Err("policy changed; inspect and review before apply (permission expansion is never implicit)".into());
    }
    let current = inspect(policy, home, projects)?;
    // Every difference is listed, not just the first: one review should be enough.
    let mut problems = Vec::new();
    let mut waiting = Vec::new();
    // Hash whole shared files; compare only permission fields for mutable user configs.
    for s in &current.sources {
        let Some(old) = record.sources.iter().find(|o| o.path == s.path) else {
            match tolerated_new_source(s) {
                Some(t) if t.rules.is_empty() => {}
                Some(t) => waiting.push(Pending {
                    path: s.path.clone(),
                    rules: t.rules,
                    revert: t.revert,
                }),
                None => problems.push(format!("new permission source: {}", s.path.display())),
            }
            continue;
        };
        // Records written before empty lists and disabled servers were left out
        // of `grants` are compared in the current shape.
        let mut old = old.clone();
        old.grants = without_non_grants(&old.kind, &old.grants);
        if s.kind != old.kind
            || s.writable != old.writable
            || s.grants != old.grants
            || s.unknown != old.unknown
            || (!s.writable && s.digest != old.digest)
        {
            match tolerated_changes(&old, s) {
                Some(t) if t.rules.is_empty() => {}
                Some(t) => waiting.push(Pending {
                    path: s.path.clone(),
                    rules: t.rules,
                    revert: t.revert,
                }),
                None => problems.push(format!("permission drift: {}", s.path.display())),
            }
        }
    }
    for old in &record.sources {
        if !current.sources.iter().any(|s| s.path == old.path) {
            problems.push(format!("permission source removed: {}", old.path.display()));
        }
    }
    if problems.is_empty() {
        return Ok(waiting);
    }
    problems.push("run agent-permissions inspect --json, review then migrate".into());
    Err(problems.join("\n"))
}

/// Strict form of [`assess`]: pending allow rules are an error too. Used where
/// the result must equal the reviewed record, such as the read-back after a
/// migration and before an ordinary `apply`.
pub fn check(policy: &Policy, home: &Path, projects: &[PathBuf]) -> Result<(), String> {
    let waiting = assess(policy, home, projects)?;
    if waiting.is_empty() {
        return Ok(());
    }
    let mut lines = vec!["pending permission grants are not in the reviewed record:".to_owned()];
    for p in &waiting {
        lines.push(format!("{}: {}", p.path.display(), p.rules.join(", ")));
    }
    lines.push("reclaim them or run a reviewed migrate first".into());
    Err(lines.join("\n"))
}

/// Ordinary activation may narrow a reviewed policy, never expand one.
pub fn apply(policy: &Policy, home: &Path, projects: &[PathBuf]) -> Result<(), String> {
    let record = load_record(home)?;
    check(&record.policy, home, projects)?;
    if record.policy == *policy {
        return Ok(());
    }
    let old = &record.policy;
    if policy.domains.iter().any(|d| !old.domains.contains(d))
        || old
            .secret_paths
            .iter()
            .any(|p| !policy.secret_paths.contains(p))
        || policy.projects != old.projects
        || policy.project_rules.iter().any(|(p, r)| {
            old.project_rules
                .get(p)
                .is_none_or(|o| r.domains.iter().any(|d| !o.domains.contains(d)))
        })
    {
        return Err(
            "permission expansion or source scope change requires inspect and reviewed migrate"
                .into(),
        );
    }
    let review = inspect(policy, home, projects)?;
    migrate(policy, home, projects, &review)?;
    check(policy, home, projects)
}

fn retained_safe(source: &Source, policy: &Policy) -> Result<(), String> {
    let g = &source.grants;
    if source.kind == "codex-config" && !g.as_object().is_some_and(|o| o.is_empty()) {
        // Config layers above user config can override the selected permissions.
        return Err(format!("{}: shared Codex permission overrides require native managed enforcement; retained override is not safe", source.path.display()));
    }
    if g["disableAllHooks"] == true
        || g["allowManagedHooksOnly"] == true
        || g["autoMode"]["classifyAllShell"] == false
        || g["permissions"]["defaultMode"] == "bypassPermissions"
        || g["permissions"]["additionalDirectories"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    {
        return Err(format!(
            "{}: retained settings weaken the shared boundary",
            source.path.display()
        ));
    }
    if let Some(sandbox) = g.get("sandbox") {
        let desired = policy.generated();
        if !is_subset(sandbox, &desired["claude"]["sandbox"]) {
            return Err(format!(
                "{}: retained sandbox override differs from shared policy",
                source.path.display()
            ));
        }
    }
    Ok(())
}

fn is_subset(have: &Value, want: &Value) -> bool {
    match have.as_object() {
        Some(obj) => obj
            .iter()
            .all(|(k, v)| want.get(k).is_some_and(|w| is_subset(v, w))),
        None => have == want,
    }
}

/// Reviewed snapshot is re-read immediately before writing; no count is inherited.
struct Prepared {
    current: Review,
    writes: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

fn prepare_migration(
    policy: &Policy,
    home: &Path,
    projects: &[PathBuf],
    review: &Review,
    retire_ownership: bool,
) -> Result<Prepared, String> {
    transaction::check_pending(home)?;
    let current = inspect(policy, home, projects)?;
    if review.version != 1
        || review.policy != *policy
        || review.sources != current.sources
        || review.generated != current.generated
        || review.ownership_digest != current.ownership_digest
        || review.projects != current.projects
        || review.ownership_problem != current.ownership_problem
    {
        return Err("review is stale (policy or sources changed); inspect again".into());
    }
    if review.decisions.len() != current.sources.len() {
        return Err("review decisions incomplete".into());
    }
    let mut writes = BTreeMap::new();
    if current.ownership_problem.is_some() && !retire_ownership {
        return Err("ownership record is invalid; review inspect output then migrate --retire-ownership --approved REVIEW".into());
    }
    if retire_ownership && current.ownership_problem.is_none() {
        return Err("--retire-ownership only rebuilds an invalid, reviewed record".into());
    }
    let previous = if current.ownership_digest.is_some() && !retire_ownership {
        Some(load_record(home)?.policy)
    } else {
        None
    };
    for source in &current.sources {
        let action = review
            .decisions
            .get(&source.path)
            .map(String::as_str)
            .unwrap_or("review");
        match action {
            "retain" if !source.writable && source.unknown.is_empty() => { retained_safe(source, policy)?; },
            "remove" if source.writable && source.kind == "codex-rules" => {
                writes.insert(source.path.clone(), Some(Vec::new()));
            },
            "manage" if source.writable && source.unknown.is_empty() => {
                let data = render_source(source, policy, previous.as_ref())?;
                writes.insert(source.path.clone(), Some(data));
            },
            _ => return Err(format!("{}: disposition {action:?} requires review; unknown/shared settings are never silently rewritten", source.path.display())),
        }
    }
    // Create both user sources even on a fresh machine.
    let generated = policy.generated();
    let claude = home.join(".claude/settings.json");
    writes
        .entry(claude)
        .or_insert_with(|| Some(serde_json::to_vec_pretty(&generated["claude"]).unwrap()));
    let codex = home.join(".codex/config.toml");
    if let std::collections::btree_map::Entry::Vacant(entry) = writes.entry(codex) {
        let mut doc = toml_edit::DocumentMut::new();
        merge_toml(doc.as_table_mut(), &generated["codex"])?;
        entry.insert(Some(doc.to_string().into_bytes()));
    }
    Ok(Prepared { current, writes })
}

pub fn migrate(
    policy: &Policy,
    home: &Path,
    projects: &[PathBuf],
    review: &Review,
) -> Result<PathBuf, String> {
    let prepared = prepare_migration(policy, home, projects, review, false)?;
    finish_migration(policy, home, prepared)
}

fn finish_migration(policy: &Policy, home: &Path, prepared: Prepared) -> Result<PathBuf, String> {
    let Prepared { current, writes } = prepared;
    let expected: BTreeMap<_, _> = current
        .sources
        .iter()
        .map(|s| (s.path.clone(), s.digest.clone()))
        .collect();
    transaction::commit(home, writes, &expected, || {
        let sources = inventory::discover(home, &all_projects(policy, &current.projects))?;
        for source in &sources {
            if !current.sources.iter().any(|s| s.path == source.path)
                && source.path != home.join(".claude/settings.json")
                && source.path != home.join(".codex/config.toml")
            {
                return Err(format!(
                    "new source appeared during migration: {}",
                    source.path.display()
                ));
            }
        }
        serde_json::to_vec_pretty(&Record {
            version: 1,
            policy: policy.clone(),
            projects: current.projects.clone(),
            sources,
        })
        .map_err(|e| e.to_string())
    })
}

fn render_source(
    source: &Source,
    policy: &Policy,
    previous: Option<&Policy>,
) -> Result<Vec<u8>, String> {
    let before = String::from_utf8(read(&source.path)?).map_err(|e| e.to_string())?;
    if source.kind == "codex-config" {
        let mut doc: toml_edit::DocumentMut = before.parse().map_err(|e| format!("{e}"))?;
        // Remove legacy selectors and persistent overrides, retaining unrelated keys.
        for key in [
            "sandbox_mode",
            "sandbox_workspace_write",
            "default_permissions",
            "permissions",
            "approval_policy",
            "approvals_reviewer",
        ] {
            doc.remove(key);
        }
        scrub_codex(doc.as_table_mut());
        merge_toml(doc.as_table_mut(), &policy.generated()["codex"])?;
        return Ok(doc.to_string().into_bytes());
    }
    let mut root: Value = serde_json::from_str(&before).map_err(|e| e.to_string())?;
    if source.kind == "claude-settings" {
        let obj = root.as_object_mut().ok_or("settings must be object")?;
        if let Some(p) = obj.get_mut("permissions") {
            let p = p.as_object_mut().ok_or("permissions must be object")?;
            p.insert("allow".into(), json!([]));
            p.remove("additionalDirectories");
            if p.get("defaultMode")
                .is_some_and(|m| m == "bypassPermissions" || m == "dontAsk")
            {
                p.remove("defaultMode");
            }
        }
        // Only user source gets generated boundaries; local settings lose broadening fields.
        if source
            .path
            .file_name()
            .is_some_and(|f| f == "settings.local.json")
        {
            if let Some(sandbox) = obj.get_mut("sandbox") {
                let mut restrictions = json!({});
                for (parent, leaf) in [
                    ("filesystem", "denyRead"),
                    ("filesystem", "denyWrite"),
                    ("network", "deniedDomains"),
                ] {
                    if let Some(value) = sandbox.get(parent).and_then(|v| v.get(leaf)) {
                        restrictions
                            .as_object_mut()
                            .unwrap()
                            .entry(parent)
                            .or_insert(json!({}))[leaf] = value.clone();
                    }
                }
                *sandbox = restrictions;
            }
            if let Some(mode) = obj.get_mut("autoMode").and_then(Value::as_object_mut) {
                mode.remove("classifyAllShell");
            }
            for key in ["disableAllHooks", "allowManagedHooksOnly", "policyHelper"] {
                obj.remove(key);
            }
        } else {
            for key in ["allowManagedHooksOnly", "policyHelper"] {
                obj.remove(key);
            }
            // Preserve existing restrictions, discard user-side sandbox escapes.
            let mut sandbox = policy.generated()["claude"]["sandbox"].clone();
            if let Some(old) = obj.get("sandbox") {
                for (parent, leaf) in [
                    ("filesystem", "denyRead"),
                    ("filesystem", "denyWrite"),
                    ("network", "deniedDomains"),
                ] {
                    if let Some(values) = old[parent][leaf].as_array() {
                        let target = sandbox[parent]
                            .as_object_mut()
                            .unwrap()
                            .entry(leaf)
                            .or_insert(json!([]));
                        let target = target.as_array_mut().ok_or("restriction must be array")?;
                        for value in values {
                            let was_owned = parent == "filesystem"
                                && leaf == "denyRead"
                                && previous.is_some_and(|p| {
                                    p.secret_paths.iter().any(|s| value.as_str() == Some(s))
                                });
                            if !was_owned && !target.contains(value) {
                                target.push(value.clone());
                            }
                        }
                    }
                }
            }
            let mut generated = policy.generated()["claude"].clone();
            // Native deny rules are additive; retain the owner's stricter rules.
            if let Some(old) = obj.get("permissions").and_then(|p| p["deny"].as_array()) {
                let denies = generated["permissions"]["deny"].as_array_mut().unwrap();
                for value in old {
                    let was_owned = previous.is_some_and(|p| {
                        p.generated()["claude"]["permissions"]["deny"]
                            .as_array()
                            .unwrap()
                            .contains(value)
                    });
                    if !was_owned && !denies.contains(value) {
                        denies.push(value.clone());
                    }
                }
            }
            generated.as_object_mut().unwrap().remove("sandbox");
            obj.insert("sandbox".into(), sandbox);
            merge_json(&mut root, &generated)?;
        }
    } else if source.kind == "claude-state" {
        scrub_claude_state(&mut root);
    } else {
        return Err(format!(
            "{}: cannot manage kind {}; review retained source",
            source.path.display(),
            source.kind
        ));
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn scrub_claude_state(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        for key in ["allowedTools", "enabledMcpjsonServers"] {
            obj.remove(key);
        }
        if let Some(projects) = obj.get_mut("projects").and_then(Value::as_object_mut) {
            for project in projects.values_mut().filter_map(Value::as_object_mut) {
                for key in ["allowedTools", "enabledMcpjsonServers"] {
                    project.remove(key);
                }
            }
        }
    }
}

fn scrub_codex(table: &mut dyn toml_edit::TableLike) {
    prompt_servers(table);
    if let Some(apps) = table
        .get_mut("apps")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in apps.iter_mut() {
            if let Some(app) = item.as_table_like_mut() {
                prompt_tools(app);
                if let Some(links) = app
                    .get_mut("links")
                    .and_then(toml_edit::Item::as_table_like_mut)
                {
                    for (_, item) in links.iter_mut() {
                        if let Some(link) = item.as_table_like_mut() {
                            prompt_tools(link);
                        }
                    }
                }
            }
        }
    }
    if let Some(plugins) = table
        .get_mut("plugins")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in plugins.iter_mut() {
            if let Some(plugin) = item.as_table_like_mut() {
                prompt_servers(plugin);
            }
        }
    }
    // Profile overrides must not select a legacy/full-access sandbox or other policy.
    if let Some(profiles) = table
        .get_mut("profiles")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in profiles.iter_mut() {
            if let Some(p) = item.as_table_like_mut() {
                for k in [
                    "sandbox_mode",
                    "sandbox_workspace_write",
                    "default_permissions",
                    "approval_policy",
                    "approvals_reviewer",
                ] {
                    p.remove(k);
                }
                if let Some(f) = p
                    .get_mut("features")
                    .and_then(toml_edit::Item::as_table_like_mut)
                {
                    f.remove("network_proxy");
                }
            }
        }
    }
}

fn prompt_servers(table: &mut dyn toml_edit::TableLike) {
    if let Some(servers) = table
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in servers.iter_mut() {
            if let Some(server) = item.as_table_like_mut() {
                prompt_tools(server);
            }
        }
    }
}

fn prompt_tools(table: &mut dyn toml_edit::TableLike) {
    table.insert("default_tools_approval_mode", toml_edit::value("prompt"));
    if let Some(tools) = table
        .get_mut("tools")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (_, item) in tools.iter_mut() {
            if let Some(tool) = item.as_table_like_mut() {
                tool.insert("approval_mode", toml_edit::value("prompt"));
            }
        }
    }
}

fn merge_json(target: &mut Value, desired: &Value) -> Result<(), String> {
    let obj = target
        .as_object_mut()
        .ok_or("permission field must be object")?;
    for (key, value) in desired.as_object().ok_or("declaration must be object")? {
        if value.is_object() {
            merge_json(obj.entry(key).or_insert(json!({})), value)?;
        } else {
            obj.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn merge_toml(table: &mut dyn toml_edit::TableLike, desired: &Value) -> Result<(), String> {
    for (key, value) in desired.as_object().ok_or("expected object")? {
        if value.is_object() {
            if !table.contains_key(key) {
                table.insert(key, toml_edit::Item::Table(toml_edit::Table::new()));
            }
            merge_toml(
                table
                    .get_mut(key)
                    .and_then(toml_edit::Item::as_table_like_mut)
                    .ok_or_else(|| format!("{key} must be a table"))?,
                value,
            )?;
        } else {
            let item = if let Some(s) = value.as_str() {
                toml_edit::value(s)
            } else if let Some(b) = value.as_bool() {
                toml_edit::value(b)
            } else {
                return Err(format!("unsupported native value: {key}"));
            };
            table.insert(key, item);
        }
    }
    Ok(())
}

/// Hook adapter. Missing deployment is permitted before stage2; a deployed policy
/// without a valid ownership record is always denied. No process-global mutation.
pub fn hook_check(agent: &str, raw: &str) -> Result<Option<&'static str>, String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let path = home.join(".config/agent-permissions/policy.json");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if state_path(&home).exists() {
                return Err("deployed permission policy disappeared; restore declaration".into());
            }
            return Ok(None);
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(_) => {}
    }
    if std::env::var_os("CODEX_HOME").is_some() || std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        return Err("custom agent configuration home is not inventoried".into());
    }
    let policy = Policy::load(&path)?;
    let input: Value =
        serde_json::from_str(raw).map_err(|e| format!("permission hook input: {e}"))?;
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or("permission hook cwd missing")?;
    // Pending allow rules never block here; a structural drift blocks every call
    // except the inert ones, so the agent can still ask the owner and diagnose.
    if let Err(reason) = assess(&policy, &home, &[PathBuf::from(cwd)]) {
        let named = input.get("tool_name").and_then(Value::as_str);
        if inert_call(named, &input["tool_input"]) {
            return Ok(None);
        }
        return Err(reason);
    }
    if agent == "codex" {
        return Ok(None);
    } // Native profile, not ignored Codex ask output.
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or("permission hook tool missing")?;
    if tool.starts_with("mcp__") {
        return Ok(Some("ask"));
    }
    let args = &input["tool_input"];
    let shell = ["Bash", "PowerShell", "Monitor"].contains(&tool);
    if shell && args["dangerouslyDisableSandbox"] == true {
        return Ok(Some("ask"));
    }
    if shell {
        // Claude folds shared WebFetch allow rules into sandbox connectivity.
        // A one-time ask is needed when that retained connectivity exceeds canon.
        let record = load_record(&home)?;
        if record.sources.iter().any(|s| {
            !s.writable
                && s.grants["permissions"]["allow"]
                    .as_array()
                    .is_some_and(|a| {
                        a.iter().filter_map(Value::as_str).any(|r| {
                            r.starts_with("Edit")
                                || (r.starts_with("WebFetch")
                                    && !r
                                        .strip_prefix("WebFetch(domain:")
                                        .and_then(|d| d.strip_suffix(')'))
                                        .is_some_and(|d| policy.domains.iter().any(|p| p == d)))
                        })
                    })
        }) {
            return Ok(Some("ask"));
        }
    }
    if ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"].contains(&tool) {
        let path = args
            .get("file_path")
            .or_else(|| args.get("notebook_path"))
            .and_then(Value::as_str)
            .ok_or("file tool path missing")?;
        let path = resolve(&PathBuf::from(cwd).join(path))?;
        for secret in &policy.secret_paths {
            let secret = secret
                .strip_prefix("~/")
                .map_or_else(|| PathBuf::from(secret), |p| home.join(p));
            if path.starts_with(resolve(&secret)?) {
                return Err("raw credential access requires a reviewed operation".into());
            }
        }
        if tool != "Read"
            && !path.starts_with(resolve(Path::new(cwd))?)
            && !path.starts_with(resolve(&std::env::temp_dir())?)
        {
            return Ok(Some("ask"));
        }
    }
    if tool == "WebFetch" {
        let url = args["url"].as_str().ok_or("WebFetch URL missing")?;
        let host = url
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
            .ok_or("invalid URL")?;
        let project_known = policy.project_rules.iter().any(|(p, rule)| {
            resolve(Path::new(cwd))
                .ok()
                .zip(resolve(Path::new(p)).ok())
                .is_some_and(|(cwd, root)| cwd.starts_with(root))
                && rule.domains.iter().any(|d| d == host)
        });
        if !policy.domains.iter().any(|d| d == host) && !project_known {
            return Ok(Some("ask"));
        }
    }
    Ok(None)
}

fn resolve(path: &Path) -> Result<PathBuf, String> {
    fn walk(path: &Path, depth: usize) -> Result<PathBuf, String> {
        if depth > 128 {
            return Err("path resolution depth exceeded".into());
        }
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => {
                let target = std::fs::read_link(path).map_err(|e| e.to_string())?;
                return walk(
                    &path.parent().ok_or("symlink has no parent")?.join(target),
                    depth + 1,
                );
            }
            Ok(_) => {
                return std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        let parent = path.parent().ok_or("path has no parent")?;
        let leaf = path.file_name().ok_or("path has no filename")?;
        Ok(walk(parent, depth + 1)?.join(leaf))
    }
    walk(path, 0)
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "inspect",
        "list every permission source and its proposed disposition (read-only)",
    ),
    (
        "check",
        "verify the sources still match the ownership record (read-only)",
    ),
    (
        "apply",
        "reapply the declared policy; only narrowing is applied automatically",
    ),
    (
        "migrate",
        "write a reviewed migration, preview it (--dry-run), or undo one (--restore)",
    ),
    (
        "reclaim",
        "move pending allow rules to the promotion queue and remove them from settings",
    ),
    (
        "pending",
        "list the promotion queue (--json), or withdraw entries (--drop ID,ID)",
    ),
    (
        "hook",
        "session boundary hook: `hook session-start|session-end` (hook input on stdin)",
    ),
];

fn usage(command: Option<&str>) -> Result<String, String> {
    let mut out = String::new();
    match command {
        None => {
            out.push_str("usage: agent-permissions <command> [options]\n\ncommands:\n");
            for (name, about) in COMMANDS {
                out.push_str(&format!("  {name:<10}{about}\n"));
            }
        }
        Some(name) => {
            let (_, about) = COMMANDS
                .iter()
                .find(|(n, _)| *n == name)
                .ok_or("unknown subcommand")?;
            out.push_str(&format!(
                "usage: agent-permissions {name} [options]\n\n{about}\n"
            ));
        }
    }
    out.push_str(
        "\noptions:\n\
         \x20 --policy PATH       policy file (default: ~/.config/agent-permissions/policy.json)\n\
         \x20 --home PATH         absolute home directory (default: $HOME)\n\
         \x20 --project PATH      extra project to inventory (repeatable)\n\
         \x20 --json              machine-readable output (inspect, check)\n\
         \x20 --approved REVIEW   review file saved by `inspect --json` (migrate)\n\
         \x20 --dry-run           check the review and list changes without writing (migrate)\n\
         \x20 --restore JOURNAL   restore the files recorded in a journal (migrate)\n\
         \x20 --retire-ownership  rebuild a damaged ownership record (migrate)\n\
         \x20 --drop ID,ID        withdraw promotion candidates by id (pending)\n\
         \x20 -h, --help          show this help; `agent-permissions <command> --help` for one command\n\
         \nrecovery from a permission drift (only the third command writes):\n\
         \x20 agent-permissions inspect --json > review.json\n\
         \x20 agent-permissions migrate --approved review.json --dry-run\n\
         \x20 agent-permissions migrate --approved review.json\n\
         \x20 agent-permissions check\n",
    );
    Ok(out)
}

/// `agent-permissions hook session-start|session-end`: reads the hook input on
/// stdin and prints what the session should be told. It never fails a session.
fn hook_cli(args: &[String]) -> Result<(), String> {
    use std::io::Read as _;
    let event = args
        .first()
        .map(String::as_str)
        .filter(|e| matches!(*e, "session-start" | "session-end"))
        .ok_or("usage: agent-permissions hook session-start|session-end")?;
    let mut stdin = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut stdin);
    print!("{}", reclaim::hook_output(event, &stdin));
    Ok(())
}

pub fn cli(args: Vec<String>) -> Result<(), String> {
    // Help needs no policy, no environment checks and no files.
    let is_help = |a: &String| matches!(a.as_str(), "--help" | "-h" | "help");
    match args.first() {
        Some(first) if is_help(first) => {
            print!("{}", usage(None)?);
            return Ok(());
        }
        Some(first) if args[1..].iter().any(is_help) => {
            print!("{}", usage(Some(first))?);
            return Ok(());
        }
        _ => {}
    }
    if args.first().map(String::as_str) == Some("hook") {
        return hook_cli(&args[1..]);
    }
    if std::env::var_os("CODEX_HOME").is_some() || std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        return Err("custom CODEX_HOME/CLAUDE_CONFIG_DIR is not yet inventoried; use canonical settings, no changes made".into());
    }
    let command = args
        .first()
        .ok_or("usage: agent-permissions <command> [options]; run `agent-permissions --help`")?;
    let home_default = std::env::var_os("HOME").ok_or("HOME missing")?;
    let mut home = PathBuf::from(home_default);
    let mut policy_path = None;
    let mut projects = Vec::new();
    let mut approved = None;
    let mut restore = None;
    let mut drop_ids: Vec<String> = Vec::new();
    let mut output_json = false;
    let mut dry_run = false;
    let mut retire_ownership = false;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--json" {
            output_json = true;
            i += 1;
            continue;
        }
        if args[i] == "--dry-run" {
            dry_run = true;
            i += 1;
            continue;
        }
        if args[i] == "--retire-ownership" {
            retire_ownership = true;
            i += 1;
            continue;
        }
        let value = args.get(i + 1).ok_or("option value missing")?;
        match args[i].as_str() {
            "--policy" => policy_path = Some(PathBuf::from(value)),
            "--home" => home = PathBuf::from(value),
            "--project" => projects.push(PathBuf::from(value)),
            "--approved" => approved = Some(PathBuf::from(value)),
            "--restore" => restore = Some(PathBuf::from(value)),
            "--drop" => drop_ids = value.split(',').map(str::to_owned).collect(),
            flag => return Err(format!("unknown option: {flag}")),
        }
        i += 2;
    }
    if !home.is_absolute() {
        return Err("--home must be absolute".into());
    }
    if (dry_run || retire_ownership) && command != "migrate" {
        return Err("migration options require migrate".into());
    }
    if let Some(journal) = restore {
        if command != "migrate" {
            return Err("--restore requires migrate".into());
        }
        transaction::restore(&home, &journal)?;
        println!("restored and verified {}", journal.display());
        return Ok(());
    }
    if command == "pending" {
        if !drop_ids.is_empty() {
            let n = reclaim::drop_candidates(&home, &drop_ids)?;
            println!("withdrew {n} candidate(s)");
            return Ok(());
        }
        let queue = reclaim::candidates(&home)?;
        if output_json {
            println!("{}", json!({"candidates": queue}));
        } else if queue.is_empty() {
            println!("no pending promotion candidates");
        } else {
            for c in &queue {
                println!("{}  {}  {}", c.id, c.rule, c.project.display());
            }
        }
        return Ok(());
    }
    let path = policy_path.unwrap_or_else(|| home.join(".config/agent-permissions/policy.json"));
    let policy = Policy::load(&path)?;
    match command.as_str() {
        "inspect" => {
            let review = inspect(&policy, &home, &projects)?;
            if output_json {
                println!("{}", serde_json::to_string_pretty(&review).unwrap());
            } else {
                println!(
                    "{} distinct permission sources; policy {}",
                    review.sources.len(),
                    policy.hash()
                );
                if let Some(problem) = &review.ownership_problem {
                    println!("REVIEW ownership: {problem}");
                }
                for s in &review.sources {
                    println!(
                        "{} {} {} {}",
                        review.decisions[&s.path],
                        s.kind,
                        s.path.display(),
                        s.digest
                    );
                    println!("{}", serde_json::to_string_pretty(&s.grants).unwrap());
                    for u in &s.unknown {
                        println!("REVIEW: {u}");
                    }
                }
                println!(
                    "Proposed native settings:\n{}",
                    serde_json::to_string_pretty(&review.generated).unwrap()
                );
                println!("Use --json to save an exact review. Review dispositions before migrate --approved REVIEW.");
            }
        }
        "check" => {
            // Pending allow rules are reported, not an error: they wait for reclaim.
            let waiting = assess(&policy, &home, &projects)?;
            if output_json {
                println!("{}", json!({"ok": true, "pending": waiting}));
            } else {
                println!("permissions match policy and ownership record");
                for p in &waiting {
                    println!(
                        "pending grants awaiting reclaim: {}: {}",
                        p.path.display(),
                        p.rules.join(", ")
                    );
                }
            }
        }
        "apply" => {
            apply(&policy, &home, &projects)?;
            println!("permissions applied and read back");
        }
        "reclaim" => {
            let done = reclaim::reclaim(&policy, &home, &projects)?;
            if output_json {
                println!("{}", serde_json::to_string(&done).unwrap());
            } else {
                println!(
                    "reclaimed {} rule(s) in {} file(s); {} new candidate(s) queued",
                    done.removed, done.files, done.queued
                );
            }
        }
        "migrate" => {
            let path = approved.ok_or(
                "migrate requires --approved REVIEW; inspect and obtain the owner's review first",
            )?;
            let review: Review =
                serde_json::from_slice(&read(&path)?).map_err(|e| e.to_string())?;
            let prepared = prepare_migration(&policy, &home, &projects, &review, retire_ownership)?;
            if dry_run {
                let changes:Vec<_> = prepared.writes.iter().map(|(path, data)| {
                    let before = prepared.current.sources.iter().find(|s| s.path == *path).map(|s| &s.digest);
                    let after = data.as_ref().map(|b| digest(b));
                    json!({
                        "path": path,
                        "before": before,
                        "after": after,
                        "changed": before != after.as_ref(),
                        "disposition": review.decisions.get(path).map(String::as_str).unwrap_or("create")
                    })
                }).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"dry_run":true,"changes":changes}))
                        .unwrap()
                );
                return Ok(());
            }
            let journal = finish_migration(&policy, &home, prepared)?;
            check(&policy, &home, &projects)?;
            println!(
                "migrated and read back; restore journal: {}",
                journal.display()
            );
        }
        _ => return Err("unknown subcommand".into()),
    }
    Ok(())
}
