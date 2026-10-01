//! github-app-snapshot — GitHub App の PEM を持てる、このリポジトリで唯一の
//! コマンド(ADR-590 D3/D5)。
//!
//! 各 owned App の登録(permissions/events)と install 先リポジトリの read-only
//! snapshot(app-snapshot.json)を `$XDG_STATE_HOME/github-audit/` に書く。
//! `github-audit` と `github-app-registry-check` はそのファイルを遅延して読み、
//! 秘密を一切見ない(ADR-436 D4 は保たれる)。
//!
//! bash 版(scripts/github-app-snapshot)からの移植(#414、ADR-0024 Stage 4e)。
//! 出力と終了コードは bash 版と一致させる。セットアップ・ローテーション:
//! docs/github-app-snapshot.md

mod api;
mod bitwarden;
mod config;
mod jwt;
mod manifest;

use config::Config;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage:
  github-app-snapshot configure-token
  github-app-snapshot clear-token
  github-app-snapshot manifest-form <name> [--out FILE]
  github-app-snapshot convert <name> <code>
  github-app-snapshot run [--out FILE]
  github-app-snapshot exec -- <cmd...>
  github-app-snapshot --selftest

The Bitwarden Secrets Manager machine account must have read-only access to
a project holding, per owned App <name>: GITHUB_APP_<NAME>_ID,
GITHUB_APP_<NAME>_PEM (NAME = <name> upper-cased, '-' -> '_'), and — for
distribution to repo secrets, not read by `run` — GITHUB_APP_<NAME>_CLIENT_ID.
The bws access token is stored only in GNOME Keyring.

Full setup + rotation: docs/github-app-snapshot.md
";

fn usage_err() -> u8 {
    eprint!("{USAGE}");
    2
}

fn fail(msg: &str) -> u8 {
    eprintln!("github-app-snapshot: {msg}");
    1
}

/// `bws run` の内側(秘密が環境に注入されている)で動く操作。
fn run_inner(cfg: &Config, args: &[String]) -> u8 {
    let Some(operation) = args.first() else {
        return usage_err();
    };
    let rest = &args[1..];
    match operation.as_str() {
        "snapshot" => {
            let out_path = if rest.first().map(String::as_str) == Some("--out") {
                match rest.get(1) {
                    Some(p) => PathBuf::from(p),
                    // bash 版は `$2` が無いと set -u の unbound variable で終了コード 1。
                    None => return fail("--out: missing value"),
                }
            } else {
                cfg.state_dir.join("app-snapshot.json")
            };
            match api::do_snapshot(cfg, &out_path) {
                Ok(()) => 0,
                Err(e) => fail(&e),
            }
        }
        "exec" => {
            if rest.is_empty() {
                return usage_err();
            }
            let err = Command::new(&rest[0]).args(&rest[1..]).exec();
            eprintln!("github-app-snapshot: {}: {err}", rest[0]);
            if err.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }
        }
        other => {
            eprintln!("github-app-snapshot: unknown inner operation: {other}");
            2
        }
    }
}

fn run(args: &[String]) -> u8 {
    let cfg = Config::from_env();
    let Some(cmd) = args.first() else {
        return usage_err();
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "configure-token" => bitwarden::configure_token(&cfg),
        "clear-token" => bitwarden::clear_token(&cfg),
        "manifest-form" => match rest.split_first() {
            Some((name, tail)) => match manifest::manifest_form(&cfg, name, tail) {
                Ok(()) => 0,
                Err(e) => fail(&e),
            },
            None => usage_err(),
        },
        "convert" => match rest {
            [name, code] => match manifest::convert_manifest(&cfg, name, code) {
                Ok(()) => 0,
                Err(e) => fail(&e),
            },
            _ => usage_err(),
        },
        "run" => bitwarden::run_with_bitwarden(&cfg, "snapshot", rest),
        "exec" => {
            if rest.first().map(String::as_str) != Some("--") || rest.len() < 2 {
                return usage_err();
            }
            bitwarden::run_with_bitwarden(&cfg, "exec", &rest[1..])
        }
        "__run" => {
            if rest.is_empty() {
                return usage_err();
            }
            run_inner(&cfg, rest)
        }
        _ => usage_err(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}
