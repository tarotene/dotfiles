//! Opt-in, bounded feedback collection. Local configuration owns all destinations.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
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
    pub delivery_turn: String,
    pub usage_before: Value,
    pub usage_after: Value,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    checks: BTreeMap<String, Check>,
    finalized_commit: String,
    #[serde(default)]
    report_attempts: u8,
    #[serde(default)]
    report_retry_turn: String,
}

fn config_path() -> PathBuf {
    std::env::var_os("WRAPUP_TRIAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(super::home()).join(".config"));
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
    let start = super::retro::parse_iso_epoch(&c.start).ok_or("invalid start")?;
    let end = super::retro::parse_iso_epoch(&c.end_exclusive).ok_or("invalid end")?;
    if start >= end
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

fn active(c: &Config, time: i64) -> bool {
    super::retro::parse_iso_epoch(&c.start).is_some_and(|s| s <= time)
        && super::retro::parse_iso_epoch(&c.end_exclusive).is_some_and(|e| time < e)
}

fn transaction<T>(f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let root = PathBuf::from(super::state_root()).join("trial");
    fs::create_dir_all(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let _lock = super::lock(root.join("state.lock").to_str().ok_or("invalid path")?)?;
    let path = root.join("state.json");
    let mut state: State = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        State::default()
    };
    let result = f(&mut state)?;
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
    for line in fs::read_to_string(path)?.lines() {
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
    format!(
        r#"Trial feedback: inspect good and difficult moments, including scene, observed behavior and work impact. Never invent the owner's feelings.
Use '{gate}' --trial list to read pending checks. For each unchecked id, record either no-candidate or draft <absolute-body-file>. A draft MUST include the scene and proposed exact wording. Present it to the owner, ask for their impression and approval, then STOP while awaiting their answer. Approval must never be inferred from silence.
Commands: --trial none <id>; --trial draft <id> <file>; --trial approve <id> <same-file> <owner-verbatim-answer>; --trial post <id>.
Drafting/revising invalidates approval. Plan mode permits drafts only. Do not post until explicit owner approval in Default mode. post reads back the exact body and marker; failures stay pending. Never post manually around this command.
After post verification, use send_message_to_thread to send ONLY pending ids and comment URLs to report thread {thread}; request: read the Issue, update {script} in {worktree}, preserve stable beat ids and add the comment URL and <!-- codex-trial:ID --> marker, commit, then --trial applied <id> <commit>. Do not send the feedback body. Send one batch per normal input/resume, not a new production thread. After successful sending run --trial delivered <id>; it suppresses repeats in this turn only, NEVER marks completion. Delivery success is NOT completion; only report-side saved commit acknowledgement completes it. Delivery failures remain pending.
The report thread owns this worktree exclusively and reads all pending checks on start/stop. It must re-read the Issue, synthesize into S3, and acknowledge only after saving the script. Duplicates reuse the existing marker. After {end}, collect no new observations; finish pending feedback and final aggregation, script approval, storyboard and visual review using /slides. Finalize with --trial finalize <commit> only after that work, then disable the local config. Slack outputs are drafts only.
Trial source: {repo}#{issue}; collection starts {start}. Hook continuations are never new observations. --trial list includes token snapshots for reporting overhead; null means unmeasured. Do not count reporting as business output."#,
        thread = c.report_thread,
        script = c.script,
        worktree = c.report_worktree.display(),
        end = c.end_exclusive,
        repo = c.repository,
        issue = c.issue,
        start = c.start
    )
}

/// Return a bounded continuation, or advisory-only context on session start.
pub fn hook(gate: &str, input: &str, start: bool) -> Result<Option<String>> {
    // Explicit wiring prevents Claude/Copilot sessions from opting in accidentally.
    if std::env::var("WRAPUP_TRIAL_AGENT").as_deref() != Ok("codex") {
        return Ok(None);
    }
    let Some(c) = config()? else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(input)?;
    let sid = v["session_id"].as_str().ok_or("missing session_id")?;
    if !safe_id(sid) {
        return Err("invalid session_id".into());
    }
    let path = v["transcript_path"].as_str().unwrap_or("");
    let ctx = context(path).unwrap_or_default();
    let turn = v["turn_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(&ctx.turn);
    let continuation = v["stop_hook_active"] == true;
    transaction(|state| {
        if sid == c.report_thread {
            let pending: Vec<_> = state
                .checks
                .iter()
                .filter(|(_, x)| x.status == Status::PostVerified && x.applied_commit.is_empty())
                .map(|(id, x)| json!({"id":id,"url":x.comment_url}))
                .collect();
            let final_due = now()
                >= super::retro::parse_iso_epoch(&c.end_exclusive).unwrap_or(i64::MAX)
                && state.finalized_commit.is_empty();
            if pending.is_empty() && !final_due {
                return Ok(None);
            }
            if start && v["source"] != "compact" || !continuation && state.report_retry_turn != turn
            {
                state.report_attempts = 0;
                state.report_retry_turn = turn.into();
            }
            if !start {
                if ctx.mode != "default" || state.report_attempts >= 3 {
                    eprintln!("trial report pending: {}; final_due={final_due}; retry next normal input/resume", json!(pending));
                    return Ok(None);
                }
                state.report_attempts += 1;
            }
            return Ok(Some(format!(
                "Report updates: {}. final_due={final_due}\n{}",
                json!(pending),
                procedure(gate, &c)
            )));
        }
        if !start && !continuation && active(&c, now()) && safe_id(turn) {
            let id = format!("{sid}:{turn}");
            // A confirmation turn resolves its open candidate instead of creating another.
            if !state.checks.values().any(|x| {
                x.session == sid
                    && matches!(
                        x.status,
                        Status::Unchecked | Status::AwaitingOwner | Status::AwaitingPost
                    )
            }) {
                state.checks.entry(id).or_insert_with(|| Check {
                    session: sid.into(),
                    turn: turn.into(),
                    transcript: path.into(),
                    usage_before: ctx.usage.clone(),
                    ..Check::default()
                });
            }
        }
        let mut due = Vec::new();
        for (id, check) in &mut state.checks {
            if check.session != sid
                || check.status == Status::NoCandidate
                || !check.applied_commit.is_empty()
            {
                continue;
            }
            if start && v["source"] != "compact" || !continuation && check.retry_turn != turn {
                check.attempts = 0;
                check.retry_turn = turn.into();
                check.delivery_turn.clear();
            }
            // Waiting for a human and Plan mode can end normally.
            if !start
                && (check.status == Status::AwaitingOwner
                    || ctx.mode != "default" && check.status != Status::Unchecked
                    || check.status == Status::PostVerified && check.delivery_turn == turn)
            {
                continue;
            }
            if !start && check.attempts >= 3 {
                eprintln!("trial: {id} still pending after 3 continuations; retry on next normal input/resume");
                continue;
            }
            if !start {
                check.attempts += 1;
            }
            due.push(id.clone());
        }
        if due.is_empty() && !(start && active(&c, now())) {
            return Ok(None);
        }
        Ok(Some(format!(
            "Pending trial checks: {}\n{}",
            json!(due),
            procedure(gate, &c)
        )))
    })
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
    if context(&check.transcript)?.mode != "default" {
        return Err("posting requires confirmed Default mode".into());
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
        let path = PathBuf::from(super::state_root()).join("trial/post-body.md");
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
    check.usage_after = context(&check.transcript)?.usage;
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
        let c = config()?.ok_or("trial is not configured or disabled")?;
        let action = args.first().map(String::as_str).unwrap_or("list");
        let id = args.get(1).map(String::as_str).unwrap_or("");
        let value = args.get(2).map(String::as_str).unwrap_or("");
        transaction(|state| {
            if action == "list" {
                println!("{}", serde_json::to_string_pretty(state)?);
                return Ok(());
            }
            if action == "finalize" {
                if std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&c.report_thread)
                    || active(&c, now())
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
            if action != "applied"
                && std::env::var("CODEX_THREAD_ID").as_deref() != Ok(&check.session)
            {
                return Err("only the originating thread may change this observation".into());
            }
            match action {
                "none" if check.status == Status::Unchecked => {
                    check.status = Status::NoCandidate;
                    check.usage_after = context(&check.transcript)?.usage;
                }
                "draft" if check.status != Status::PostVerified => {
                    let body = fs::read_to_string(value)?;
                    if body.trim().is_empty() {
                        return Err("empty draft".into());
                    }
                    check.body = format!("{}\n{}\n\n🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)", marker(id), body.trim());
                    check.approval.clear();
                    check.status = Status::AwaitingOwner;
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
                "post" => post(&c, id, check)?,
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
                "delivered" if check.status == Status::PostVerified => {
                    check.delivery_turn = context(&check.transcript)?.turn;
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
