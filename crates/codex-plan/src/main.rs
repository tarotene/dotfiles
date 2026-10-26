use anyhow::{bail, Context, Result};
use codex_plan::{runtime, Model, Settings};
use std::path::PathBuf;

const USAGE: &str = "Usage:
  codex-plan [PROMPT]
  codex-plan resume <ID|--last>
  codex-plan status
  codex-plan configure --plan-model ID --plan-effort EFFORT --execute-model ID --execute-effort EFFORT

Plan: gpt-6-astra medium; Default: gpt-6.1-sol medium (unless configured).
Settings: $XDG_CONFIG_HOME/codex-plan/settings.toml (default ~/.config).
Changes apply on the next launch. /model is respected until the next mode switch.
Native Codex owns permissions, hooks and history; no automatic model fallback.";

#[tokio::main]
async fn main() {
    let code = match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("codex-plan: {error:#}");
            1
        }
    };
    std::process::exit(code);
}

async fn run() -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("--help" | "-h" | "help")
    ) {
        println!("{USAGE}");
        return Ok(0);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("HOME or XDG_CONFIG_HOME is required")?
        .join("codex-plan/settings.toml");
    let native = PathBuf::from("codex");
    match args.first().map(String::as_str) {
        Some("status") if args.len() == 1 => {
            println!(
                "{}\n{}",
                config.display(),
                toml::to_string_pretty(&Settings::load(&config)?)?
            );
            Ok(0)
        }
        Some("configure") => {
            let mut fields = std::collections::HashMap::new();
            for pair in args[1..].chunks(2) {
                if pair.len() != 2
                    || !matches!(
                        pair[0].as_str(),
                        "--plan-model" | "--plan-effort" | "--execute-model" | "--execute-effort"
                    )
                    || fields.insert(pair[0].as_str(), pair[1].clone()).is_some()
                {
                    bail!("configure requires each of the four options exactly once\n{USAGE}");
                }
            }
            if fields.len() != 4 {
                bail!("configure requires the complete pair\n{USAGE}");
            }
            let settings = Settings {
                plan: Model {
                    model: fields["--plan-model"].clone(),
                    reasoning_effort: fields["--plan-effort"].clone(),
                },
                execute: Model {
                    model: fields["--execute-model"].clone(),
                    reasoning_effort: fields["--execute-effort"].clone(),
                },
            };
            runtime::preflight(&native, &settings).await?;
            settings.save(&config)?;
            // Report the persisted pair, not merely the write's success.
            println!(
                "Saved {} (effective next launch)\n{}",
                config.display(),
                toml::to_string_pretty(&Settings::load(&config)?)?
            );
            Ok(0)
        }
        Some("resume") if args.len() == 2 && (!args[1].starts_with('-') || args[1] == "--last") => {
            runtime::launch(&native, &Settings::load(&config)?, Some(&args[1]), None).await
        }
        Some("resume" | "status") => bail!("invalid arguments\n{USAGE}"),
        _ if args.len() <= 1 && args.first().is_none_or(|a| !a.starts_with('-')) => {
            runtime::launch(
                &native,
                &Settings::load(&config)?,
                None,
                args.first().map(String::as_str),
            )
            .await
        }
        _ => bail!("invalid arguments\n{USAGE}"),
    }
}
