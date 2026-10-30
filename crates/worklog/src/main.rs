//! worklog の CLI と SessionStart hook(docs/agents/worklog.md)。
//!
//! ```text
//! worklog add <kind> <text...> [--ref N] [--repo owner/repo]
//! worklog list [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--repo owner/repo] [--kind k[,k]]
//! worklog mail open|check|sweep [--today YYYY-MM-DD] [--dry-run]|restore <鍵>|forget <鍵>
//! worklog add slot plan <鍵> --start <ISO> --end <ISO> [--issue owner/repo#N …]
//! worklog add slot resched <鍵> --start <ISO> --end <ISO>
//! worklog add slot done|drop <鍵>
//! worklog slot open [--json]|overdue [--now <ISO>] [--json]|check
//! worklog session-start [--agent claude|codex|copilot]   # SessionStart hook。vault を宣言していない機体では無音
//! ```
//!
//! `WORKLOG_NOW`(unix 秒)は試験用に今の時刻を固定する。`WORKLOG_STATE_DIR` は終端の控えの置き場
//! (既定 `~/.local/state/claude/worklog`)。

use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
use worklog::mail::{self, Actor, Ledger, Vocabulary};
use worklog::slot::{self, SlotVocabulary};
use worklog::{
    add, build_context, format_date, jst_parts, list, parse_date, select_recent, Added, Kind,
    Reference, Vault, MAIL_CHANNELS, MAIL_SUMMARY_MAX_CHARS, SESSION_MAX_LINES, VAULT, WINDOW_DAYS,
};

fn usage() -> String {
    format!(
        "usage:\n  worklog add <kind> <text...> [--ref N] [--repo owner/repo]\n  \
         worklog list [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--repo owner/repo] [--kind k[,k]]\n  \
         worklog mail open|check\n  worklog mail sweep [--today YYYY-MM-DD] [--dry-run]\n  \
         worklog mail restore <鍵>|forget <鍵>\n  \
         worklog add slot plan <鍵> --start <ISO 時刻> --end <ISO 時刻> [--issue owner/repo#N …]\n  \
         worklog add slot resched <鍵> --start <ISO 時刻> --end <ISO 時刻>\n  \
         worklog add slot done|drop <鍵>   (出来事: {})\n  \
         worklog slot open [--json]\n  worklog slot overdue [--now <ISO 時刻>] [--json]\n  worklog slot check\n  \
         worklog session-start [--agent claude|codex|copilot]\nkind: {} (list では memo も)\n\
         mail の本文: '<{}>: <要旨(結論だけ・{}字まで)> [<{}>:<スレッド>/<受信> [due:YYYY-MM-DD]]'\
         (媒体 none は '[none]'、move は to:<owner/repo#N|cal:YYYY-MM-DD|mail:<鍵>> が要る。\
         受信は gmail ならメッセージ ID、それ以外は YYYYMMDDTHHMM)",
        SlotVocabulary::get().event_names(),
        Kind::names(),
        Vocabulary::get().event_names(),
        MAIL_SUMMARY_MAX_CHARS,
        MAIL_CHANNELS.join("|")
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("add") => add_cmd(&args[1..]).map(|()| ExitCode::SUCCESS),
        Some("list") => list_cmd(&args[1..]).map(|()| ExitCode::SUCCESS),
        Some("mail") => mail_cmd(&args[1..]),
        Some("slot") => slot_cmd(&args[1..]),
        Some("session-start") => session_arg(&args[1..])
            .map(session_start)
            .map(|()| ExitCode::SUCCESS),
        _ => Err(usage()),
    };
    match result {
        Ok(code) => code,
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
    if kind == Kind::Slot {
        return add_slot_cmd(&args[1..]);
    }
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
    // mail の出典は本文の印で、リポジトリには結びつかない(作業ディレクトリから参照を付けない)。
    // 状態の検査(畳み込み・終端の控え)は mail::add_mail が担う。
    if kind == Kind::Mail {
        if repo.is_some() || number.is_some() {
            return Err("mail には owner/repo の参照を付けない(出典は本文の印)".into());
        }
        let vault = open_vault()?;
        let (days, time) = jst_parts(now_unix());
        let ledger = Ledger::from_env()?;
        let r = mail::add_mail(&vault, &ledger, days, &time, &text.join(" "), Actor::Agent)?;
        for old in &r.superseded {
            println!("同じスレッドの古い項目を移した: {old}");
        }
        match (r.duplicate, r.path) {
            (true, p) => println!(
                "既に記録済み: {}",
                p.map_or("同じ出来事は畳み込み済み".to_string(), |p| p
                    .display()
                    .to_string())
            ),
            (false, p) => println!(
                "追記した: {}",
                p.map_or(String::new(), |p| p.display().to_string())
            ),
        }
        return Ok(());
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

/// `add slot <出来事> <鍵> [--start ISO --end ISO] [--issue owner/repo#N …]`。
/// 本文は正規の形に組み立てて `slot::add_slot` に渡す(状態の検査はそこ)。
fn add_slot_cmd(args: &[String]) -> Result<(), String> {
    let (mut positional, mut start, mut end) = (Vec::new(), None, None);
    let mut issues = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--start" => start = Some(value(&mut it, "--start")?),
            "--end" => end = Some(value(&mut it, "--end")?),
            "--issue" => issues.push(value(&mut it, "--issue")?),
            "--ref" | "--repo" => {
                return Err("slot には owner/repo の参照を付けない(参照 Issue は --issue)".into())
            }
            other => positional.push(other),
        }
    }
    let [event, key] = positional[..] else {
        return Err(format!(
            "slot は `<出来事> <鍵>` の 2 つが要る(出来事: {})",
            SlotVocabulary::get().event_names()
        ));
    };
    let mut tag = Vec::new();
    tag.extend(start.map(|s| format!("start:{s}")));
    tag.extend(end.map(|s| format!("end:{s}")));
    tag.extend(issues.iter().map(|i| format!("issue:{i}")));
    let mut text = format!("{event}: {key}");
    if !tag.is_empty() {
        text.push_str(&format!(" [{}]", tag.join(" ")));
    }
    let line = slot::parse_slot(&text)?;
    let vault = open_vault()?;
    let (days, time) = jst_parts(now_unix());
    let r = slot::add_slot(&vault, days, &time, &slot::format_slot(&line))?;
    if r.duplicate {
        println!("既に記録済み: {}", r.path.display());
    } else {
        println!("追記した: {}", r.path.display());
    }
    Ok(())
}

/// `slot open|overdue|check`。`check` は食い違いがあれば exit 1。
fn slot_cmd(args: &[String]) -> Result<ExitCode, String> {
    let sub = args.first().map(String::as_str).ok_or_else(usage)?;
    let (mut json, mut now) = (false, None);
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match (sub, a.as_str()) {
            ("open" | "overdue", "--json") => json = true,
            ("overdue", "--now") => now = Some(slot::parse_instant(value(&mut it, "--now")?)?),
            _ => return Err(usage()),
        }
    }
    let vault = open_vault()?;
    // 「今」は --now があればそれ。読むノートも、その日(JST)までにする。
    let now_epoch = now.map_or_else(|| now_unix() as i64, |n| n.epoch);
    let until = jst_parts(u64::try_from(now_epoch).map_err(|_| "--now が 1970 年より前")?).0;
    let (_, folded) = slot::load(&vault, until)?;
    let print = |slots: Vec<&slot::Slot>| {
        if json {
            let a: Vec<_> = slots.iter().map(|s| s.to_json()).collect();
            println!("{}", serde_json::Value::Array(a));
        } else {
            for s in slots {
                println!("{}", s.tsv());
            }
        }
    };
    match sub {
        "open" => print(folded.open()),
        "overdue" => print(folded.overdue(now_epoch)),
        "check" => {
            for d in &folded.discrepancies {
                println!("食い違い\t{}\t{}", d.key, d.message);
            }
            if !folded.discrepancies.is_empty() {
                return Ok(ExitCode::from(1));
            }
        }
        _ => return Err(usage()),
    }
    Ok(ExitCode::SUCCESS)
}

/// `mail open|check|sweep|restore|forget`。`check` は食い違いがあれば exit 1。
fn mail_cmd(args: &[String]) -> Result<ExitCode, String> {
    let sub = args.first().map(String::as_str).ok_or_else(usage)?;
    let rest = &args[1..];
    let vault = open_vault()?;
    let (today, time) = jst_parts(now_unix());
    let ledger = Ledger::from_env()?;
    match sub {
        "open" if rest.is_empty() => {
            let (_, folded) = mail::load(&vault, today)?;
            let entries = ledger.entries()?;
            let vanished = mail::deleted_terminals(&folded, &entries);
            let skip: BTreeSet<&str> = vanished.iter().map(|(k, _, _)| k.as_str()).collect();
            let v = Vocabulary::get();
            for i in folded.items.values().filter(|i| !v.is_terminal(&i.state)) {
                if skip.contains(i.key.as_str()) {
                    continue;
                }
                println!(
                    "{}\t{}\t{}日\t{}\t{}",
                    i.state,
                    i.key,
                    today - i.last_day,
                    i.due.map_or("-".to_string(), format_date),
                    i.summary
                );
            }
            for (key, was, now) in &vanished {
                println!("終端の行が消えた\t{key}\t控え: {was} / 今: {now}\t-\t`worklog mail restore` か `forget`");
            }
            Ok(ExitCode::SUCCESS)
        }
        "check" if rest.is_empty() => {
            let (_, folded) = mail::load(&vault, today)?;
            let vanished = mail::deleted_terminals(&folded, &ledger.entries()?);
            for d in &folded.discrepancies {
                println!("食い違い\t{}\t{}", d.key, d.message);
            }
            for (key, was, now) in &vanished {
                println!("食い違い\t{key}\t終端の行が消えた(控え: {was} / 今: {now})");
            }
            let n = folded.discrepancies.len() + vanished.len();
            Ok(if n == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "sweep" => {
            let (mut dry_run, mut day) = (false, today);
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--dry-run" => dry_run = true,
                    "--today" => {
                        let s = value(&mut it, "--today")?;
                        day = parse_date(s).ok_or_else(|| format!("--today が日付でない: {s}"))?;
                    }
                    _ => return Err(usage()),
                }
            }
            for a in mail::sweep(&vault, &ledger, day, &time, dry_run)? {
                println!("{}\t{}\t{}", a.event, a.key, a.text);
            }
            Ok(ExitCode::SUCCESS)
        }
        "restore" | "forget" if rest.len() == 1 => {
            let key = rest[0].as_str();
            let entries = ledger.entries()?;
            let entry = entries
                .get(key)
                .ok_or_else(|| format!("終端の控えに無い鍵: {key}"))?;
            if sub == "forget" {
                ledger.forget(key)?;
                println!("控えから捨てた: {key}");
            } else {
                mail::add_mail(&vault, &ledger, today, &time, &entry.text, Actor::Machine)?;
                println!("終端の行を書き戻した: {key}");
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(usage()),
    }
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
