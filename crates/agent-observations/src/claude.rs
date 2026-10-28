//! Claude Code 2.1.285 transcript adapter. Content is examined only to
//! classify real user turns and correlate PR creation calls/results; it is not
//! retained in the returned observation.
use crate::codex::{Thread, ToPr};
use crate::history::{calendar_start, parse_timestamp};
use anyhow::{bail, Result};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

fn text_contains(value: &Value, pred: &dyn Fn(&str) -> bool) -> bool {
    match value {
        Value::String(s) => pred(s),
        Value::Array(a) => a.iter().any(|v| text_contains(v, pred)),
        Value::Object(o) => o.values().any(|v| text_contains(v, pred)),
        _ => false,
    }
}
fn scan(path: &Path, seen: &mut HashSet<String>) -> Result<Option<Thread>> {
    let input = BufReader::new(fs::File::open(path)?);
    let (mut id, mut start, mut user_turns) = (None::<String>, None::<i64>, 0u64);
    let mut permission = String::new();
    let mut plan_model = None;
    let mut default_model = None;
    let mut default_effort = None;
    let mut models = HashSet::new();
    let mut efforts = HashSet::new();
    let mut durations = Vec::new();
    let mut default_user_turns = 0usize;
    let mut calls = HashSet::new();
    let mut entries = HashSet::new();
    let mut pr = None;
    for line in input.lines() {
        let line = line?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v["isSidechain"] == true {
            continue;
        }
        if let Some(uuid) = v["uuid"].as_str() {
            if !entries.insert(uuid.to_owned()) {
                continue;
            }
        }
        let Some(kind) = v["type"].as_str() else {
            continue;
        };
        if id.is_none() {
            id = v["sessionId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        }
        if kind == "permission-mode" {
            if let Some(mode) = v["permissionMode"].as_str() {
                permission = mode.to_owned();
            }
            if permission != "plan" {
                default_user_turns += 1;
            }
        }
        if kind == "user"
            && v["isMeta"] != true
            && !agent_session::transcript::extract_user_text(&line).is_empty()
        {
            if let Some(t) = v["timestamp"].as_str().and_then(parse_timestamp) {
                start.get_or_insert(t);
                if pr.is_none() {
                    user_turns += 1;
                }
            }
            if let Some(mode) = v["permissionMode"].as_str() {
                permission = mode.to_owned();
            }
        }
        if kind == "assistant" {
            let model = v["message"]["model"].as_str().map(str::to_owned);
            let effort = v["effort"].as_str().map(str::to_owned);
            if permission == "plan" {
                if plan_model.is_none() {
                    plan_model = model;
                }
            } else {
                if default_model.is_none() {
                    default_model = model.clone();
                    default_effort = effort.clone();
                }
                if model.is_some() {
                    models.insert(model);
                }
                if effort.is_some() {
                    efforts.insert(effort);
                }
            }
            if let Some(content) = v["message"]["content"].as_array() {
                for item in content {
                    if item["type"] == "tool_use"
                        && item["name"] == "Bash"
                        && text_contains(&item["input"], &|s| s.contains("gh pr create"))
                    {
                        if let Some(id) = item["id"].as_str() {
                            calls.insert(id.to_owned());
                        }
                    }
                }
            }
        }
        if kind == "system" && v["subtype"] == "turn_duration" && permission != "plan" {
            if let Some(ms) = v["durationMs"].as_u64() {
                durations.push(ms);
            }
        }
        if kind == "user" && pr.is_none() {
            if let Some(items) = v["message"]["content"].as_array() {
                for item in items {
                    if item["type"] == "tool_result"
                        && item["is_error"] != true
                        && item["tool_use_id"]
                            .as_str()
                            .is_some_and(|id| calls.contains(id))
                        && text_contains(&item["content"], &|s| {
                            s.lines().any(|l| github_command::url::is_pr_url(l.trim()))
                        })
                    {
                        pr = v["timestamp"].as_str().and_then(parse_timestamp);
                    }
                }
            }
        }
    }
    let Some(id) = id else { return Ok(None) };
    if !seen.insert(id) {
        return Ok(None);
    }
    let Some(started) = start else {
        return Ok(None);
    };
    let to_pr = pr
        .and_then(|t| u64::try_from(t.checked_sub(started)?).ok())
        .map(|seconds| ToPr {
            agent_ms: None,
            wall_ms: Some(seconds * 1000),
            user_turns,
        });
    let count = default_user_turns.max(durations.len());
    Ok(Some(Thread {
        started,
        assigned: false,
        model: default_model.clone(),
        plan_model,
        effort: default_effort,
        has_default_turn: default_model.is_some(),
        manual_effort_change: efforts.len() > 1,
        manual_model_change: models.len() > 1,
        non_default_tier: false,
        aborted_turns: 0,
        default_turn_ms: durations,
        default_ttft_ms: Vec::new(),
        default_turn_count: count,
        to_pr,
    }))
}

pub fn collect(projects: &Path, days: u32, now: i64) -> Result<(Vec<Thread>, usize)> {
    if days == 0 || days > 3660 {
        bail!("--days must be between 1 and 3660")
    }
    let earliest = calendar_start(now, -(i64::from(days) - 1))?;
    let mut paths = Vec::<PathBuf>::new();
    if !projects.exists() {
        return Ok((Vec::new(), 0));
    }
    for project in fs::read_dir(projects)? {
        let project = project?;
        if !project.file_type()?.is_dir() {
            continue;
        }
        for file in fs::read_dir(project.path())? {
            let file = file?;
            if file.file_type()?.is_file() && file.path().extension().is_some_and(|e| e == "jsonl")
            {
                paths.push(file.path());
            }
        }
    }
    paths.sort();
    let (mut seen, mut result, mut unreadable) = (HashSet::new(), Vec::new(), 0);
    for path in paths {
        match scan(&path, &mut seen) {
            Ok(Some(t)) if t.started >= earliest && t.started <= now => result.push(t),
            Ok(_) => (),
            Err(_) => unreadable += 1,
        }
    }
    Ok((result, unreadable))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correlates_success_and_excludes_sidechain_without_exporting_content() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("session.jsonl");
        let lines = [
            r#"{"type":"user","sessionId":"private-id","timestamp":"2026-10-27T01:00:00Z","message":{"content":"private prompt"},"permissionMode":"plan"}"#,
            r#"{"type":"assistant","sessionId":"private-id","timestamp":"2026-10-27T01:01:00Z","message":{"model":"planner","content":[]}}"#,
            r#"{"type":"user","sessionId":"private-id","timestamp":"2026-10-27T01:02:00Z","message":{"content":"continue"},"permissionMode":"default"}"#,
            r#"{"type":"assistant","sessionId":"private-id","timestamp":"2026-10-27T01:03:00Z","effort":"low","message":{"model":"runner","content":[{"type":"tool_use","name":"Bash","id":"c1","input":{"command":"gh pr create --body private"}}]}}"#,
            r#"{"type":"user","sessionId":"private-id","timestamp":"2026-10-27T01:04:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"c1","content":"https://github.com/private/repo/pull/5"}]}}"#,
            r#"{"type":"assistant","sessionId":"private-id","isSidechain":true,"timestamp":"2026-10-27T01:05:00Z","message":{"model":"other","content":[]}}"#,
        ];
        fs::write(&p, lines.join("\n")).unwrap();
        let t = scan(&p, &mut HashSet::new()).unwrap().unwrap();
        assert_eq!(t.plan_model.as_deref(), Some("planner"));
        assert_eq!(t.model.as_deref(), Some("runner"));
        assert!(!t.manual_model_change);
        assert_eq!(t.to_pr.as_ref().unwrap().user_turns, 2);
        assert_eq!(t.to_pr.as_ref().unwrap().wall_ms, Some(240_000));
        assert_eq!(t.to_pr.as_ref().unwrap().agent_ms, None);
        let serialized = crate::codex::report(&[t], 0, 14, None).to_string();
        for secret in ["private-id", "private prompt", "private/repo", "pull/5"] {
            assert!(!serialized.contains(secret));
        }
    }
}
