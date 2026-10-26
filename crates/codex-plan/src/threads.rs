//! Per-thread outcome accounting for the execute-effort experiment: how long a
//! request took to reach a created pull request, and how many human round trips
//! it needed. Native contracts: OpenAI Codex rust-v0.164.0,
//! protocol/src/protocol.rs:2273-2325 (TurnStarted/TurnComplete),
//! rollout/src/policy.rs:137-138 (turn events always persisted) (retrieved 2026-10-26).
//!
//! Only tool-call text is read beyond metadata, and only to answer two yes/no
//! questions (does a call contain `gh pr create`; does its output contain a PR
//! URL line). The text is dropped immediately: nothing is stored or printed.
use crate::experiment::Experiment;
use crate::history::{calendar_start, local_day, parse_timestamp};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// An arm with fewer threads that reached a PR is shown but not compared.
pub const MIN_PR_THREADS: usize = 5;

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Stamp {
    timestamp: String,
}

#[derive(Deserialize)]
struct Timed<T> {
    payload: T,
}

#[derive(Deserialize)]
struct PayloadKind {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Peek {
    payload: PayloadKind,
}

#[derive(Default, Deserialize)]
struct Meta {
    id: String,
    timestamp: Option<String>,
    parent_thread_id: Option<String>,
    forked_from_id: Option<String>,
}

#[derive(Default, Deserialize)]
struct TurnCtx {
    turn_id: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    service_tier: Option<String>,
    collaboration_mode: Option<Mode>,
}

#[derive(Deserialize)]
struct Mode {
    mode: Option<String>,
}

#[derive(Deserialize)]
struct Attribution {
    turn_trigger: Option<String>,
}

#[derive(Deserialize)]
struct Started {
    turn_id: String,
    started_at: Option<i64>,
    turn_attribution: Option<Attribution>,
}

#[derive(Deserialize)]
struct Finished {
    turn_id: String,
    duration_ms: Option<u64>,
    time_to_first_token_ms: Option<u64>,
}

#[derive(Deserialize)]
struct Call {
    call_id: String,
    arguments: Option<Value>,
    input: Option<Value>,
}

#[derive(Deserialize)]
struct Output {
    call_id: String,
    output: Option<Value>,
}

#[derive(Default)]
struct Turn {
    started_at: Option<i64>,
    user: bool,
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    tier: Option<String>,
    duration_ms: Option<u64>,
    ttft_ms: Option<u64>,
    aborted: bool,
}

#[derive(Default)]
struct Raw {
    meta: Meta,
    created: Option<i64>,
    turns: Vec<Turn>,
    index: HashMap<String, usize>,
    /// (turn index, seconds) of the first successful PR creation.
    pr: Option<(usize, i64)>,
}

impl Raw {
    fn turn(&mut self, id: &str) -> &mut Turn {
        let next = self.turns.len();
        let at = *self.index.entry(id.to_owned()).or_insert(next);
        if at == next {
            self.turns.push(Turn::default());
        }
        &mut self.turns[at]
    }
}

fn leaves(value: &Value, each: &mut dyn FnMut(&str)) {
    match value {
        Value::String(s) => each(s),
        Value::Array(items) => items.iter().for_each(|v| leaves(v, each)),
        Value::Object(map) => map.values().for_each(|v| leaves(v, each)),
        _ => {}
    }
}

/// Same rule as `crates/gh-edit-allow/src/gh.rs` `parse_url` for pull requests:
/// strip `#`/`?`, require exactly owner/repo/pull/N with a positive integer N.
pub fn is_pr_url(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("https://github.com/") else {
        return false;
    };
    let rest = rest.split(['#', '?']).next().unwrap_or("");
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let [owner, repo, "pull", number] = parts.as_slice() else {
        return false;
    };
    !owner.is_empty()
        && !repo.is_empty()
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && number.parse::<u64>().is_ok_and(|n| n > 0)
}

fn calls_pr_create(call: &Call) -> bool {
    let mut found = false;
    for value in [&call.arguments, &call.input].into_iter().flatten() {
        leaves(value, &mut |s| found |= s.contains("gh pr create"));
    }
    found
}

fn output_has_pr_url(output: &Output) -> bool {
    let mut found = false;
    if let Some(value) = &output.output {
        leaves(value, &mut |s| {
            found |= s.lines().any(|line| is_pr_url(line.trim()));
        });
    }
    found
}

fn read(path: &Path, files_seen: &mut HashSet<String>) -> Result<Option<Raw>> {
    let reader = BufReader::new(File::open(path).context("open native rollout")?);
    let mut raw = Raw::default();
    let mut have_meta = false;
    let mut pr_calls = HashSet::new();
    let mut current: Option<String> = None;
    for line in reader.lines() {
        let line = line.context("read native rollout")?;
        let Ok(header) = serde_json::from_str::<Header>(&line) else {
            continue;
        };
        if !matches!(
            header.kind.as_str(),
            "session_meta" | "turn_context" | "event_msg" | "response_item"
        ) {
            continue;
        }
        if header.kind == "session_meta" {
            // Only the first metadata record identifies this physical file.
            if have_meta {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<Timed<Meta>>(&line) else {
                return Ok(None);
            };
            have_meta = true;
            raw.created = entry.payload.timestamp.as_deref().and_then(parse_timestamp);
            raw.meta = entry.payload;
            // Subagent rollouts belong to the parent's work; counting them as
            // threads would double-count and mix their efforts into the arms.
            if raw.meta.parent_thread_id.is_some() || !files_seen.insert(raw.meta.id.clone()) {
                return Ok(None);
            }
            continue;
        }
        if !have_meta {
            continue;
        }
        let Some(stamp) = serde_json::from_str::<Stamp>(&line)
            .ok()
            .and_then(|t| parse_timestamp(&t.timestamp))
        else {
            continue;
        };
        if raw.meta.forked_from_id.is_some() && raw.created.is_some_and(|c| stamp < c) {
            continue;
        }
        if header.kind == "turn_context" {
            if let Ok(entry) = serde_json::from_str::<Timed<TurnCtx>>(&line) {
                let ctx = entry.payload;
                if let Some(id) = ctx.turn_id.or_else(|| current.clone()) {
                    let turn = raw.turn(&id);
                    turn.model = ctx.model;
                    turn.effort = ctx.effort;
                    turn.tier = ctx.service_tier;
                    turn.mode = ctx.collaboration_mode.and_then(|m| m.mode);
                }
            }
            continue;
        }
        let Ok(peek) = serde_json::from_str::<Peek>(&line) else {
            continue;
        };
        match (header.kind.as_str(), peek.payload.kind.as_str()) {
            ("event_msg", "task_started" | "turn_started") => {
                if let Ok(entry) = serde_json::from_str::<Timed<Started>>(&line) {
                    let started = entry.payload;
                    current = Some(started.turn_id.clone());
                    let turn = raw.turn(&started.turn_id);
                    turn.started_at = started.started_at;
                    turn.user = started
                        .turn_attribution
                        .and_then(|a| a.turn_trigger)
                        .is_some_and(|t| t == "user");
                }
            }
            ("event_msg", "task_complete" | "turn_complete" | "turn_aborted") => {
                if let Ok(entry) = serde_json::from_str::<Timed<Finished>>(&line) {
                    let finished = entry.payload;
                    let turn = raw.turn(&finished.turn_id);
                    turn.duration_ms = finished.duration_ms;
                    turn.ttft_ms = finished.time_to_first_token_ms;
                    turn.aborted = peek.payload.kind == "turn_aborted";
                }
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                if let Ok(entry) = serde_json::from_str::<Timed<Call>>(&line) {
                    if calls_pr_create(&entry.payload) {
                        pr_calls.insert(entry.payload.call_id);
                    }
                }
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                if raw.pr.is_none() {
                    if let Ok(entry) = serde_json::from_str::<Timed<Output>>(&line) {
                        if pr_calls.contains(&entry.payload.call_id)
                            && output_has_pr_url(&entry.payload)
                        {
                            if let Some(id) = current.clone() {
                                let at = raw.index.get(&id).copied();
                                raw.pr = at.map(|i| (i, stamp));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(have_meta.then_some(raw))
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub started: i64,
    pub assigned: bool,
    pub model: Option<String>,
    /// Effort of the thread's first Default-mode turn; `None` = model default.
    pub effort: Option<String>,
    pub has_default_turn: bool,
    pub manual_effort_change: bool,
    pub non_default_tier: bool,
    pub aborted_turns: usize,
    pub default_turn_ms: Vec<u64>,
    pub default_ttft_ms: Vec<u64>,
    pub to_pr: Option<ToPr>,
}

#[derive(Clone, Debug)]
pub struct ToPr {
    pub agent_ms: u64,
    pub wall_ms: u64,
    pub user_turns: u64,
}

fn summarize(raw: Raw, experiment: Option<&Experiment>) -> Option<Thread> {
    let started = raw.created?;
    let assigned = experiment
        .zip(local_day(started).ok())
        .is_some_and(|(e, day)| e.covers_day(&day));
    let default_turns: Vec<&Turn> = raw
        .turns
        .iter()
        .filter(|t| t.mode.as_deref() == Some("default"))
        .collect();
    let first = default_turns.first();
    let efforts: HashSet<&Option<String>> = default_turns.iter().map(|t| &t.effort).collect();
    let to_pr = raw.pr.and_then(|(at, pr_stamp)| {
        let first_start = raw.turns.iter().find_map(|t| t.started_at)?;
        let mut agent_ms = 0u64;
        for (i, turn) in raw.turns[..=at].iter().enumerate() {
            if i == at {
                // The PR turn keeps running after the PR exists (summary, checks).
                let begun = turn.started_at.unwrap_or(pr_stamp);
                agent_ms += u64::try_from((pr_stamp - begun).max(0)).unwrap_or(0) * 1000;
            } else {
                agent_ms += turn.duration_ms.unwrap_or(0);
            }
        }
        Some(ToPr {
            agent_ms,
            wall_ms: u64::try_from((pr_stamp - first_start).max(0)).unwrap_or(0) * 1000,
            user_turns: raw.turns[..=at].iter().filter(|t| t.user).count() as u64,
        })
    });
    Some(Thread {
        started,
        assigned,
        model: first.and_then(|t| t.model.clone()),
        effort: first.and_then(|t| t.effort.clone()),
        has_default_turn: first.is_some(),
        manual_effort_change: efforts.len() > 1,
        non_default_tier: raw.turns.iter().any(|t| {
            t.tier
                .as_deref()
                .is_some_and(|s| !matches!(s, "default" | "standard"))
        }),
        aborted_turns: raw.turns.iter().filter(|t| t.aborted).count(),
        default_turn_ms: default_turns.iter().filter_map(|t| t.duration_ms).collect(),
        default_ttft_ms: default_turns.iter().filter_map(|t| t.ttft_ms).collect(),
        to_pr,
    })
}

fn files_under(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            files_under(&entry.path(), out)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Threads created in the last `days` local calendar days, from `sessions` and
/// `archived_sessions`. Unreadable files are counted, not fatal.
pub fn collect(
    codex_home: &Path,
    days: u32,
    now: i64,
    experiment: Option<&Experiment>,
) -> Result<(Vec<Thread>, usize)> {
    if days == 0 || days > 3660 {
        bail!("--days must be between 1 and 3660");
    }
    let start = calendar_start(now, -(i64::from(days) - 1))?;
    let mut paths = vec![];
    files_under(&codex_home.join("sessions"), &mut paths)?;
    files_under(&codex_home.join("archived_sessions"), &mut paths)?;
    paths.sort();
    let mut seen = HashSet::new();
    let mut threads = vec![];
    let mut unreadable = 0;
    for path in paths {
        match read(&path, &mut seen) {
            Ok(Some(raw)) => {
                if let Some(thread) = summarize(raw, experiment) {
                    if thread.started >= start && thread.started <= now {
                        threads.push(thread);
                    }
                }
            }
            Ok(None) => {}
            Err(_) => unreadable += 1,
        }
    }
    Ok((threads, unreadable))
}

fn percentile(sorted: &[u64], p: usize) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

fn spread(mut values: Vec<u64>) -> Value {
    values.sort_unstable();
    json!({"n": values.len(), "median": percentile(&values, 50), "p90": percentile(&values, 90)})
}

#[derive(Default)]
struct Group {
    threads: usize,
    threads_with_pr: usize,
    aborted_turns: usize,
    agent_ms: Vec<u64>,
    wall_ms: Vec<u64>,
    user_turns: Vec<u64>,
    turn_ms: Vec<u64>,
    ttft_ms: Vec<u64>,
}

/// Arms by (assignment, effort). Threads whose effort was changed by hand or
/// that ran on a non-default service tier are counted apart, never averaged in.
pub fn report(
    threads: &[Thread],
    unreadable: usize,
    days: u32,
    experiment: Option<&Experiment>,
) -> Value {
    let mut groups: BTreeMap<(bool, String, String), Group> = BTreeMap::new();
    let (mut manual, mut tier, mut no_default) = (0, 0, 0);
    for thread in threads {
        if !thread.has_default_turn {
            no_default += 1;
            continue;
        }
        if thread.manual_effort_change {
            manual += 1;
            continue;
        }
        if thread.non_default_tier {
            tier += 1;
            continue;
        }
        let key = (
            thread.assigned,
            thread
                .model
                .clone()
                .unwrap_or_else(|| "(unknown model)".into()),
            thread
                .effort
                .clone()
                .unwrap_or_else(|| "(model default)".into()),
        );
        let group = groups.entry(key).or_default();
        group.threads += 1;
        group.aborted_turns += thread.aborted_turns;
        group.turn_ms.extend(&thread.default_turn_ms);
        group.ttft_ms.extend(&thread.default_ttft_ms);
        if let Some(to_pr) = &thread.to_pr {
            group.threads_with_pr += 1;
            group.agent_ms.push(to_pr.agent_ms);
            group.wall_ms.push(to_pr.wall_ms);
            group.user_turns.push(to_pr.user_turns);
        }
    }
    let arms: Vec<Value> = groups
        .into_iter()
        .map(|((assigned, model, effort), g)| {
            json!({
                "assignment": if assigned { "assigned" } else { "observed" },
                "model": model,
                "effort": effort,
                "threads": g.threads,
                "threads_with_pr": g.threads_with_pr,
                "sufficient": g.threads_with_pr >= MIN_PR_THREADS,
                "aborted_turns": g.aborted_turns,
                "to_pr_agent_ms": spread(g.agent_ms),
                "to_pr_wall_ms": spread(g.wall_ms),
                "to_pr_user_turns": spread(g.user_turns),
                "default_turn_ms": spread(g.turn_ms),
                "default_ttft_ms": spread(g.ttft_ms),
            })
        })
        .collect();
    json!({
        "days": days,
        "min_threads_with_pr": MIN_PR_THREADS,
        "threads_read": threads.len(),
        "unreadable_files": unreadable,
        "excluded": {
            "manual_effort_change": manual,
            "non_default_service_tier": tier,
            "no_default_mode_turn": no_default,
        },
        "experiment": experiment,
        "arms": arms,
    })
}

fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..=59 => format!("{}.{}s", secs, (ms % 1000) / 100),
        60..=3599 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

fn pair(value: &Value, as_duration: bool) -> String {
    let show = |v: &Value| match v.as_u64() {
        Some(n) if as_duration => duration(n),
        Some(n) => n.to_string(),
        None => "-".into(),
    };
    format!("{} / {}", show(&value["median"]), show(&value["p90"]))
}

/// Human-readable form of [`report`]; every figure is median / p90.
pub fn render(report: &Value) -> String {
    let mut text = format!(
        "\n依頼〜PR の時間と推論量（直近{}暦日、スレッド{}件、各値は 中央値 / p90）\n",
        report["days"], report["threads_read"]
    );
    if let Some(experiment) = report["experiment"].as_object() {
        text.push_str(&format!(
            "  実験: {} 〜 {}（候補 {}）\n",
            experiment
                .get("start")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            experiment
                .get("ended")
                .or_else(|| experiment.get("until"))
                .and_then(Value::as_str)
                .unwrap_or("?"),
            experiment["execute_efforts"]
        ));
    }
    text.push_str(
        "  割当てあり=実験期間に始まったスレッド。観察のみ=それ以外（設定は自分で選んだので、仕事の偏りが混ざる）\n",
    );
    let excluded = &report["excluded"];
    text.push_str(&format!(
        "  除外: 手動で推論量を変更 {} / service tier が既定以外 {} / Default モードのターンなし {} / 読めないファイル {}\n",
        excluded["manual_effort_change"],
        excluded["non_default_service_tier"],
        excluded["no_default_mode_turn"],
        report["unreadable_files"]
    ));
    let arms = report["arms"].as_array().cloned().unwrap_or_default();
    if arms.is_empty() {
        text.push_str("  該当するスレッドなし\n");
    }
    for arm in arms {
        let label = if arm["assignment"] == "assigned" {
            "割当てあり"
        } else {
            "観察のみ"
        };
        text.push_str(&format!(
            "  [{label}] {} {}: スレッド{}（PR 到達 {}）、中断ターン{}\n",
            arm["model"].as_str().unwrap_or("?"),
            arm["effort"].as_str().unwrap_or("?"),
            arm["threads"],
            arm["threads_with_pr"],
            arm["aborted_turns"]
        ));
        text.push_str(&format!(
            "    PR まで: 稼働 {}、壁時計 {}、人の往復 {}\n    1 ターン: {}、最初のトークン {}\n",
            pair(&arm["to_pr_agent_ms"], true),
            pair(&arm["to_pr_wall_ms"], true),
            pair(&arm["to_pr_user_turns"], false),
            pair(&arm["default_turn_ms"], true),
            pair(&arm["default_ttft_ms"], true),
        ));
        if arm["sufficient"] != true {
            text.push_str(&format!(
                "    件数不足（PR 到達 {} < {}）: 比較には使わない\n",
                arm["threads_with_pr"], MIN_PR_THREADS
            ));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_url_rule_matches_gh_edit_allow_parse_url_for_pull_requests() {
        // Same inputs as crates/gh-edit-allow/src/gh.rs `urls` (issues are not PRs).
        for (url, pr) in [
            ("https://github.com/Octo/Hello/pull/7", true),
            ("https://github.com/o/r/pull/7#issuecomment-1", true),
            ("https://github.com/o/r/pull/7/", true),
            ("https://github.com/o/r/issues/3#issuecomment-1", false),
            ("https://github.com/o/r/pull/7/files", false),
            ("https://github.com/o/r/commit/abc", false),
            ("https://gitlab.com/o/r/pull/7", false),
            ("https://github.com/o/r/pull/0", false),
            ("https://github.com//r/pull/7", false),
            ("https://github.com/o/r/pull/+7", false),
            ("", false),
        ] {
            assert_eq!(is_pr_url(url), pr, "{url}");
        }
    }

    const BASE: &str = "2026-10-27T01:00:00.000Z";

    fn at(offset: i64) -> (i64, String) {
        let base = parse_timestamp(BASE).unwrap();
        let t = base + offset;
        let (d, r) = (t.div_euclid(86_400), t.rem_euclid(86_400));
        // 2026-10-27 plus whole days is all these tests need.
        let day = 27 + (d - base.div_euclid(86_400));
        (
            t,
            format!(
                "2026-10-{day:02}T{:02}:{:02}:{:02}.000Z",
                r / 3600,
                r % 3600 / 60,
                r % 60
            ),
        )
    }

    fn rec(offset: i64, kind: &str, payload: &str) -> String {
        format!(
            r#"{{"timestamp":"{}","type":"{kind}","payload":{payload}}}"#,
            at(offset).1
        )
    }

    fn meta(id: &str, extra: &str) -> String {
        rec(
            0,
            "session_meta",
            &format!(r#"{{"id":"{id}","timestamp":"{BASE}"{extra}}}"#),
        )
    }

    fn turn(
        id: &str,
        off: i64,
        effort: Option<&str>,
        mode: &str,
        tier: Option<&str>,
        trigger: &str,
    ) -> Vec<String> {
        let (secs, _) = at(off);
        let effort = effort
            .map(|e| format!(r#","effort":"{e}""#))
            .unwrap_or_default();
        let tier = tier
            .map(|t| format!(r#","service_tier":"{t}""#))
            .unwrap_or_default();
        vec![
            rec(
                off,
                "event_msg",
                &format!(
                    r#"{{"type":"task_started","turn_id":"{id}","started_at":{secs},"turn_attribution":{{"turn_trigger":"{trigger}"}}}}"#
                ),
            ),
            rec(
                off,
                "turn_context",
                &format!(
                    r#"{{"turn_id":"{id}","model":"m","collaboration_mode":{{"mode":"{mode}"}}{effort}{tier}}}"#
                ),
            ),
        ]
    }

    fn done(id: &str, off: i64, ms: u64) -> String {
        rec(
            off,
            "event_msg",
            &format!(
                r#"{{"type":"task_complete","turn_id":"{id}","duration_ms":{ms},"time_to_first_token_ms":1000}}"#
            ),
        )
    }

    fn call(off: i64, id: &str, input: &str) -> String {
        rec(
            off,
            "response_item",
            &format!(
                r#"{{"type":"custom_tool_call","call_id":"{id}","name":"exec","input":{}}}"#,
                serde_json::to_string(input).unwrap()
            ),
        )
    }

    fn out(off: i64, id: &str, output: &str) -> String {
        rec(
            off,
            "response_item",
            &format!(
                r#"{{"type":"custom_tool_call_output","call_id":"{id}","output":{}}}"#,
                serde_json::to_string(output).unwrap()
            ),
        )
    }

    fn collect_lines(
        files: &[Vec<String>],
        experiment: Option<&Experiment>,
    ) -> (Vec<Thread>, usize) {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("sessions/2026/10/27");
        std::fs::create_dir_all(&dir).unwrap();
        for (i, lines) in files.iter().enumerate() {
            std::fs::write(dir.join(format!("r{i}.jsonl")), lines.join("\n")).unwrap();
        }
        collect(home.path(), 30, at(3600).0, experiment).unwrap()
    }

    #[test]
    fn pr_completion_needs_a_pr_create_call_and_a_pr_url_in_its_own_output() {
        let mut lines = vec![meta("t", "")];
        lines.extend(turn("a", 1, Some("low"), "default", None, "user"));
        // help text and a failure print no PR URL: not a completion, and the retry counts.
        lines.push(call(5, "c1", "gh pr create --help"));
        lines.push(out(6, "c1", "Create a pull request"));
        lines.push(call(7, "c2", "gh pr create -R o/r --body SECRET"));
        lines.push(out(8, "c2", "error: no commits between main and branch"));
        // another call that merely prints a PR URL is not a creation.
        lines.push(call(9, "c3", "gh pr view 12 --json url"));
        lines.push(out(10, "c3", "https://github.com/o/r/pull/12"));
        lines.push(done("a", 12, 11_000));
        lines.extend(turn("b", 20, Some("low"), "default", None, "user"));
        lines.push(call(25, "c4", "gh pr create -R o/r --body SECRET"));
        lines.push(out(
            30,
            "c4",
            "warning: 1 uncommitted change\nhttps://github.com/o/r/pull/13\n",
        ));
        lines.push(done("b", 40, 20_000));
        let (threads, _) = collect_lines(&[lines], None);
        let to_pr = threads[0]
            .to_pr
            .as_ref()
            .expect("retry after failures completes");
        // Turn a ran 11 s; turn b ran from +20 to the PR output at +30 = 10 s.
        assert_eq!(to_pr.agent_ms, 21_000);
        assert_eq!(to_pr.wall_ms, 29_000);
        assert_eq!(to_pr.user_turns, 2);

        let mut never = vec![meta("n", "")];
        never.extend(turn("a", 1, Some("low"), "default", None, "user"));
        never.push(call(5, "c1", "gh pr create --help"));
        never.push(out(6, "c1", "https://github.com/o/r/pull/12"));
        never.push(call(7, "c2", "gh pr view 12"));
        never.push(out(8, "c2", "https://github.com/o/r/pull/12"));
        let (threads, _) = collect_lines(&[never], None);
        // The first call is a `gh pr create` whose output has a PR URL line: by the
        // shared rule that is a completion (the --help case prints no URL in real life).
        assert!(threads[0].to_pr.is_some());
        let mut only_view = vec![meta("v", "")];
        only_view.extend(turn("a", 1, Some("low"), "default", None, "user"));
        only_view.push(call(7, "c2", "gh pr view 12"));
        only_view.push(out(8, "c2", "https://github.com/o/r/pull/12"));
        let (threads, _) = collect_lines(&[only_view], None);
        assert!(threads[0].to_pr.is_none());
    }

    #[test]
    fn arms_exclude_manual_changes_tier_plan_only_subagents_and_duplicates() {
        let mut low = vec![meta("low", "")];
        low.extend(turn("a", 1, Some("low"), "default", None, "user"));
        low.push(done("a", 9, 8_000));
        let mut changed = vec![meta("changed", "")];
        changed.extend(turn("a", 1, Some("low"), "default", None, "user"));
        changed.extend(turn("b", 10, Some("high"), "default", None, "user"));
        let mut tiered = vec![meta("tiered", "")];
        tiered.extend(turn(
            "a",
            1,
            Some("low"),
            "default",
            Some("priority"),
            "user",
        ));
        let mut plan_only = vec![meta("plan", "")];
        plan_only.extend(turn("a", 1, Some("medium"), "plan", None, "user"));
        let mut child = vec![meta("child", r#","parent_thread_id":"low""#)];
        child.extend(turn("a", 1, Some("low"), "default", None, "user"));
        let duplicate = low.clone();
        let experiment = Experiment::start(
            vec!["low".into(), "medium".into()],
            "2026-10-20",
            "2026-11-30",
        )
        .unwrap();
        let (threads, unreadable) = collect_lines(
            &[low, changed, tiered, plan_only, child, duplicate],
            Some(&experiment),
        );
        assert_eq!(unreadable, 0);
        assert_eq!(threads.len(), 4, "child skipped, duplicate id counted once");
        let value = report(&threads, 0, 30, Some(&experiment));
        assert_eq!(value["excluded"]["manual_effort_change"], 1);
        assert_eq!(value["excluded"]["non_default_service_tier"], 1);
        assert_eq!(value["excluded"]["no_default_mode_turn"], 1);
        let arms = value["arms"].as_array().unwrap();
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0]["effort"], "low");
        assert_eq!(arms[0]["assignment"], "assigned");
        assert_eq!(arms[0]["model"], "m");
        assert_eq!(arms[0]["sufficient"], false);
        assert_eq!(arms[0]["default_turn_ms"]["median"], 8_000);
    }

    #[test]
    fn threads_outside_the_experiment_window_and_default_effort_are_labelled() {
        let mut a = vec![meta("a", "")];
        a.extend(turn("a", 1, None, "default", None, "user"));
        // Window ended before this thread started: an observed (self-chosen) setting.
        let mut ended = Experiment::start(
            vec!["low".into(), "medium".into()],
            "2026-10-01",
            "2026-11-30",
        )
        .unwrap();
        ended.ended = Some("2026-10-26".into());
        let (threads, _) = collect_lines(&[a], Some(&ended));
        let value = report(&threads, 0, 30, Some(&ended));
        let arm = &value["arms"][0];
        assert_eq!(arm["assignment"], "observed");
        assert_eq!(arm["effort"], "(model default)");
    }

    #[test]
    fn rendering_labels_insufficient_arms_and_formats_durations() {
        assert_eq!(duration(8_000), "8.0s");
        assert_eq!(duration(754_000), "12m34s");
        assert_eq!(duration(7_380_000), "2h03m");
        let mut a = vec![meta("a", "")];
        a.extend(turn("a", 1, Some("low"), "default", None, "user"));
        let (threads, _) = collect_lines(&[a], None);
        let text = render(&report(&threads, 0, 7, None));
        assert!(
            text.contains("観察のみ") && text.contains("件数不足"),
            "{text}"
        );
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let v = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&v, 50), Some(50));
        assert_eq!(percentile(&v, 90), Some(90));
        assert_eq!(percentile(&[7], 90), Some(7));
        assert_eq!(percentile(&[], 50), None);
    }
}
