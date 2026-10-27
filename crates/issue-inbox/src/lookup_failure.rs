//! Failed lookups defer only the exact inbox rows observed in this session.
use agent_session::ledger::SessionLedger;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, io, path::PathBuf};

pub(super) struct Context {
    inbox: String,
    session: String,
    title: String,
    repo: String,
    lines: Vec<String>,
}

fn ledger() -> SessionLedger {
    SessionLedger::new(
        PathBuf::from(super::state_root()).join("lookup-failures"),
        "jsonl",
    )
}

fn key(inbox: &str, title: &str, repo: &str) -> String {
    json!([inbox, title, repo]).to_string()
}

fn path(inbox: &str) -> io::Result<String> {
    Ok(fs::canonicalize(inbox)?.to_string_lossy().into_owned())
}

impl Context {
    pub(super) fn capture(inbox: &str, session: &str, title: &str, repo: &str) -> io::Result<Self> {
        if session.is_empty() || session == "unknown" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a known session ID is required",
            ));
        }
        let inbox = path(inbox)?;
        let _lock = super::lock(&format!("{inbox}.lock"))?;
        let lines = fs::read_to_string(&inbox)?
            .split_terminator('\n')
            .filter(|line| {
                serde_json::from_str::<Value>(line).is_ok_and(|v| {
                    v["title"].as_str() == Some(title) && v["repo"].as_str().unwrap_or("") == repo
                })
            })
            .map(str::to_owned)
            .collect();
        Ok(Self {
            inbox,
            session: session.into(),
            title: title.into(),
            repo: repo.into(),
            lines,
        })
    }

    pub(super) fn record(&self, failed: bool) -> io::Result<()> {
        let ledger = ledger();
        let file = ledger.path(&self.session);
        let dir = file.parent().expect("ledger parent");
        fs::create_dir_all(dir)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let _lock = super::lock(&format!("{}.lock", file.display()))?;
        let lines = if failed { &self.lines[..] } else { &[] };
        ledger.append(
            &self.session,
            &json!({
                "session": self.session,
                "key": key(&self.inbox, &self.title, &self.repo),
                "inbox": self.inbox,
                "lines": lines,
            })
            .to_string(),
        )
    }
}

/// Missing/corrupt state must restore the filing request, never hide work.
pub(super) fn pending_lines(inbox: &str, session: &str) -> io::Result<Vec<String>> {
    let text = match fs::read_to_string(inbox) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let lines: Vec<_> = text
        .split_terminator('\n')
        .filter(|line| !line.is_empty())
        .collect();
    if session.is_empty() || session == "unknown" {
        return Ok(lines.into_iter().map(str::to_owned).collect());
    }
    let inbox = path(inbox)?;
    let ledger = ledger();
    // Read atomically with appenders. An absent ledger means no deferred rows.
    let file = ledger.path(session);
    if !file.exists() {
        return Ok(lines.into_iter().map(str::to_owned).collect());
    }
    let _lock = super::lock(&format!("{}.lock", file.display()))?;
    let mut active = HashMap::new();
    for record in fs::read_to_string(file)?.lines() {
        let record: Value = serde_json::from_str(record).map_err(io::Error::other)?;
        if record["inbox"].as_str() != Some(&inbox) || record["session"].as_str() != Some(session) {
            continue;
        }
        let key = record["key"]
            .as_str()
            .ok_or_else(|| io::Error::other("invalid failure ledger key"))?
            .to_owned();
        let rows = record["lines"]
            .as_array()
            .ok_or_else(|| io::Error::other("invalid failure ledger rows"))?;
        let rows = rows
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| io::Error::other("invalid failure ledger row"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        active.insert(key, rows);
    }
    Ok(lines
        .iter()
        .filter(|line| !active.values().flatten().any(|failed| failed == **line))
        .map(|line| (*line).to_owned())
        .collect())
}
