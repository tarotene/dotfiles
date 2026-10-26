//! Narrow credential entry point; the native installer retains its binary.
//! Argument vocabulary: OpenAI Codex rust-v0.164.0 (retrieved 2026-10-26).
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

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

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let invocation = classify(&args)?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    if !home.is_absolute() {
        return Err("HOME must be absolute".into());
    }
    let native = home.join(".local/bin/codex");
    let real = std::fs::canonicalize(&native)
        .map_err(|_| "native Codex is missing; install it using the native installer")?;
    if real == std::env::current_exe().map_err(|_| "cannot resolve launcher executable")? {
        return Err("native Codex points back to the launcher".into());
    }
    let mut child = clean_command(&native);
    child.args(&args);
    if invocation.protected {
        for key in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "CODEX_PROFILE"] {
            if std::env::var_os(key).is_some() {
                return Err("alternate config environment is not inventoried".into());
            }
        }
        // Updates can introduce new subcommands / permission flags. Review the
        // parser and repeat acceptance tests before attaching credentials again.
        let version = clean_command(&native)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "cannot inspect native Codex version")?;
        if !version.status.success() || version.stdout != b"codex-cli 0.164.0\n" {
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
        let checked = clean_command(&home.join(".local/bin/agent-permissions"))
            .args(["check", "--project"])
            .arg(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "cannot run agent-permissions check")?;
        if !checked.success() {
            return Err(
                "permission check failed; run agent-permissions inspect/check before launching"
                    .into(),
            );
        }
        // gh diagnostics can contain credentials: capture stdout, discard stderr.
        let output = clean_command(Path::new("gh"))
            .args(["auth", "token", "--hostname", "github.com"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "cannot run gh auth token; install/authenticate gh first")?;
        if !output.status.success() {
            return Err("GitHub authentication failed; run gh auth login --hostname github.com in your terminal".into());
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
    Err(format!("cannot execute native Codex: {}", child.exec()))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("codex-launcher: {e}");
            ExitCode::FAILURE
        }
    }
}
