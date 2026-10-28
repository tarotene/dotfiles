//! Narrow credential entry point; the native installer retains its binary.
//! Argument vocabulary: OpenAI Codex rust-v0.165.0 (retrieved 2026-10-28).
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Invocation {
    protected: bool,
    cwd: Option<PathBuf>,
}

// Closed vocabulary. Values are data, not options; `--` ends option parsing.
fn classify(args: &[OsString]) -> Result<Invocation, &'static str> {
    if args.len() == 1
        && ["--help", "-h", "--version", "-V"]
            .iter()
            .any(|s| args[0] == *s)
    {
        return Ok(Invocation {
            protected: false,
            cwd: None,
        });
    }
    if args.first().is_some_and(|a| {
        ["help", "completion", "update", "login", "logout"]
            .iter()
            .any(|s| a == s)
    }) {
        return Ok(Invocation {
            protected: false,
            cwd: None,
        });
    }
    let mut cwd = None;
    let mut command = "";
    let mut positional = false;
    let mut informational = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i]
            .to_str()
            .ok_or("non-UTF-8 arguments are unclassified")?;
        if a == "--" {
            break;
        }
        if a.starts_with('-') && a != "-" {
            let (flag, inline) = a.split_once('=').map_or((a, None), |(k, v)| (k, Some(v)));
            match flag {
                "--help" | "-h" | "--version" | "-V" if inline.is_none() => informational = true,
                "--model" | "-m" | "--image" | "-i" | "--cd" | "-C"
                | "--output-last-message" | "-o" | "--output-schema" | "--color"
                | "--base" | "--commit" | "--title" => {
                    let value = match inline {
                        Some(v) => OsString::from(v),
                        None => { i += 1; args.get(i).ok_or("missing option value")?.clone() }
                    };
                    // Do not mistake a native option for a consumed value.
                    if value.to_string_lossy().starts_with('-') { return Err("ambiguous option value"); }
                    if ["--cd", "-C"].contains(&flag) {
                        if cwd.is_some() { return Err("multiple working directories are unclassified"); }
                        cwd = Some(PathBuf::from(value));
                    }
                }
                "--strict-config" | "--search" | "--no-alt-screen" | "--no-daemon"
                | "--json" | "--experimental-json" | "--skip-git-repo-check"
                | "--ephemeral" | "--last" | "--all" | "--uncommitted"
                | "--include-non-interactive" if inline.is_none() => {}
                _ => return Err("unclassified or permission-changing option; use the native binary without launcher credentials for special invocations"),
            }
        } else if !positional {
            if command.is_empty() {
                match a {
                    "exec" | "e" | "resume" | "fork" | "review" | "sandbox" => command = a,
                    "agents"
                    | "tcp-tunnel"
                    | "mcp"
                    | "plugin"
                    | "app-server"
                    | "remote-control"
                    | "app"
                    | "doctor"
                    | "debug"
                    | "execpolicy"
                    | "apply"
                    | "a"
                    | "queue"
                    | "archive"
                    | "delete"
                    | "migrate-rollouts"
                    | "migrate-nfs-db"
                    | "unarchive"
                    | "cloud"
                    | "cloud-tasks"
                    | "responses-api-proxy"
                    | "stdio-to-uds"
                    | "exec-server"
                    | "features"
                    | "help"
                    | "completion"
                    | "update"
                    | "login"
                    | "logout" => {
                        return Err("unclassified command; call the native binary directly")
                    }
                    _ => positional = true,
                }
            } else if ["exec", "e"].contains(&command) && ["resume", "fork", "review"].contains(&a)
            {
                command = "exec-subcommand";
            } else {
                positional = true;
            }
        }
        i += 1;
    }
    if command == "sandbox" && !args.iter().any(|a| a == "--") {
        return Err("sandbox commands require -- before the child command");
    }
    Ok(Invocation {
        protected: !informational,
        cwd,
    })
}

const TOKEN_VARS: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

fn clean_command(path: &Path) -> Command {
    let mut c = Command::new(path);
    for key in TOKEN_VARS {
        c.env_remove(key);
    }
    c
}

/// Protection-changing config is rejected both at startup and on per-thread RPCs.
/// App-server request overrides follow CLI overrides in the pinned native loader.
pub fn overrides_protections(value: &toml::Value) -> bool {
    let Some(table) = value.as_table() else {
        return true;
    };
    table.iter().any(|(key, value)| {
        [
            "default_permissions",
            "permissions",
            "sandbox_mode",
            "sandbox_workspace_write",
            "approval_policy",
            "approvals_reviewer",
            "profile",
            "shell_environment_policy",
        ]
        .iter()
        .any(|protected| key == protected || key.starts_with(&format!("{protected}.")))
            || ["features.network_proxy", "features.credential_masking"]
                .iter()
                .any(|protected| key == protected || key.starts_with(&format!("{protected}.")))
            || (key == "features"
                && value.as_table().is_none_or(|features| {
                    features.contains_key("network_proxy")
                        || features.contains_key("credential_masking")
                }))
            || key.starts_with("profiles.")
            || (key == "profiles"
                && value
                    .as_table()
                    .is_none_or(|profiles| profiles.values().any(overrides_protections)))
    })
}

/// Recheck native protections for the cwd a credential-bearing thread will use.
/// A resumed thread's saved cwd must never be assumed equal to the launch cwd.
pub fn validate_working_directory(cwd: &Path) -> Result<String, String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let cwd = std::fs::canonicalize(cwd).map_err(|_| "cannot resolve working directory")?;
    validate_config(&home, &cwd)
}

// OpenAI Codex rust-v0.165.0 core/src/config/mod.rs (0.164.0 lines 577-604,2652-2700; no change to the checked keys)
// and config/src/permissions_toml.rs:121-127 (retrieved 2026-10-28).
// https://github.com/openai/codex/tree/360319d8dee62d87e2d59fc0c21805d79519cfed/codex-rs
// Validate native protection settings only; no ownership record or other
// worktree's state participates in credential delivery.
fn validate_config(home: &Path, cwd: &Path) -> Result<String, String> {
    fn read(path: &Path) -> Result<toml::Value, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|_| format!("cannot read native config {}", path.display()))?;
        toml::from_str(&text).map_err(|_| format!("invalid native config {}", path.display()))
    }
    let config = read(&home.join(".codex/config.toml"))?;
    for key in ["network_proxy", "credential_masking"] {
        if config
            .get("features")
            .and_then(|f| f.get(key))
            .and_then(toml::Value::as_bool)
            != Some(true)
        {
            return Err(format!(
                "native protective config requires features.{key} = true"
            ));
        }
    }
    if config.get("approval_policy").and_then(toml::Value::as_str) != Some("on-request")
        || config
            .get("approvals_reviewer")
            .and_then(toml::Value::as_str)
            != Some("auto_review")
    {
        return Err(
            "native protective config requires on-request approval with auto_review".into(),
        );
    }
    if config
        .get("shell_environment_policy")
        .and_then(|policy| policy.get("set"))
        .and_then(toml::Value::as_table)
        .is_some_and(|values| {
            values.keys().any(|key| {
                TOKEN_VARS
                    .iter()
                    .any(|token| key.eq_ignore_ascii_case(token))
            })
        })
    {
        return Err("shell environment must not replace GitHub credentials or introduce conflicting provider keys".into());
    }
    let selected = config
        .get("default_permissions")
        .and_then(toml::Value::as_str)
        .ok_or("native protective config requires default_permissions")?;
    let profile = config
        .get("permissions")
        .and_then(|p| p.get(selected))
        .ok_or("native protective config selected profile is missing")?;
    if profile.get("extends").and_then(toml::Value::as_str) != Some(":workspace")
        || profile
            .get("network")
            .and_then(|n| n.get("enabled"))
            .and_then(toml::Value::as_bool)
            != Some(true)
        || config.get("sandbox_mode").is_some()
        || config.get("profile").is_some()
    {
        return Err("native protective config requires a network-enabled :workspace profile and no legacy/profile override".into());
    }
    if config
        .get("profiles")
        .and_then(toml::Value::as_table)
        .is_some_and(|profiles| profiles.values().any(overrides_protections))
    {
        return Err("native config profiles must not override credential protections".into());
    }
    // Native loader discovers ancestor project config. Treat even untrusted
    // project layers conservatively: trust can change independently of this run.
    for ancestor in cwd.ancestors() {
        let path = ancestor.join(".codex/config.toml");
        if path == home.join(".codex/config.toml") {
            continue;
        }
        if path.exists() && overrides_protections(&read(&path)?) {
            return Err(format!(
                "project config overrides native credential protections: {}",
                path.display()
            ));
        }
    }
    Ok(selected.to_owned())
}

/// The only credential-bearing entry points are classified CLI sessions and the
/// fixed stdio app-server. Discovery and the remote TUI never inherit tokens.
pub enum Launch {
    Cli(Vec<OsString>),
    SessionServer,
    DiscoveryServer,
    RemoteTui(Vec<OsString>),
}

/// Construct the child only after validation and fresh credential acquisition.
/// The returned command must stay in the parent; never format it for logging.
pub fn command(native: &Path, launch: Launch) -> Result<Command, String> {
    let (args, invocation) = match launch {
        Launch::Cli(args) => {
            let invocation = classify(&args)?;
            (args, invocation)
        }
        Launch::SessionServer | Launch::DiscoveryServer => {
            let protected = matches!(launch, Launch::SessionServer);
            (
                vec!["app-server".into(), "--listen".into(), "stdio://".into()],
                Invocation {
                    protected,
                    cwd: None,
                },
            )
        }
        Launch::RemoteTui(args) => (
            args,
            Invocation {
                protected: false,
                cwd: None,
            },
        ),
    };
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    if !home.is_absolute() {
        return Err("HOME must be absolute".into());
    }
    let real = std::fs::canonicalize(native)
        .map_err(|_| "native Codex is missing; install it using the native installer")?;
    if real == std::env::current_exe().map_err(|_| "cannot resolve launcher executable")? {
        return Err("native Codex points back to the launcher".into());
    }
    let mut child = clean_command(native);
    if invocation.protected {
        for key in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "CODEX_PROFILE"] {
            if std::env::var_os(key).is_some() {
                return Err("alternate config environment is not inventoried".into());
            }
        }
        // Updates can introduce new subcommands / permission flags. Review the
        // parser and repeat acceptance tests before attaching credentials again.
        let version = clean_command(native)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "cannot inspect native Codex version")?;
        if !version.status.success() || version.stdout != b"codex-cli 0.165.0\n" {
            return Err(
                "unreviewed Codex version; update launcher vocabulary and repeat acceptance tests"
                    .into(),
            );
        }
        let cwd = std::fs::canonicalize(
            invocation
                .cwd
                .unwrap_or(std::env::current_dir().map_err(|_| "cannot read working directory")?),
        )
        .map_err(|_| "cannot resolve working directory")?;
        let selected = validate_config(&home, &cwd)?;
        // A resumed session can retain a permission selection. Explicitly select
        // the validated profile so its managed sandbox/proxy remains active.
        child
            .arg("--config")
            .arg(format!("default_permissions={selected:?}"));
        // gh diagnostics can contain credentials: capture stdout, discard stderr.
        let output = clean_command(Path::new("gh"))
            .args(["auth", "token", "--hostname", "github.com"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "cannot run gh auth token; install/authenticate gh first")?;
        if !output.status.success() {
            return Err("GitHub authentication failed; run gh auth login --hostname github.com in your terminal, then relaunch or resume the session to acquire credentials; no child session was started".into());
        }
        let token =
            String::from_utf8(output.stdout).map_err(|_| "invalid GitHub credential encoding")?;
        let token = token.trim();
        if token.is_empty() || token.contains(char::is_whitespace) || token.contains('\0') {
            return Err("GitHub credential is empty or invalid".into());
        }
        // The pinned broker recognizes these shapes (github.rs:14–16). Reject
        // anything it would leave raw rather than silently relying on masking.
        if token.len() < 40
            || !["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
                .iter()
                .any(|prefix| token.starts_with(prefix))
        {
            return Err(
                "GitHub credential shape is not supported by the reviewed masking provider".into(),
            );
        }
        child.env("GH_TOKEN", token);
    }
    child.args(&args);
    Ok(child)
}
