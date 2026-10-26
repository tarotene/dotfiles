//! Read-only inventory of permission-bearing configuration sources.
//!
//! Codex schemas: openai/codex rust-v0.164.0, config/src/{config_toml,
//! profile_toml,mcp_types,types}.rs (retrieved 2026-10-26).
//! Claude scopes: <https://code.claude.com/docs/en/settings> (retrieved 2026-10-26).
//! Transport configuration, environment variables, credentials and model settings
//! are deliberately excluded from `grants`; the digest covers the complete file.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: PathBuf,
    pub kind: String,
    pub writable: bool,
    pub digest: String,
    pub grants: Value,
    pub unknown: Vec<String>,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Discover configured projects without recursively scanning the home directory.
/// Missing optional files are normal; unreadable or malformed present sources
/// are errors. Every discovered physical file appears exactly once.
pub fn discover(home: &Path, projects: &[PathBuf]) -> Result<Vec<Source>, String> {
    let home = home.canonicalize().map_err(|e| at(home, e))?;
    let mut inventory = Inventory::default();
    let mut roots: BTreeSet<PathBuf> = projects.iter().cloned().collect();
    inventory.add(&home.join(".claude/settings.json"), "claude-settings", true)?;
    if let Some(state) = inventory.add(&home.join(".claude.json"), "claude-state", true)? {
        collect_projects(&state, "projects", &mut roots)?;
    }
    if let Some(config) = inventory.add(&home.join(".codex/config.toml"), "codex-config", true)? {
        collect_projects(&config, "projects", &mut roots)?;
    }
    inventory.rules(&home.join(".codex/rules"), true)?;
    inventory.add(&home.join(".claude/remote-settings.json"), "managed", false)?;
    inventory.add(
        &home.join(".claude/managed-settings.json"),
        "managed",
        false,
    )?;
    inventory.add(&home.join(".codex/requirements.toml"), "managed", false)?;
    // Plugin permission policy is read from host settings/config. Installed
    // plugin settings can contribute defaults too; only enumerate paths named
    // by the installed-plugin registry, never arbitrary cached files.
    inventory.claude_plugins(&home.join(".claude/plugins/installed_plugins.json"))?;
    for p in [
        "/etc/claude-code/managed-settings.json",
        "/Library/Application Support/ClaudeCode/managed-settings.json",
    ] {
        inventory.add(Path::new(p), "managed", false)?;
    }
    inventory.json_directory(Path::new("/etc/claude-code/managed-settings.d"), "managed")?;
    inventory.add(Path::new("/etc/codex/requirements.toml"), "managed", false)?;
    // Legacy managed configuration remains a higher-precedence source in
    // rust-v0.164.0 (config/src/loader/mod.rs L116–121).
    inventory.add(
        Path::new("/etc/codex/managed_config.toml"),
        "managed",
        false,
    )?;
    inventory.add(Path::new("/etc/codex/config.toml"), "codex-config", false)?;

    for root in roots {
        if !root.is_absolute() {
            return Err(format!("project path must be absolute: {}", root.display()));
        }
        // A stale trust record may reference a removed checkout. It has no
        // configuration to load, but permission errors are never called absent.
        match std::fs::metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(at(&root, e)),
            Ok(m) if !m.is_dir() => return Err(at(&root, "project is not a directory")),
            Ok(_) => (),
        }
        for dir in root.ancestors() {
            if dir != home {
                inventory.add(&dir.join(".claude/settings.json"), "claude-settings", false)?;
                // Codex rust-v0.164.0 loader/mod.rs:1866-1876 ignores a
                // project .codex that is a regular file, not a directory.
                match std::fs::metadata(dir.join(".codex")) {
                    Ok(m) if m.is_dir() => {
                        inventory.add(&dir.join(".codex/config.toml"), "codex-config", false)?;
                        inventory.rules(&dir.join(".codex/rules"), false)?;
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(at(dir, e)),
                }
            }
            inventory.local(&dir.join(".claude/settings.local.json"))?;
            inventory.add(&dir.join(".mcp.json"), "claude-mcp", false)?;
        }
        // Claude 2.1.285 stores linked-worktree local settings at the main
        // checkout. Read .git text directly: no git execution, configuration,
        // pager, external helpers or shell is involved in inventory.
        if let Some(main) = main_checkout(&root)? {
            inventory.local(&main.join(".claude/settings.local.json"))?;
        }
    }
    Ok(inventory.files.into_values().collect())
}

#[derive(Default)]
struct Inventory {
    files: BTreeMap<PathBuf, Source>,
}

impl Inventory {
    fn local(&mut self, path: &Path) -> Result<(), String> {
        // Even a file named settings.local.json is shared when tracked. Git's
        // read-only index query disables fsmonitor to avoid spawning helpers.
        let exists = match std::fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(at(path, e)),
        };
        if !exists {
            return Ok(());
        }
        let directory = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| at(path, "missing project directory"))?;
        let mut in_repo = false;
        for parent in directory.ancestors() {
            let marker = parent.join(".git");
            match std::fs::metadata(&marker) {
                // Some sandboxes put an empty .git sentinel in /tmp. It
                // contains no index and cannot make files tracked.
                Ok(m) if m.is_dir() => {
                    if std::fs::read_dir(&marker)
                        .map_err(|e| at(&marker, e))?
                        .next()
                        .is_some()
                    {
                        in_repo = true;
                        break;
                    }
                }
                Ok(_) => {
                    in_repo = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(at(&marker, e)),
            }
        }
        let tracked = if in_repo {
            let output = std::process::Command::new("git")
                .args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-C"])
                .arg(directory)
                .args([
                    "ls-files",
                    "--error-unmatch",
                    "--",
                    ".claude/settings.local.json",
                ])
                .output()
                .map_err(|e| at(path, e))?;
            match output.status.code() {
                Some(0) => true,
                Some(1) => false,
                _ => {
                    return Err(at(
                        path,
                        "cannot determine whether local settings are tracked",
                    ))
                }
            }
        } else {
            false
        };
        self.add(path, "claude-settings", !tracked)?;
        Ok(())
    }

    fn add(&mut self, path: &Path, kind: &str, writable: bool) -> Result<Option<Value>, String> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(at(path, e)),
        };
        let physical = path.canonicalize().map_err(|e| at(path, e))?;
        let symlink = metadata.file_type().is_symlink() || has_symlink_parent(path)?;
        if !std::fs::metadata(&physical)
            .map_err(|e| at(path, e))?
            .is_file()
        {
            return Err(at(path, "configuration source is not a regular file"));
        }
        let bytes = std::fs::read(&physical).map_err(|e| at(path, e))?;
        let text = std::str::from_utf8(&bytes).map_err(|e| at(path, e))?;
        let value = if path.extension().is_some_and(|s| s == "toml") {
            let doc: DocumentMut = text.parse().map_err(|e| at(path, e))?;
            table_json(doc.as_table())
        } else if kind == "codex-rules" {
            json!({})
        } else {
            let v: Value = serde_json::from_str(text).map_err(|e| at(path, e))?;
            if !v.is_object() {
                return Err(at(path, "configuration must be an object"));
            }
            v
        };
        let mut unknown = Vec::new();
        let grants = match kind {
            "codex-rules" => {
                let opaque = text
                    .lines()
                    .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
                if opaque {
                    unknown.push("executable Starlark rules require native parsing or explicit reviewed retirement".into());
                }
                json!({"opaque_rules": opaque, "rules": text})
            }
            "claude-state" => claude_state(&value, &mut unknown),
            "claude-mcp" => json!({}), // definitions are not standing approvals
            "codex-config" => codex(&value, &mut unknown),
            "managed" if path.extension().is_some_and(|s| s == "toml") => {
                unknown.push(
                    "administrator requirements are retained; effective policy must be reviewed"
                        .into(),
                );
                json!({})
            }
            _ => claude(&value, &mut unknown),
        };
        if let Some(existing) = self.files.get_mut(&physical) {
            // Shared/managed ownership wins over a writable alias.
            existing.writable &= writable && !symlink;
            return Ok(Some(value));
        }
        self.files.insert(
            physical.clone(),
            Source {
                path: physical,
                kind: kind.into(),
                writable: writable && !symlink,
                digest: digest(&bytes),
                grants,
                unknown,
            },
        );
        Ok(Some(value))
    }

    fn rules(&mut self, dir: &Path, writable: bool) -> Result<(), String> {
        for path in entries(dir)? {
            if path.extension().is_some_and(|e| e == "rules") {
                self.add(&path, "codex-rules", writable)?;
            }
        }
        Ok(())
    }

    fn json_directory(&mut self, dir: &Path, kind: &str) -> Result<(), String> {
        for path in entries(dir)? {
            if path.extension().is_some_and(|e| e == "json") {
                self.add(&path, kind, false)?;
            }
        }
        Ok(())
    }

    fn claude_plugins(&mut self, registry: &Path) -> Result<(), String> {
        let text = match std::fs::read_to_string(registry) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(at(registry, e)),
        };
        let value: Value = serde_json::from_str(&text).map_err(|e| at(registry, e))?;
        let plugins = value
            .get("plugins")
            .and_then(Value::as_object)
            .ok_or_else(|| at(registry, "plugin registry plugins must be an object"))?;
        for installs in plugins.values() {
            let installs = installs
                .as_array()
                .ok_or_else(|| at(registry, "plugin installations must be an array"))?;
            for install in installs {
                let path = install
                    .get("installPath")
                    .and_then(Value::as_str)
                    .ok_or_else(|| at(registry, "plugin installPath missing"))?;
                let path = Path::new(path);
                if !path.is_absolute() {
                    return Err(at(registry, "plugin installPath must be absolute"));
                }
                self.add(&path.join("settings.json"), "claude-plugin", false)?;
            }
        }
        Ok(())
    }
}

fn at(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(at(dir, e)),
    };
    let mut paths = read
        .map(|entry| entry.map(|e| e.path()).map_err(|e| at(dir, e)))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn has_symlink_parent(path: &Path) -> Result<bool, String> {
    for parent in path.ancestors().skip(1) {
        if std::fs::symlink_metadata(parent)
            .map_err(|e| at(parent, e))?
            .file_type()
            .is_symlink()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_projects(
    value: &Value,
    field: &str,
    roots: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if let Some(projects) = value.get(field) {
        let projects = projects
            .as_object()
            .ok_or_else(|| format!("{field} must be an object"))?;
        for path in projects.keys() {
            roots.insert(PathBuf::from(path));
        }
    }
    Ok(())
}

fn main_checkout(root: &Path) -> Result<Option<PathBuf>, String> {
    for dir in root.ancestors() {
        let dotgit = dir.join(".git");
        let metadata = match std::fs::metadata(&dotgit) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(at(&dotgit, e)),
        };
        if metadata.is_dir() {
            return Ok(Some(dir.to_path_buf()));
        }
        let text = std::fs::read_to_string(&dotgit).map_err(|e| at(&dotgit, e))?;
        let gitdir = text
            .trim()
            .strip_prefix("gitdir: ")
            .ok_or_else(|| at(&dotgit, "invalid gitdir file"))?;
        let gitdir = dir.join(gitdir);
        let common_file = gitdir.join("commondir");
        let common = match std::fs::read_to_string(&common_file) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Some(dir.to_path_buf()))
            }
            Err(e) => return Err(at(&common_file, e)),
        };
        let common = gitdir
            .join(common.trim())
            .canonicalize()
            .map_err(|e| at(&common_file, e))?;
        return Ok(common.parent().map(Path::to_path_buf));
    }
    Ok(None)
}

fn permission_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "permission",
        "sandbox",
        "approval",
        "allow",
        "deny",
        "automode",
    ]
    .iter()
    .any(|word| key.contains(word))
}

fn pick(value: &Value, names: &[&str]) -> Value {
    Value::Object(
        value
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| names.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn claude(value: &Value, unknown: &mut Vec<String>) -> Value {
    const KEYS: &[&str] = &[
        "permissions",
        "sandbox",
        "autoMode",
        "useAutoModeDuringPlan",
        "enableAllProjectMcpServers",
        "enabledMcpjsonServers",
        "disabledMcpjsonServers",
        "allowedMcpServers",
        "deniedMcpServers",
        "allowManagedPermissionRulesOnly",
        "allowManagedMcpServersOnly",
        "disableAllHooks",
        "allowManagedHooksOnly",
        "allowManagedSettingsFrom",
        "policyHelper",
        "enabledPlugins",
    ];
    if let Some(root) = value.as_object() {
        for key in root.keys() {
            if permission_key(key) && !KEYS.contains(&key.as_str()) {
                unknown.push(format!("unknown Claude permission key: {key}"));
            }
        }
    }
    if let Some(permissions) = value.get("permissions") {
        if let Some(permissions) = permissions.as_object() {
            for (key, val) in permissions {
                if ![
                    "allow",
                    "ask",
                    "deny",
                    "defaultMode",
                    "additionalDirectories",
                    "disableBypassPermissionsMode",
                    "disableAutoMode",
                ]
                .contains(&key.as_str())
                {
                    unknown.push(format!("unknown permissions.{key}"));
                }
                if ["allow", "ask", "deny", "additionalDirectories"].contains(&key.as_str())
                    && !strings(val)
                {
                    unknown.push(format!("permissions.{key} must be an array of strings"));
                }
                if ["allow", "ask", "deny"].contains(&key.as_str()) {
                    for rule in val
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        if !known_claude_rule(rule) {
                            unknown
                                .push(format!("unrecognized permission rule in permissions.{key}"));
                        }
                    }
                }
                if key == "defaultMode" {
                    enum_field(
                        permissions,
                        key,
                        &[
                            "default",
                            "manual",
                            "acceptEdits",
                            "plan",
                            "auto",
                            "dontAsk",
                            "bypassPermissions",
                        ],
                        "permissions",
                        unknown,
                    );
                }
                if ["disableBypassPermissionsMode", "disableAutoMode"].contains(&key.as_str()) {
                    enum_field(permissions, key, &["disable"], "permissions", unknown);
                }
            }
        } else {
            unknown.push("permissions must be an object".into());
        }
    }
    if let Some(sandbox) = value.get("sandbox") {
        known_fields(
            sandbox,
            "sandbox",
            &[
                "enabled",
                "autoAllowBashIfSandboxed",
                "allowUnsandboxedCommands",
                "excludedCommands",
                "enableWeakerNestedSandbox",
                "enableWeakerNetworkIsolation",
                "failIfUnavailable",
                "network",
                "filesystem",
                "ignoreViolations",
            ],
            unknown,
        );
        booleans(
            sandbox,
            "sandbox",
            &[
                "enabled",
                "autoAllowBashIfSandboxed",
                "allowUnsandboxedCommands",
                "enableWeakerNestedSandbox",
                "enableWeakerNetworkIsolation",
                "failIfUnavailable",
            ],
            unknown,
        );
        string_arrays(sandbox, "sandbox", &["excludedCommands"], unknown);
        if let Some(filesystem) = sandbox.get("filesystem") {
            known_fields(
                filesystem,
                "sandbox.filesystem",
                &["allowWrite", "denyWrite", "denyRead", "allowRead"],
                unknown,
            );
            string_arrays(
                filesystem,
                "sandbox.filesystem",
                &["allowWrite", "denyWrite", "denyRead", "allowRead"],
                unknown,
            );
        }
        if let Some(network) = sandbox.get("network") {
            known_fields(
                network,
                "sandbox.network",
                &[
                    "allowedDomains",
                    "deniedDomains",
                    "allowManagedDomainsOnly",
                    "allowUnixSockets",
                    "allowAllUnixSockets",
                    "allowLocalBinding",
                    "httpProxyPort",
                    "socksProxyPort",
                ],
                unknown,
            );
            string_arrays(
                network,
                "sandbox.network",
                &["allowedDomains", "deniedDomains", "allowUnixSockets"],
                unknown,
            );
            booleans(
                network,
                "sandbox.network",
                &[
                    "allowManagedDomainsOnly",
                    "allowAllUnixSockets",
                    "allowLocalBinding",
                ],
                unknown,
            );
            for key in ["httpProxyPort", "socksProxyPort"] {
                if let Some(port) = network.get(key) {
                    if !port.as_u64().is_some_and(|p| p > 0 && p <= 65535) {
                        unknown.push(format!("sandbox.network.{key} must be a port number"));
                    }
                }
            }
        }
    }
    if let Some(mode) = value.get("autoMode") {
        known_fields(
            mode,
            "autoMode",
            &[
                "classifyAllShell",
                "environment",
                "allow",
                "soft_deny",
                "hard_deny",
            ],
            unknown,
        );
        booleans(mode, "autoMode", &["classifyAllShell"], unknown);
        // Claude 2.1.285's embedded native schema and the official
        // auto-mode-config reference (retrieved 2026-10-26) define these as
        // string arrays. `$defaults` remains an ordinary native entry; do not
        // expand it into a second, drifting copy of the built-in rules.
        string_arrays(
            mode,
            "autoMode",
            &["environment", "allow", "soft_deny", "hard_deny"],
            unknown,
        );
    }
    booleans(
        value,
        "Claude",
        &[
            "enableAllProjectMcpServers",
            "allowManagedPermissionRulesOnly",
            "allowManagedMcpServersOnly",
            "disableAllHooks",
            "allowManagedHooksOnly",
            "useAutoModeDuringPlan",
        ],
        unknown,
    );
    string_arrays(
        value,
        "Claude",
        &["enabledMcpjsonServers", "disabledMcpjsonServers"],
        unknown,
    );
    if let Some(plugins) = value.get("enabledPlugins") {
        if !plugins
            .as_object()
            .is_some_and(|p| p.values().all(Value::is_boolean))
        {
            unknown.push("enabledPlugins must map plugin names to booleans".into());
        }
    }
    pick(value, KEYS)
}

fn booleans(value: &Value, prefix: &str, keys: &[&str], unknown: &mut Vec<String>) {
    for key in keys {
        if value.get(key).is_some_and(|v| !v.is_boolean()) {
            unknown.push(format!("{prefix}.{key} must be a boolean"));
        }
    }
}

fn string_arrays(value: &Value, prefix: &str, keys: &[&str], unknown: &mut Vec<String>) {
    for key in keys {
        if value.get(key).is_some_and(|v| !strings(v)) {
            unknown.push(format!("{prefix}.{key} must be an array of strings"));
        }
    }
}

fn enum_field(
    fields: &Map<String, Value>,
    key: &str,
    choices: &[&str],
    prefix: &str,
    unknown: &mut Vec<String>,
) {
    if fields
        .get(key)
        .is_some_and(|v| !v.as_str().is_some_and(|s| choices.contains(&s)))
    {
        unknown.push(format!("unrecognized {prefix}.{key}"));
    }
}

fn known_fields(value: &Value, prefix: &str, fields: &[&str], unknown: &mut Vec<String>) {
    if let Some(object) = value.as_object() {
        for key in object.keys() {
            if !fields.contains(&key.as_str()) {
                unknown.push(format!("unknown {prefix}.{key}"));
            }
        }
    } else {
        unknown.push(format!("{prefix} must be an object/table"));
    }
}

fn known_claude_rule(rule: &str) -> bool {
    let tool = if let Some((tool, specifier)) = rule.split_once('(') {
        if !specifier.ends_with(')') {
            return false;
        }
        tool
    } else {
        rule
    };
    [
        "Bash",
        "Read",
        "Edit",
        "Write",
        "Glob",
        "Grep",
        "WebFetch",
        "WebSearch",
        "NotebookEdit",
        "Agent",
        "Task",
        "Skill",
        "ToolSearch",
        "TodoWrite",
    ]
    .contains(&tool)
        || (tool.starts_with("mcp__")
            && tool.len() > "mcp__".len()
            && tool
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_*-".contains(&b)))
}

fn claude_state(value: &Value, unknown: &mut Vec<String>) -> Value {
    let mut out = pick(value, &["allowedTools", "toolPermissions", "permissions"]);
    if let Some(projects) = value.get("projects") {
        if let Some(projects) = projects.as_object() {
            let projects: Map<_, _> = projects
                .iter()
                .filter_map(|(path, project)| {
                    state_unknown(project, unknown);
                    let grants = pick(
                        project,
                        &[
                            "allowedTools",
                            "toolPermissions",
                            "permissions",
                            "enabledMcpjsonServers",
                            "disabledMcpjsonServers",
                            "enableAllProjectMcpServers",
                        ],
                    );
                    (!grants.as_object().is_some_and(Map::is_empty)).then(|| (path.clone(), grants))
                })
                .collect();
            if !projects.is_empty() {
                out["projects"] = Value::Object(projects);
            }
        } else {
            unknown.push("Claude projects must be an object".into());
        }
    }
    state_unknown(value, unknown);
    out
}

fn state_unknown(value: &Value, unknown: &mut Vec<String>) {
    if !value.is_object() {
        unknown.push("Claude project state must be an object".into());
    }
    for key in ["permissions", "toolPermissions"] {
        if value.get(key).is_some() {
            unknown.push(format!("Claude state {key} semantics require review"));
        }
    }
    string_arrays(
        value,
        "Claude state",
        &[
            "allowedTools",
            "enabledMcpjsonServers",
            "disabledMcpjsonServers",
        ],
        unknown,
    );
    booleans(
        value,
        "Claude state",
        &["enableAllProjectMcpServers"],
        unknown,
    );
}

fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.iter().all(Value::is_string))
}

fn codex(value: &Value, unknown: &mut Vec<String>) -> Value {
    const KEYS: &[&str] = &[
        "approval_policy",
        "approvals_reviewer",
        "auto_review",
        "sandbox_mode",
        "sandbox_workspace_write",
        "default_permissions",
        "permissions",
        "allow_symlinked_codex_home",
        "browser_use",
        "computer_use",
    ];
    let mut out = pick(value, KEYS);
    if let Some(fields) = value.as_object() {
        enum_field(
            fields,
            "approvals_reviewer",
            &["user", "auto_review", "guardian_subagent"],
            "Codex",
            unknown,
        );
        enum_field(
            fields,
            "sandbox_mode",
            &["read-only", "workspace-write", "danger-full-access"],
            "Codex",
            unknown,
        );
        if let Some(policy) = fields.get("approval_policy") {
            if policy.is_string() {
                enum_field(
                    fields,
                    "approval_policy",
                    &["untrusted", "on-request", "on-failure", "never"],
                    "Codex",
                    unknown,
                );
            } else {
                // Keep new compound variants visible without pretending to
                // validate an unimplemented upstream policy interpreter.
                unknown.push("compound Codex approval_policy requires review".into());
            }
        }
        if fields
            .get("default_permissions")
            .is_some_and(|v| !v.is_string())
        {
            unknown.push("default_permissions must be a profile name".into());
        }
    } else {
        unknown.push("Codex config/profile must be a table".into());
    }
    booleans(value, "Codex", &["allow_symlinked_codex_home"], unknown);
    if let Some(root) = value.as_object() {
        for key in root.keys() {
            if permission_key(key)
                && !KEYS.contains(&key.as_str())
                && !["allow_login_shell", "include_permissions_instructions"]
                    .contains(&key.as_str())
            {
                unknown.push(format!("unknown Codex permission key: {key}"));
            }
        }
    }
    if let Some(features) = value.get("features") {
        if !features.is_object() {
            unknown.push("Codex features must be a table".into());
        }
        booleans(
            features,
            "features",
            &[
                "network_proxy",
                "exec_policy",
                "guardian_approval",
                "sandbox_heuristic",
            ],
            unknown,
        );
        let features = pick(
            features,
            &[
                "network_proxy",
                "exec_policy",
                "guardian_approval",
                "sandbox_heuristic",
            ],
        );
        if !features.as_object().is_some_and(Map::is_empty) {
            out["features"] = features;
        }
    }
    if let Some(profiles) = value.get("profiles") {
        if let Some(profiles) = profiles.as_object() {
            let mut selected = Map::new();
            for (name, profile) in profiles {
                let p = codex(profile, unknown);
                if !p.as_object().is_some_and(Map::is_empty) {
                    selected.insert(name.clone(), p);
                }
            }
            if !selected.is_empty() {
                out["profiles"] = Value::Object(selected);
            }
        } else {
            unknown.push("Codex profiles must be a table".into());
        }
    }
    if let Some(servers) = value.get("mcp_servers") {
        out["mcp_servers"] = server_policies(servers, unknown);
    }
    if let Some(apps) = value.get("apps") {
        out["apps"] = app_policies(apps, unknown);
    }
    if let Some(plugins) = value.get("plugins") {
        let mut policy = Map::new();
        if let Some(plugins) = plugins.as_object() {
            for (name, plugin) in plugins {
                let mut p = pick(plugin, &["enabled"]);
                if !plugin.is_object() {
                    unknown.push("Codex plugin policy must be a table".into());
                }
                booleans(plugin, "plugin", &["enabled"], unknown);
                if let Some(servers) = plugin.get("mcp_servers") {
                    p["mcp_servers"] = server_policies(servers, unknown);
                }
                if !p.as_object().is_some_and(Map::is_empty) {
                    policy.insert(name.clone(), p);
                }
            }
        } else {
            unknown.push("Codex plugins must be a table".into());
        }
        if !policy.is_empty() {
            out["plugins"] = Value::Object(policy);
        }
    }
    out
}

fn server_policies(value: &Value, unknown: &mut Vec<String>) -> Value {
    let Some(servers) = value.as_object() else {
        unknown.push("mcp_servers must be a table".into());
        return json!({});
    };
    let mut out = Map::new();
    for (name, server) in servers {
        if !server.is_object() {
            unknown.push("MCP server policy must be a table".into());
        }
        let mut policy = pick(
            server,
            &[
                "default_tools_approval_mode",
                "enabled_tools",
                "disabled_tools",
                "enabled",
            ],
        );
        validate_modes(&policy, unknown);
        booleans(server, "MCP server", &["enabled"], unknown);
        string_arrays(
            server,
            "MCP server",
            &["enabled_tools", "disabled_tools"],
            unknown,
        );
        unknown_policy_fields(
            server,
            "MCP server",
            &[
                "default_tools_approval_mode",
                "enabled_tools",
                "disabled_tools",
            ],
            unknown,
        );
        if let Some(tools) = server.get("tools") {
            policy["tools"] = tool_policies(tools, unknown);
        }
        if !policy.as_object().is_some_and(Map::is_empty) {
            out.insert(name.clone(), policy);
        }
    }
    Value::Object(out)
}

fn tool_policies(value: &Value, unknown: &mut Vec<String>) -> Value {
    let Some(tools) = value.as_object() else {
        unknown.push("tools must be a table".into());
        return json!({});
    };
    Value::Object(
        tools
            .iter()
            .filter_map(|(name, tool)| {
                if !tool.is_object() {
                    unknown.push("tool policy must be a table".into());
                }
                let p = pick(tool, &["approval_mode", "enabled"]);
                validate_modes(&p, unknown);
                booleans(tool, "tool", &["enabled"], unknown);
                unknown_policy_fields(tool, "tool", &["approval_mode"], unknown);
                (!p.as_object().is_some_and(Map::is_empty)).then(|| (name.clone(), p))
            })
            .collect(),
    )
}

fn app_policies(value: &Value, unknown: &mut Vec<String>) -> Value {
    let Some(apps) = value.as_object() else {
        unknown.push("apps must be a table".into());
        return json!({});
    };
    let mut out = Map::new();
    for (name, app) in apps {
        if !app.is_object() {
            unknown.push("app policy must be a table".into());
        }
        let mut policy = pick(
            app,
            &[
                "approvals_reviewer",
                "default_tools_approval_mode",
                "destructive_enabled",
                "open_world_enabled",
                "enabled",
                "default_tools_enabled",
            ],
        );
        validate_modes(&policy, unknown);
        booleans(
            app,
            "app",
            &[
                "enabled",
                "default_tools_enabled",
                "destructive_enabled",
                "open_world_enabled",
            ],
            unknown,
        );
        unknown_policy_fields(
            app,
            "app",
            &["approvals_reviewer", "default_tools_approval_mode"],
            unknown,
        );
        if let Some(tools) = app.get("tools") {
            policy["tools"] = tool_policies(tools, unknown);
        }
        if let Some(links) = app.get("links") {
            if let Some(links) = links.as_object() {
                let mut selected = Map::new();
                for (name, link) in links {
                    if !link.is_object() {
                        unknown.push("app link policy must be a table".into());
                    }
                    let p = pick(link, &["approvals_reviewer", "default_tools_approval_mode"]);
                    validate_modes(&p, unknown);
                    unknown_policy_fields(
                        link,
                        "app link",
                        &["approvals_reviewer", "default_tools_approval_mode"],
                        unknown,
                    );
                    if !p.as_object().is_some_and(Map::is_empty) {
                        selected.insert(name.clone(), p);
                    }
                }
                policy["links"] = Value::Object(selected);
            } else {
                unknown.push("apps links must be a table".into());
            }
        }
        if !policy.as_object().is_some_and(Map::is_empty) {
            out.insert(name.clone(), policy);
        }
    }
    Value::Object(out)
}

fn validate_modes(value: &Value, unknown: &mut Vec<String>) {
    if let Some(fields) = value.as_object() {
        enum_field(
            fields,
            "approvals_reviewer",
            &["user", "auto_review", "guardian_subagent"],
            "tool",
            unknown,
        );
    }
    for key in ["default_tools_approval_mode", "approval_mode"] {
        if let Some(v) = value.get(key) {
            if !v
                .as_str()
                .is_some_and(|s| ["prompt", "approve", "auto", "writes"].contains(&s))
            {
                unknown.push(format!("unrecognized {key}"));
            }
        }
    }
}

fn unknown_policy_fields(value: &Value, prefix: &str, known: &[&str], unknown: &mut Vec<String>) {
    for key in value.as_object().into_iter().flatten().map(|(key, _)| key) {
        if permission_key(key) && !known.contains(&key.as_str()) {
            unknown.push(format!("unknown {prefix} permission key: {key}"));
        }
    }
}

fn table_json(table: &dyn toml_edit::TableLike) -> Value {
    Value::Object(
        table
            .iter()
            .filter(|(_, item)| !item.is_none())
            .map(|(key, item)| (key.to_owned(), item_json(item)))
            .collect(),
    )
}

fn item_json(item: &Item) -> Value {
    match item {
        Item::Value(value) => value_json(value),
        Item::Table(table) => table_json(table),
        Item::ArrayOfTables(tables) => Value::Array(tables.iter().map(|t| table_json(t)).collect()),
        Item::None => Value::Null,
    }
}

fn value_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(v) => json!(v.value()),
        toml_edit::Value::Integer(v) => json!(v.value()),
        toml_edit::Value::Float(v) => json!(v.value()),
        toml_edit::Value::Boolean(v) => json!(v.value()),
        toml_edit::Value::Datetime(v) => json!(v.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(value_json).collect()),
        toml_edit::Value::InlineTable(table) => table_json(table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn profiles_apps_and_plugins_exclude_transport_auth_and_models() {
        let d = tempfile::tempdir().unwrap();
        write(
            &d.path().join(".codex/config.toml"),
            r#"
model = 'private-model'
[profiles.relaxed]
sandbox_mode = 'danger-full-access'
[mcp_servers.source]
env = { API_TOKEN = 'secret-value' }
default_tools_approval_mode = 'approve'
[mcp_servers.source.tools.lookup]
approval_mode = 'prompt'
[apps._default]
default_tools_approval_mode = 'prompt'
[apps.source.links.account]
default_tools_approval_mode = 'approve'
[plugins.'source@market'.mcp_servers.server]
default_tools_approval_mode = 'approve'
"#,
        );
        let out = discover(d.path(), &[]).unwrap();
        let source = out.iter().find(|s| s.kind == "codex-config").unwrap();
        let text = source.grants.to_string();
        assert!(!text.contains("secret-value"));
        assert!(!text.contains("private-model"));
        assert_eq!(
            source.grants["profiles"]["relaxed"]["sandbox_mode"],
            "danger-full-access"
        );
        assert_eq!(
            source.grants["apps"]["source"]["links"]["account"]["default_tools_approval_mode"],
            "approve"
        );
        assert_eq!(
            source.grants["plugins"]["source@market"]["mcp_servers"]["server"]
                ["default_tools_approval_mode"],
            "approve"
        );
    }

    #[test]
    fn malformed_and_unknown_are_not_treated_as_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".claude/settings.json");
        write(&p, "{");
        assert!(discover(d.path(), &[]).is_err());
        write(&p, r#"{"permissions":{"allow":42,"future":true}}"#);
        let out = discover(d.path(), &[]).unwrap();
        assert_eq!(
            out.iter()
                .find(|s| s.kind == "claude-settings")
                .unwrap()
                .unknown
                .len(),
            2
        );
    }

    #[test]
    fn unknown_nested_state_and_malformed_native_fields_are_visible() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        write(&d.path().join(".claude.json"), &json!({"projects":{project.to_str().unwrap():{"permissions":{"allow":["Bash"]},"toolPermissions":{"approve":true}}}}).to_string());
        write(
            &d.path().join(".claude/settings.json"),
            r#"{"autoMode":{"classifyAllShell":"yes","environment":{}},"sandbox":{"enabled":"yes","network":{"allowedDomains":true}},"enabledPlugins":{"plugin":true}}"#,
        );
        write(&d.path().join(".codex/config.toml"), "approvals_reviewer = 'future-mode'\n[apps.source.tools.lookup]\napproval_mode = 1\n[profiles.broken]\nsandbox_mode = true\n");
        let out = discover(d.path(), &[]).unwrap();
        let state = out.iter().find(|s| s.kind == "claude-state").unwrap();
        assert!(state
            .unknown
            .iter()
            .any(|s| s.contains("permissions semantics")));
        assert!(state
            .unknown
            .iter()
            .any(|s| s.contains("toolPermissions semantics")));
        let settings = out.iter().find(|s| s.kind == "claude-settings").unwrap();
        for field in [
            "autoMode.classifyAllShell",
            "autoMode.environment",
            "sandbox.enabled",
            "sandbox.network.allowedDomains",
        ] {
            assert!(
                settings.unknown.iter().any(|s| s.contains(field)),
                "missing {field}: {:?}",
                settings.unknown
            );
        }
        assert_eq!(settings.grants["enabledPlugins"]["plugin"], true);
        let codex = out.iter().find(|s| s.kind == "codex-config").unwrap();
        for field in ["approvals_reviewer", "approval_mode", "sandbox_mode"] {
            assert!(codex.unknown.iter().any(|s| s.contains(field)));
        }
    }

    #[test]
    fn verified_auto_mode_fields_are_typed_and_preserved_without_expanding_defaults() {
        let d = tempfile::tempdir().unwrap();
        let settings = d.path().join(".claude/settings.json");
        let config = json!({"useAutoModeDuringPlan":true,"autoMode":{"classifyAllShell":true,"environment":["$defaults","User-requested worktree operations"],"allow":["$defaults"],"soft_deny":[],"hard_deny":["$defaults"]}});
        write(&settings, &config.to_string());
        let out = discover(d.path(), &[]).unwrap();
        let source = out.iter().find(|s| s.path == settings).unwrap();
        assert!(source.unknown.is_empty(), "{:?}", source.unknown);
        assert_eq!(source.grants, config);
        write(
            &settings,
            r#"{"useAutoModeDuringPlan":"true","autoMode":{"allow":true,"soft_deny":{},"hard_deny":[42]}}"#,
        );
        let out = discover(d.path(), &[]).unwrap();
        let source = out.iter().find(|s| s.path == settings).unwrap();
        assert_eq!(source.unknown.len(), 4);
    }

    #[test]
    fn project_codex_regular_file_is_skipped_like_native_loader() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let project = d.path().join("project");
        std::fs::create_dir_all(&home).unwrap();
        write(&project.join(".codex"), "not a configuration directory\n");
        write(
            &project.join(".claude/settings.json"),
            r#"{"permissions":{"allow":[]}}"#,
        );
        let out = discover(&home, std::slice::from_ref(&project)).unwrap();
        assert!(out
            .iter()
            .any(|s| s.path == project.join(".claude/settings.json")));
        assert!(!out
            .iter()
            .any(|s| s.path.starts_with(project.join(".codex"))));
    }

    #[test]
    fn tracked_local_settings_are_retained_and_home_is_not_misclassified_as_project() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let project = home.join("project");
        let local = project.join(".claude/settings.local.json");
        let user = home.join(".claude/settings.json");
        write(&user, r#"{"permissions":{"allow":[]}}"#);
        write(&local, r#"{"permissions":{"allow":["Bash(ls)"]}}"#);
        let run = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "--quiet"]);
        run(&["add", "-f", "--", ".claude/settings.local.json"]);
        let out = discover(&home, &[project]).unwrap();
        assert!(!out.iter().find(|s| s.path == local).unwrap().writable);
        assert!(out.iter().find(|s| s.path == user).unwrap().writable);
    }

    #[test]
    fn opaque_starlark_is_unknown_not_evaluated() {
        let d = tempfile::tempdir().unwrap();
        write(
            &d.path().join(".codex/rules/default.rules"),
            "prefix_rule(pattern=['git'], decision='allow')\n",
        );
        let out = discover(d.path(), &[]).unwrap();
        let source = out.iter().find(|s| s.kind == "codex-rules").unwrap();
        assert!(!source.unknown.is_empty());
        assert_eq!(
            source.grants,
            json!({"opaque_rules":true, "rules":"prefix_rule(pattern=['git'], decision='allow')\n"})
        );
    }

    #[test]
    fn discovered_projects_include_ancestors_and_main_checkout_local() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let main = d.path().join("main");
        let worktree = d.path().join("worktree");
        let gitdir = main.join(".git/worktrees/linked");
        std::fs::create_dir_all(main.join(".git/objects")).unwrap();
        std::fs::create_dir_all(main.join(".git/refs")).unwrap();
        write(&main.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(&gitdir.join("HEAD"), "ref: refs/heads/linked\n");
        write(&gitdir.join("commondir"), "../..\n");
        write(
            &worktree.join(".git"),
            &format!("gitdir: {}\n", gitdir.display()),
        );
        // This fixture only checks worktree path resolution; source mutation
        // classification is covered separately with a real Git index.
        write(
            &main.join(".claude/settings.local.json"),
            r#"{"permissions":{"allow":["Bash(git *)"]}}"#,
        );
        write(&home.join(".claude.json"), &json!({"projects":{worktree.to_str().unwrap():{"allowedTools":["Bash(ls)"]}},"oauthAccount":{"secret":"excluded"}}).to_string());
        let out = discover(&home, &[]).unwrap();
        assert!(out
            .iter()
            .any(|s| s.path == main.join(".claude/settings.local.json") && s.writable));
        assert!(!out
            .iter()
            .any(|s| s.grants.to_string().contains("excluded")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_alias_is_deduplicated_and_never_owned() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let project = d.path().join("project");
        let settings = home.join(".claude/settings.json");
        write(&settings, r#"{"permissions":{"allow":[]}}"#);
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::os::unix::fs::symlink(&settings, project.join(".claude/settings.local.json")).unwrap();
        let out = discover(&home, &[project]).unwrap();
        let matching: Vec<_> = out.iter().filter(|s| s.path == settings).collect();
        assert_eq!(matching.len(), 1);
        assert!(!matching[0].writable);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_configuration_directory_is_never_owned() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let external = d.path().join("external");
        std::fs::create_dir_all(&home).unwrap();
        write(
            &external.join("settings.json"),
            r#"{"permissions":{"allow":[]}}"#,
        );
        std::os::unix::fs::symlink(&external, home.join(".claude")).unwrap();
        let out = discover(&home, &[]).unwrap();
        let source = out
            .iter()
            .find(|s| s.path == external.join("settings.json"))
            .unwrap();
        assert!(!source.writable);
    }
}
