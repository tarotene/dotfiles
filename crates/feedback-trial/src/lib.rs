//! Opt-in, bounded feedback collection. Local configuration owns all destinations.
mod collector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::BufRead;
use std::os::unix::fs::PermissionsExt;
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    #[serde(default)]
    pub posting_authorization: Option<String>,
    pub start: String,
    pub end_exclusive: String,
    pub repository: String,
    pub issue: u64,
    pub report_thread: String,
    pub report_worktree: PathBuf,
    pub script: String,
}

#[derive(Default, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Unchecked,
    NoCandidate,
    AwaitingOwner,
    AwaitingPost,
    PostVerified,
}

#[derive(Default, Deserialize, Serialize)]
pub struct Check {
    #[serde(default)]
    pub observation: Value,
    pub session: String,
    pub turn: String,
    pub transcript: String,
    pub status: Status,
    pub body: String,
    pub approval: String,
    pub comment_url: String,
    pub attempts: u8,
    pub retry_turn: String,
    pub applied_commit: String,
    #[serde(default)]
    pub waiting_turn: String,
    pub usage_before: Value,
    pub usage_after: Value,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    collector: collector::Progress,
    checks: BTreeMap<String, Check>,
    finalized_commit: String,
    #[serde(default)]
    report_attempts: u8,
    #[serde(default)]
    report_retry_turn: String,
    #[serde(default)]
    report_waiting: bool,
}

/// Trial owns its existing persistent location; naming migration does not move records.
fn state_root() -> String {
    let env_nonempty = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string_lossy().into_owned())
    };
    let base = env_nonempty("WRAPUP_STATE_DIR")
        .or_else(|| env_nonempty("XDG_STATE_HOME"))
        .unwrap_or_else(|| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));
    format!("{base}/claude/wrapup")
}

fn config_path() -> PathBuf {
    std::env::var_os("WRAPUP_TRIAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
                });
            root.join("codex/feedback-trial.json")
        })
}

fn config() -> Result<Option<Config>> {
    let path = config_path();
    if !path.exists() {
        return Ok(None);
    }
    let c: Config = serde_json::from_slice(&fs::read(path)?)?;
    if !c.enabled {
        return Ok(None);
    }
    let start = agent_session::time::parse_iso_epoch(&c.start).ok_or("invalid start")?;
    let end = agent_session::time::parse_iso_epoch(&c.end_exclusive).ok_or("invalid end")?;
    if c.posting_authorization
        .as_ref()
        .is_some_and(|x| x.trim().is_empty())
        || start >= end
        || c.issue == 0
        || !c.report_worktree.is_absolute()
        || c.repository.split('/').count() != 2
        || !c
            .repository
            .chars()
            .all(|x| x.is_ascii_alphanumeric() || "-/_.".contains(x))
        || !safe_id(&c.report_thread)
        || c.script.starts_with('/')
        || c.script.split('/').any(|p| p == "..")
    {
        return Err("invalid trial configuration".into());
    }
    Ok(Some(c))
}

fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|x| x.is_ascii_alphanumeric() || "-_:".contains(x))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn read_state() -> Result<State> {
    let path = PathBuf::from(state_root()).join("trial/state.json");
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(e.into()),
    }
}

fn transaction<T>(f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let root = PathBuf::from(state_root()).join("trial");
    fs::create_dir_all(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let _lock = fs::File::options()
        .create(true)
        .append(true)
        .open(root.join("state.lock"))?;
    _lock.lock()?;
    let path = root.join("state.json");
    let mut state: State = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        State::default()
    };
    let result = f(&mut state)?;
    // Every writer uses the same migration boundary, including report operations
    // that happen before the first scheduled collection.
    let backup = root.join("state.pre-collector.json");
    if path.exists() && !backup.exists() {
        fs::copy(&path, backup)?;
    }
    let tmp = root.join("state.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&state)?)?;
    fs::rename(tmp, path)?;
    Ok(result)
}

#[derive(Default)]
struct Context {
    turn: String,
    mode: String,
    usage: Value,
}

fn context(path: &str) -> Result<Context> {
    let mut result = Context::default();
    for line in fs::read_to_string(path)?.split_inclusive('\n') {
        // A live rollout may currently be appending its final record.
        if !line.ends_with('\n') {
            break;
        }
        let v: Value = serde_json::from_str(line)?;
        let p = &v["payload"];
        if v["type"] == "turn_context" {
            result.mode = p["collaboration_mode"]["mode"]
                .as_str()
                .unwrap_or("")
                .into();
            result.turn = p["turn_id"].as_str().unwrap_or("").into();
        }
        if v["type"] == "event_msg" && p["type"] == "token_count" {
            result.usage = p["info"]["total_token_usage"].clone();
        }
    }
    Ok(result)
}

fn marker(id: &str) -> String {
    format!("<!-- codex-trial:{id} -->")
}

fn procedure(gate: &str, c: &Config) -> String {
    let approval = if c.posting_authorization.is_some() {
        "The owner has authorized observation posting for this configured trial. After drafting, post in Default mode without asking for per-observation approval. Owner impressions remain uncollected unless actually stated; never invent them. Final script and visual approval still require the owner."
    } else {
        "Present it to the owner, ask for their impression and approval, then STOP while awaiting their answer. Approval must never be inferred from silence. Drafting/revising invalidates approval; obtain explicit owner approval before posting."
    };
    let text = format!(
        r#"Trial feedback: inspect good and difficult moments, including scene, observed behavior and work impact. Never invent the owner's feelings.
Use '{gate}' --trial list to read pending checks. For each unchecked id, record either no-candidate or draft <absolute-body-file>. A draft MUST include the scene and proposed exact wording. {approval}
Commands: --trial none <id>; --trial draft <id> <file>; --trial approve <id> <same-file> <owner-verbatim-answer>; --trial dismiss <id> <owner-verbatim-rejection>; --trial post <id>.
Plan mode is read-only. All state changes and posting require Default mode. post reads back the exact body and marker; failures stay pending. Never post manually around this command.
Only the report thread may change observations. After post verification, the shared pending state hands the observation to report thread {thread}. The report thread retrieves comment URLs and its destination from --trial list and the local trial config, reads the Issue, updates the configured script in the configured worktree, preserves stable beat ids, adds the comment URL and <!-- codex-trial:ID --> marker, commits, then runs --trial applied <id> <commit>. Posting never marks report completion; only report-side saved commit acknowledgement completes it.
The report thread owns this worktree exclusively and reads pending checks on request. Ordinary conversations never collect or present trial checks. It must re-read the Issue, synthesize into S3, and acknowledge only after saving the script. Duplicates reuse the existing marker. After {end}, collect only previously unprocessed observations from within the trial period; finish pending feedback and final aggregation, script approval, storyboard and visual review using /slides. If waiting for the owner's script/visual approval, run --trial report-wait and return the question; next input/resume reopens the pending work. Finalize with --trial finalize <commit> only after that work, then disable the local config. Slack outputs are drafts only.
Trial source: {repo}#{issue}; collection starts {start}. Hook continuations are never new observations. --trial list includes token snapshots for reporting overhead; null means unmeasured. Do not count reporting as business output."#,
        thread = c.report_thread,
        end = c.end_exclusive,
        repo = c.repository,
        issue = c.issue,
        start = c.start
    );
    if std::path::Path::new(gate)
        .file_name()
        .is_some_and(|n| n == "feedback-trial")
    {
        text.replace("--trial ", "")
    } else {
        text
    }
}

/// Ordinary conversations bypass trial state entirely. Report context is advisory only.
pub fn hook(gate: &str, input: &str, start: bool) -> Result<Option<String>> {
    if std::env::var("WRAPUP_TRIAL_AGENT").as_deref() != Ok("codex") {
        return Ok(None);
    }
    let Some(c) = config()? else { return Ok(None) };
    let v: Value = serde_json::from_str(input)?;
    if v["session_id"].as_str() != Some(c.report_thread.as_str()) {
        return Ok(None);
    }
    let state = read_state()?;
    let pending = state
        .checks
        .values()
        .filter(|x| x.status != Status::NoCandidate && x.applied_commit.is_empty())
        .count();
    let final_due = now()
        >= agent_session::time::parse_iso_epoch(&c.end_exclusive).unwrap_or(i64::MAX)
        && state.finalized_commit.is_empty();
    let failure = !state.collector.last_error.is_empty();
    if pending == 0 && !final_due && !failure {
        return Ok(None);
    }
    let command = if std::path::Path::new(gate)
        .file_name()
        .is_some_and(|n| n == "feedback-trial")
    {
        ""
    } else {
        "--trial "
    };
    let message = format!("Trial report: {pending} pending; final_due={final_due}; collector_error={failure}. Read '{gate}' {command}list; full instructions: '{gate}' {command}procedure. Plan is read-only; return normally while waiting for the owner.");
    if start {
        Ok(Some(message))
    } else {
        eprintln!("{message}");
        Ok(None)
    }
}

/// Resolve the report's own rollout, never the observation's source mode.
fn report_context(c: &Config) -> Result<Context> {
    session_context(&c.report_thread)
}

fn session_context(thread: &str) -> Result<Context> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codex"));
    let mut dirs = vec![home.join("sessions"), home.join("archived_sessions")];
    let mut found = Vec::new();
    while let Some(dir) = dirs.pop() {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file()
                && entry.path().extension().is_some_and(|x| x == "jsonl")
                && entry.file_name().to_string_lossy().contains(thread)
            {
                let mut text = String::new();
                std::io::BufReader::new(fs::File::open(entry.path())?).read_line(&mut text)?;
                let is_report = text.lines().take(1).any(|line| {
                    serde_json::from_str::<Value>(line)
                        .is_ok_and(|v| v["type"] == "session_meta" && v["payload"]["id"] == thread)
                });
                if is_report {
                    found.push((entry.metadata()?.modified()?, entry.path()));
                }
            }
        }
    }
    found.sort_by_key(|x| x.0);
    let path = found
        .last()
        .ok_or("session rollout not found; mode is unconfirmed")?;
    context(path.1.to_str().ok_or("invalid report rollout path")?)
}

fn gh(args: &[&str]) -> Result<String> {
    let out = Command::new("gh").args(args).output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn comments(c: &Config) -> Result<Vec<Value>> {
    let endpoint = format!(
        "repos/{}/issues/{}/comments?per_page=100",
        c.repository, c.issue
    );
    let pages: Vec<Vec<Value>> =
        serde_json::from_str(&gh(&["api", &endpoint, "--paginate", "--slurp"])?)?;
    Ok(pages.into_iter().flatten().collect())
}

fn post(c: &Config, id: &str, check: &mut Check) -> Result<()> {
    if check.status != Status::AwaitingPost && check.status != Status::PostVerified {
        return Err("owner approval required".into());
    }
    let matches = |rows: Vec<Value>| -> Result<Option<Value>> {
        let found: Vec<_> = rows
            .into_iter()
            .filter(|v| {
                v["body"]
                    .as_str()
                    .is_some_and(|b| b.starts_with(&marker(id)))
            })
            .collect();
        if found.len() > 1 {
            return Err("duplicate markers: reconcile before retry".into());
        }
        Ok(found.into_iter().next())
    };
    let mut found = matches(comments(c)?)?;
    if found.is_none() {
        let path = PathBuf::from(state_root()).join("trial/post-body.md");
        fs::write(&path, &check.body)?;
        gh(&[
            "issue",
            "comment",
            &c.issue.to_string(),
            "-R",
            &c.repository,
            "--body-file",
            path.to_str().ok_or("invalid body path")?,
        ])?;
        found = matches(comments(c)?)?;
    }
    let comment = found.ok_or("posted comment not found on read-back")?;
    if comment["body"].as_str() != Some(check.body.as_str()) {
        return Err("read-back differs from approved body; re-confirm before posting".into());
    }
    check.comment_url = comment["html_url"]
        .as_str()
        .ok_or("missing comment URL")?
        .into();
    check.status = Status::PostVerified;
    check.usage_after = report_context(c)?.usage;
    Ok(())
}

fn saved_script(c: &Config, commit: &str) -> Result<String> {
    if commit.len() != 40 || !commit.chars().all(|x| x.is_ascii_hexdigit()) {
        return Err("full commit SHA required".into());
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(&c.report_worktree)
        .args(["show", &format!("{commit}:{}", c.script)])
        .output()?;
    if !out.status.success() {
        return Err("saved script not found in commit".into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

pub fn cli(args: &[String]) -> i32 {
    let run = || -> Result<()> {
        if args.first().map(String::as_str) == Some("collect") {
            if args.len() > 2 || args.get(1).is_some_and(|s| s != "--dry-run") {
                return Err("usage: collect [--dry-run]".into());
            }
            if args.len() == 1 {
                if let Ok(thread) = std::env::var("CODEX_THREAD_ID") {
                    if session_context(&thread)?.mode != "default" {
                        return Err("collection from a conversation requires Default mode; use --dry-run in Plan".into());
                    }
                }
            }
            return collector::collect(args.len() == 2);
        }
        let c = config()?.ok_or("trial is not configured or disabled")?;
        let action = args.first().map(String::as_str).unwrap_or("list");
        let id = args.get(1).map(String::as_str).unwrap_or("");
        let value = args.get(2).map(String::as_str).unwrap_or("");
        if action == "list" {
            println!("{}", serde_json::to_string_pretty(&read_state()?)?);
            return Ok(());
        }
        if action == "procedure" {
            println!("{}", procedure("feedback-trial", &c));
            return Ok(());
        }
        if std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&c.report_thread) {
            return Err("only the report thread may change trial state".into());
        }
        let ctx = report_context(&c)?;
        if ctx.mode != "default" {
            return Err(
                "trial state changes require confirmed Default mode; Plan is read-only".into(),
            );
        }
        let _collection_lock = if action == "finalize" {
            Some(collector::finalize_lock()?)
        } else {
            None
        };
        transaction(|state| {
            if action == "report-wait" {
                if std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&c.report_thread) {
                    return Err("only report thread may wait for report approval".into());
                }
                state.report_waiting = true;
                return Ok(());
            }
            if action == "finalize" {
                if !state.collector.last_error.is_empty() || collector::has_pending(&c, state)? {
                    return Err("collector has unread intervals; collect before finalizing".into());
                }
                if std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&c.report_thread)
                    || now()
                        < agent_session::time::parse_iso_epoch(&c.end_exclusive)
                            .ok_or("invalid end")?
                    || state
                        .checks
                        .values()
                        .any(|x| x.status != Status::NoCandidate && x.applied_commit.is_empty())
                {
                    return Err("report not ready to finalize".into());
                }
                saved_script(&c, id)?;
                state.finalized_commit = id.into();
                return Ok(());
            }
            let check = state.checks.get_mut(id).ok_or("unknown check id")?;
            match action {
                "none" if check.status == Status::Unchecked => {
                    check.status = Status::NoCandidate;
                    check.usage_after = ctx.usage.clone();
                }
                "dismiss" if check.status == Status::AwaitingOwner && !value.trim().is_empty() => {
                    check.approval = format!("Rejected: {value}");
                    check.status = Status::NoCandidate;
                    check.usage_after = ctx.usage.clone();
                }
                "draft" if check.status != Status::PostVerified => {
                    let body = fs::read_to_string(value)?;
                    if body.trim().is_empty() {
                        return Err("empty draft".into());
                    }
                    check.body = format!("{}\n{}\n\n🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)", marker(id), body.trim());
                    check.approval.clear();
                    if let Some(authorization) = &c.posting_authorization {
                        check.approval = format!("Standing authorization: {authorization}");
                        check.status = Status::AwaitingPost;
                    } else {
                        check.status = Status::AwaitingOwner;
                    }
                    check.waiting_turn = ctx.turn.clone();
                }
                "approve" if check.status == Status::AwaitingOwner => {
                    let body = fs::read_to_string(value)?;
                    let expected = format!("{}\n{}\n\n🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)", marker(id), body.trim());
                    if check.body != expected {
                        return Err("draft changed: show it and obtain approval again".into());
                    }
                    let answer = args
                        .get(3)
                        .filter(|s| !s.trim().is_empty())
                        .ok_or("verbatim owner approval required")?;
                    check.approval = answer.clone();
                    check.status = Status::AwaitingPost;
                }
                "post" => {
                    if check.status == Status::AwaitingOwner {
                        if let Some(authorization) = &c.posting_authorization {
                            check.approval = format!("Standing authorization: {authorization}");
                            check.status = Status::AwaitingPost;
                        }
                    }
                    post(&c, id, check)?;
                }
                "applied" if check.status == Status::PostVerified => {
                    if std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&c.report_thread) {
                        return Err("only report thread may acknowledge".into());
                    }
                    let script = saved_script(&c, value)?;
                    if !script.contains(&marker(id)) || !script.contains(&check.comment_url) {
                        return Err("saved script must cite this marker and comment URL".into());
                    }
                    let found = comments(&c)?
                        .into_iter()
                        .find(|v| v["html_url"].as_str() == Some(&check.comment_url))
                        .ok_or("comment missing on report read-back")?;
                    if found["body"].as_str() != Some(&check.body) {
                        return Err(
                            "comment changed; re-read and reconcile before acknowledgement".into(),
                        );
                    }
                    check.applied_commit = value.into();
                }
                _ => return Err(
                    "invalid transition; use list, none, draft, approve, post, applied, finalize"
                        .into(),
                ),
            }
            println!("{}", serde_json::to_string_pretty(check)?);
            Ok(())
        })
    };
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("trial: {e}; pending state retained");
            1
        }
    }
}
