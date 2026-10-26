use anyhow::{bail, Context, Result};
use codex_plan::{comparison, experiment, history, runtime, selection, threads, Model, Settings};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage:
  codex-plan [PROMPT]
  codex-plan resume <ID|--last>
  codex-plan status
  codex-plan compare [--days N] [--json]
  codex-plan configure
  codex-plan configure --plan-model ID --plan-effort EFFORT --execute-model ID --execute-effort EFFORT
  codex-plan experiment start --execute-efforts EFFORT,EFFORT[,...] --until YYYY-MM-DD
  codex-plan experiment stop

Plan / Auto (execution / native Default): gpt-6.1-sol medium (unless configured).
Settings: $XDG_CONFIG_HOME/codex-plan/settings.toml (default ~/.config).
Changes apply on the next launch. /model is respected until the next mode switch.
While an experiment runs, each fresh launch draws its execute effort from the
candidates; resume keeps the saved one. `compare` splits results by effort.
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
    // The credential launcher intentionally rejects app-server / --remote / -c.
    // This protocol adapter needs the installer-owned native entry point.
    let native = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?)
        .join(".local/bin/codex");
    match args.first().map(String::as_str) {
        Some("status") if args.len() == 1 => {
            println!(
                "{}\n{}",
                config.display(),
                toml::to_string_pretty(&Settings::load(&config)?)?
            );
            if let Some(experiment) = Settings::load(&config)?.experiment {
                println!("{}", experiment.status_line(&today()?));
            }
            Ok(0)
        }
        Some("experiment") => experiment_command(&args[1..], &config, &native).await,
        Some("compare") => {
            let mut days = 7;
            let mut json = false;
            let mut index = 1;
            while index < args.len() {
                match args[index].as_str() {
                    "--json" if !json => json = true,
                    "--days" if index + 1 < args.len() => {
                        index += 1;
                        days = args[index]
                            .parse::<u32>()
                            .context("--days requires an integer")?;
                    }
                    _ => bail!("invalid compare arguments\n{USAGE}"),
                }
                index += 1;
            }
            if days == 0 || days > 3660 {
                bail!("--days must be between 1 and 3660");
            }
            let discovery = runtime::discover(&native, true).await;
            let error = discovery.as_ref().err().map(|e| format!("{e:#}"));
            let mut report = comparison::build(
                days,
                discovery.as_ref().map_err(|_| error.as_deref().unwrap()),
            )?;
            // Settings are optional context here: a broken file must not hide the report.
            let experiment = Settings::load(&config).ok().and_then(|s| s.experiment);
            let (found, unreadable) = threads::collect(
                &comparison::codex_home()?,
                days,
                comparison::now(),
                experiment.as_ref(),
            )?;
            report["experiment_arms"] =
                threads::report(&found, unreadable, days, experiment.as_ref());
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", comparison::render(&report));
            }
            Ok(0)
        }
        Some("configure") if args.len() == 1 => {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                bail!("configure requires a TTY; nothing was saved. Use the four explicit options:\n{USAGE}");
            }
            let discovery = runtime::discover(&native, true).await;
            let error = discovery.as_ref().err().map(|e| format!("{e:#}"));
            let report =
                comparison::build(7, discovery.as_ref().map_err(|_| error.as_deref().unwrap()))?;
            print!("{}", comparison::render(&report));
            let discovery = discovery.context(
                "native model discovery failed; reference comparison only, saving disabled",
            )?;
            let current = Settings::load(&config)?;
            // Tokio 1.54.1 src/signal/unix.rs:331-340: discovery's signal
            // handlers remain process-wide even after its receivers are dropped.
            // Keep listening while a terminal read blocks on a separate worker.
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let mut hangup =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
            let models = discovery.models.clone();
            let selection = tokio::task::spawn_blocking(move || {
                selection::select(
                    &mut std::io::stdin().lock(),
                    &mut std::io::stdout().lock(),
                    &current,
                    &models,
                )
            });
            let selected = tokio::select! {
                result = selection => result.context("selection worker failed")??,
                _ = term.recv() => bail!("selection interrupted by SIGTERM; nothing saved"),
                _ = hangup.recv() => bail!("selection interrupted by SIGHUP; nothing saved"),
                _ = interrupt.recv() => bail!("selection interrupted by SIGINT; nothing saved"),
            };
            let Some(settings) = selected else {
                println!("Cancelled; nothing saved.");
                return Ok(0);
            };
            discovery.validate(&settings)?;
            settings.save(&config)?;
            println!(
                "Saved {} (effective next launch)\n{}",
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
                // A running experiment survives a pair change; a broken file has none to keep.
                experiment: Settings::load(&config).ok().and_then(|s| s.experiment),
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
            let (settings, notice) = Settings::load(&config)?
                .for_new_launch(&today()?, &mut experiment::urandom_byte)?;
            if let Some(notice) = notice {
                eprintln!("{notice}");
            }
            runtime::launch(&native, &settings, None, args.first().map(String::as_str)).await
        }
        _ => bail!("invalid arguments\n{USAGE}"),
    }
}

fn today() -> Result<String> {
    history::local_day(comparison::now())
}

async fn experiment_command(args: &[String], config: &Path, native: &Path) -> Result<i32> {
    match args.first().map(String::as_str) {
        Some("start") => {
            let mut fields = std::collections::HashMap::new();
            for pair in args[1..].chunks(2) {
                if pair.len() != 2
                    || !matches!(pair[0].as_str(), "--execute-efforts" | "--until")
                    || fields.insert(pair[0].as_str(), pair[1].clone()).is_some()
                {
                    bail!("experiment start requires --execute-efforts and --until exactly once\n{USAGE}");
                }
            }
            let (Some(efforts), Some(until)) =
                (fields.get("--execute-efforts"), fields.get("--until"))
            else {
                bail!("experiment start requires --execute-efforts and --until\n{USAGE}");
            };
            let today = today()?;
            let current = Settings::load(config)?;
            let efforts: Vec<String> = efforts.split(',').map(str::to_owned).collect();
            let experiment = match current.experiment.as_ref().filter(|e| e.is_active(&today)) {
                Some(running) => running.extend(&efforts, until)?,
                None => experiment::Experiment::start(efforts, &today, until)?,
            };
            let settings = Settings {
                experiment: Some(experiment),
                ..current
            };
            runtime::preflight(native, &settings).await?;
            settings.save(config)?;
            // Report the persisted state, not merely the write's success.
            let saved = Settings::load(config)?;
            println!(
                "Saved {}\n{}",
                config.display(),
                saved
                    .experiment
                    .context("experiment was not saved")?
                    .status_line(&today)
            );
            Ok(0)
        }
        Some("stop") if args.len() == 1 => {
            let today = today()?;
            let mut settings = Settings::load(config)?;
            let Some(experiment) = settings.experiment.as_mut() else {
                bail!("no experiment is configured");
            };
            if experiment.ended.is_some() {
                println!("{}", experiment.status_line(&today));
                return Ok(0);
            }
            experiment.ended = Some(today.clone());
            experiment.validate()?;
            settings.save(config)?;
            let saved = Settings::load(config)?;
            println!(
                "Saved {}\n{}",
                config.display(),
                saved
                    .experiment
                    .context("experiment was not saved")?
                    .status_line(&today)
            );
            Ok(0)
        }
        _ => bail!("invalid experiment arguments\n{USAGE}"),
    }
}
