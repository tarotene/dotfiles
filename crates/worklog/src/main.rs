//! worklog の CLI と SessionStart hook(docs/agents/worklog.md)。
//!
//! ```text
//! worklog add <kind> <text...> [--ref N] [--repo owner/repo]
//! worklog list [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--repo owner/repo] [--kind k[,k]]
//! worklog session-start [--agent claude|codex|copilot]   # SessionStart hook。vault を宣言していない機体では無音
//! ```
//!
//! `WORKLOG_NOW`(unix 秒)は試験用に今の時刻を固定する。

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
use worklog::{
    add, build_context, jst_parts, list, parse_date, select_recent, Added, Kind, Reference, Vault,
    SESSION_MAX_LINES, VAULT, WINDOW_DAYS,
};

fn usage() -> String {
    format!(
        "usage:\n  worklog add <kind> <text...> [--ref N] [--repo owner/repo]\n  \
         worklog list [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--repo owner/repo] [--kind k[,k]]\n  \
         worklog session-start [--agent claude|codex|copilot]\nkind: {} (list では memo も)",
        Kind::names()
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("add") => add_cmd(&args[1..]),
        Some("list") => list_cmd(&args[1..]),
        Some("session-start") => session_arg(&args[1..]).map(session_start),
        _ => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            eprintln!("worklog: {m}");
            ExitCode::from(2)
        }
    }
}

fn now_unix() -> u64 {
    std::env::var("WORKLOG_NOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        })
}

fn open_vault() -> Result<Vault, String> {
    let getenv = |k: &str| std::env::var(k).ok();
    let root = hub_resolve::resolve(&VAULT, &getenv).map_err(|m| m.trim_end().to_string())?;
    Vault::open(Path::new(&root))
}

/// 作業ディレクトリの GitHub の `owner/repo`。git の外・GitHub 以外の remote では `None`。
fn cwd_repo() -> Option<String> {
    git_repository::origin_nwo(&std::env::current_dir().ok()?)
}

/// `--flag value` の value を取る。
fn value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a str, String> {
    it.next()
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} に値が要る"))
}

fn parse_repo(s: &str) -> Result<String, String> {
    match Reference::parse(s) {
        Some(Reference { repo, number: None }) => Ok(repo),
        _ => Err(format!("--repo は owner/repo の形: '{s}'")),
    }
}

fn add_cmd(args: &[String]) -> Result<(), String> {
    let kind_s = args.first().ok_or_else(usage)?;
    let kind = Kind::parse(kind_s).ok_or_else(|| {
        format!(
            "kind が閉語彙の外: '{kind_s}'(使えるのは: {})",
            Kind::names()
        )
    })?;
    let (mut text, mut number, mut repo) = (Vec::new(), None, None);
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ref" => {
                let v = value(&mut it, "--ref")?;
                number = Some(
                    v.trim_start_matches('#')
                        .parse::<u64>()
                        .map_err(|_| format!("--ref は Issue・PR の番号: '{v}'"))?,
                );
            }
            "--repo" => repo = Some(parse_repo(value(&mut it, "--repo")?)?),
            other => text.push(other),
        }
    }
    let repo = repo.or_else(cwd_repo);
    if number.is_some() && repo.is_none() {
        return Err(
            "--ref には owner/repo が要る。git の外では --repo owner/repo を渡す".to_string(),
        );
    }
    let reference = repo.map(|repo| Reference { repo, number });

    let vault = open_vault()?;
    let (days, time) = jst_parts(now_unix());
    match add(&vault, days, &time, kind, &text.join(" "), reference)? {
        Added::Appended(p) => println!("追記した: {}", p.display()),
        Added::Duplicate(p) => println!("既に記録済み: {}", p.display()),
    }
    Ok(())
}

fn list_cmd(args: &[String]) -> Result<(), String> {
    let (mut since, mut until, mut repo, mut kinds) = (None, None, None, None::<Vec<String>>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--since" => since = Some(value(&mut it, "--since")?),
            "--until" => until = Some(value(&mut it, "--until")?),
            "--repo" => repo = Some(parse_repo(value(&mut it, "--repo")?)?),
            "--kind" => {
                let names: Vec<String> = value(&mut it, "--kind")?
                    .split(',')
                    .map(str::to_string)
                    .collect();
                if let Some(bad) = names
                    .iter()
                    .find(|n| *n != "memo" && Kind::parse(n).is_none())
                {
                    return Err(format!(
                        "kind が閉語彙の外: '{bad}'(使えるのは: {}|memo)",
                        Kind::names()
                    ));
                }
                kinds = Some(names);
            }
            _ => return Err(usage()),
        }
    }
    let date =
        |s: &str, flag: &str| parse_date(s).ok_or_else(|| format!("{flag} が日付でない: {s}"));
    let until = match until {
        Some(s) => date(s, "--until")?,
        None => jst_parts(now_unix()).0,
    };
    let since = match since {
        Some(s) => date(s, "--since")?,
        None => until - (WINDOW_DAYS - 1),
    };
    let rows = list(&open_vault()?, since, until)?;
    for r in rows.iter().filter(|r| {
        repo.as_ref()
            .is_none_or(|x| r.entry.reference.as_ref().is_some_and(|y| &y.repo == x))
            && kinds
                .as_ref()
                .is_none_or(|k| k.iter().any(|n| n == r.entry.kind_label()))
    }) {
        println!("{}", r.tsv());
    }
    Ok(())
}

/// SessionStart の出力の形。Claude Code と Codex は `hookSpecificOutput` に入れ子で読み、Copilot CLI は
/// 最上位の `additionalContext` だけを読む(GitHub Docs, "GitHub Copilot hooks reference" の
/// `sessionStart`、取得 2026-10-28)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    Nested,
    TopLevel,
}

/// `session-start [--agent claude|codex|copilot]`。既定は `claude`(入れ子)。
fn session_arg(args: &[String]) -> Result<Shape, String> {
    let mut shape = Shape::Nested;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--agent" => {
                shape = match value(&mut it, "--agent")? {
                    "claude" | "codex" => Shape::Nested,
                    "copilot" => Shape::TopLevel,
                    other => {
                        return Err(format!(
                            "--agent は claude|codex|copilot のいずれか: '{other}'"
                        ))
                    }
                };
            }
            other => return Err(format!("session-start に未知の引数: '{other}'")),
        }
    }
    Ok(shape)
}

/// 注入する JSON。`context` はそのまま `additionalContext` に入る。
fn session_output(shape: Shape, context: String) -> serde_json::Value {
    match shape {
        Shape::Nested => json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context,
            }
        }),
        Shape::TopLevel => json!({ "additionalContext": context }),
    }
}

/// SessionStart hook。常に exit 0。vault を宣言していない機体では何も出さない。
fn session_start(shape: Shape) {
    let getenv = |k: &str| std::env::var(k).ok();
    let declared = getenv(VAULT.env_var).is_some_and(|v| !v.is_empty())
        || hub_resolve::marker_path(&VAULT, &getenv).is_file();
    // 標準入力は読み切る(hook の作法)。cwd はそこから取る。
    let input = agent_session::input::read_stdin();
    if !declared {
        return;
    }
    let vault = match open_vault() {
        Ok(v) => v,
        // 宣言はあるのに使えない: 無音だと壊れた設定を隠すので 1 行だけ出す。
        Err(m) => {
            eprintln!("[worklog] {}", m.lines().next().unwrap_or(&m));
            return;
        }
    };
    let cwd: Option<PathBuf> = input
        .and_then(|i| i.project_dir())
        .or_else(|| std::env::current_dir().ok());
    let repo = cwd.and_then(|c| git_repository::origin_nwo(&c));
    let today = jst_parts(now_unix()).0;
    let rows = list(&vault, today - (WINDOW_DAYS - 1), today).unwrap_or_else(|m| {
        eprintln!("[worklog] {m}");
        Vec::new()
    });
    let recent = repo
        .as_deref()
        .map(|r| select_recent(&rows, r, SESSION_MAX_LINES))
        .unwrap_or_default();
    let out = session_output(shape, build_context(repo.as_deref(), &recent));
    println!(
        "{}",
        serde_json::to_string_pretty(&out).expect("serialize session-start output")
    );
}
