//! Metadata-only rollout accounting. The deserializer deliberately has no
//! conversation/instruction fields. Native contracts: OpenAI Codex rust-v0.164.0,
//! protocol/src/protocol.rs:2370-2475,3256-3325; core/src/state/session.rs:229-264;
//! core/src/agent/control/spawn.rs:120-125 (retrieved 2026-10-26).
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Tokens {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
}

impl Tokens {
    fn values(&self) -> [u64; 5] {
        [
            self.input_tokens,
            self.cached_input_tokens,
            self.cache_write_input_tokens,
            self.output_tokens,
            self.reasoning_output_tokens,
        ]
    }

    fn difference(&self, previous: &Self) -> Option<Self> {
        let a = self.values();
        let b = previous.values();
        let mut d = [0; 5];
        for i in 0..5 {
            d[i] = a[i].checked_sub(b[i])?;
        }
        Some(Self {
            input_tokens: d[0],
            cached_input_tokens: d[1],
            cache_write_input_tokens: d[2],
            output_tokens: d[3],
            reasoning_output_tokens: d[4],
        })
    }

    fn is_zero(&self) -> bool {
        self.values() == [0; 5]
    }

    fn valid(&self) -> bool {
        self.cached_input_tokens
            .checked_add(self.cache_write_input_tokens)
            .is_some_and(|cached| cached <= self.input_tokens)
            && self.reasoning_output_tokens <= self.output_tokens
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageSample {
    pub timestamp: i64,
    pub day: String,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub tokens: Tokens,
    pub request_boundary_known: bool,
    /// Missing means native history did not preserve the request's cache TTL.
    pub cache_write_ttl: Option<String>,
    pub cache_write_tokens_known: bool,
    pub service_tier: Option<String>,
    pub model_context_window: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnattributedUsage {
    pub timestamp: i64,
    pub tokens: Option<Tokens>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct HistoryReport {
    pub days: u32,
    pub calendar_timezone: &'static str,
    pub requests: Vec<UsageSample>,
    pub unattributed: Vec<UnattributedUsage>,
    pub diagnostics: Vec<String>,
    pub files_read: usize,
    pub rate_observations: Vec<RateObservation>,
    pub unknown_account_observations: usize,
}

#[derive(Debug, Deserialize)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
}

struct Envelope {
    timestamp: String,
    record: Record,
}

#[derive(Deserialize)]
struct Timed<T> {
    timestamp: String,
    payload: T,
}

#[derive(Deserialize)]
struct EventHeader {
    payload: EventKind,
}
#[derive(Deserialize)]
struct EventKind {
    #[serde(rename = "type")]
    kind: String,
}

enum Record {
    SessionMeta(Meta),
    TurnContext(TurnContext),
    ResponseUsage(UsageRecord),
    EventMsg(Event),
}

fn decode_envelope(line: &str, kind: &str) -> serde_json::Result<Envelope> {
    macro_rules! parse {
        ($ty:ty, $variant:path) => {{
            let value: Timed<$ty> = serde_json::from_str(line)?;
            Ok(Envelope {
                timestamp: value.timestamp,
                record: $variant(value.payload),
            })
        }};
    }
    match kind {
        "session_meta" => parse!(Meta, Record::SessionMeta),
        "turn_context" => parse!(TurnContext, Record::TurnContext),
        "token_usage_record" => parse!(UsageRecord, Record::ResponseUsage),
        "event_msg" => parse!(Event, Record::EventMsg),
        _ => unreachable!("only accounting records are decoded"),
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Meta {
    id: String,
    timestamp: Option<String>,
    parent_thread_id: Option<String>,
    forked_from_id: Option<String>,
    subagent_history_start_ordinal: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct TurnContext {
    model: Option<String>,
    effort: Option<String>,
    collaboration_mode: Option<Mode>,
    service_tier: Option<String>,
    #[serde(skip)]
    model_context_window: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct Mode {
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageRecord {
    thread_id: String,
    response_id: String,
    usage: NativeTokens,
    thread_token_usage: Tokens,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    TokenCount {
        info: Option<TokenInfo>,
        rate_limits: Option<Value>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct TokenInfo {
    total_token_usage: Tokens,
    last_token_usage: NativeTokens,
    model_context_window: Option<i64>,
}

#[derive(Debug)]
struct NativeTokens {
    tokens: Tokens,
    cache_write_tokens_known: bool,
}
impl<'de> Deserialize<'de> for NativeTokens {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let known = value.get("cache_write_input_tokens").is_some();
        let tokens = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            tokens,
            cache_write_tokens_known: known,
        })
    }
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

/// Reads existing sessions and archived sessions, without modifying native data.
/// Calendar days use the process's local timezone, including DST boundaries.
pub fn collect(codex_home: &Path, days: u32, now: i64) -> Result<HistoryReport> {
    if days == 0 || days > 3660 {
        bail!("--days must be between 1 and 3660");
    }
    let start = calendar_start(now, -(i64::from(days) - 1))?;
    let mut report = HistoryReport {
        days,
        calendar_timezone: "local",
        requests: vec![],
        unattributed: vec![],
        diagnostics: vec![],
        files_read: 0,
        rate_observations: vec![],
        unknown_account_observations: 0,
    };
    let mut paths = vec![];
    files_under(&codex_home.join("sessions"), &mut paths)?;
    files_under(&codex_home.join("archived_sessions"), &mut paths)?;
    paths.sort();
    let mut seen_responses = HashSet::new();
    let mut seen_events = HashSet::new();
    let mut legacy_samples: Vec<(String, UsageSample)> = vec![];
    let mut parents = HashSet::new();
    let mut grouped = BTreeMap::<String, (Meta, Vec<_>)>::new();
    let mut malformed = BTreeMap::<String, usize>::new();
    let mut missing_time = 0usize;
    for path in paths {
        let reader = BufReader::new(File::open(&path).context("open native rollout metadata")?);
        let mut records = vec![];
        let mut owner = None;
        let mut context = TurnContext::default();
        // A child can contain inherited SessionMeta records after its own first
        // record. Only the first metadata record identifies this physical file.
        for (ordinal, line) in reader.lines().enumerate() {
            let line = line.context("read native rollout metadata")?;
            // Deserialize only the discriminant first. serde's unit `other`
            // variant rejects non-unit payloads; irrelevant records are valid
            // native history, not malformed accounting. IgnoredAny also avoids
            // allocating conversation or instruction strings.
            let header = match serde_json::from_str::<Header>(&line) {
                Ok(header) => header,
                Err(_) => {
                    *malformed
                        .entry("invalid_json_or_record_type".into())
                        .or_default() += 1;
                    continue;
                }
            };
            if !matches!(
                header.kind.as_str(),
                "session_meta" | "turn_context" | "token_usage_record" | "event_msg"
            ) {
                continue;
            }
            if header.kind == "event_msg" {
                match serde_json::from_str::<EventHeader>(&line) {
                    Ok(event) if event.payload.kind == "token_count" => {}
                    Ok(_) => continue,
                    Err(_) => {
                        *malformed.entry("event_msg_type".into()).or_default() += 1;
                        continue;
                    }
                }
            }
            let entry = match decode_envelope(&line, &header.kind) {
                Ok(entry) => entry,
                Err(_) => {
                    *malformed.entry(header.kind).or_default() += 1;
                    continue;
                }
            };
            let Some(timestamp) = parse_timestamp(&entry.timestamp) else {
                missing_time += 1;
                continue;
            };
            match entry.record {
                Record::SessionMeta(meta) if owner.is_none() => {
                    if let Some(parent) = &meta.parent_thread_id {
                        parents.insert(parent.clone());
                    }
                    owner = Some(meta);
                }
                Record::TurnContext(new_context) => context = new_context,
                Record::ResponseUsage(record) => {
                    records.push((ordinal, timestamp, context.clone(), Either::Record(record)))
                }
                Record::EventMsg(Event::TokenCount { info, rate_limits }) => {
                    if timestamp >= start && timestamp <= now && rate_limits.is_some() {
                        // SessionMeta.creator_account_id is frozen at creation,
                        // not the active account on resume. Never use it here.
                        report.unknown_account_observations += 1;
                    }
                    if let Some(info) = info {
                        context.model_context_window = info.model_context_window;
                        if let Some((_, _, previous_context, _)) = records.last_mut() {
                            if previous_context.model == context.model {
                                previous_context.model_context_window = info.model_context_window;
                            }
                        }
                        records.push((ordinal, timestamp, context.clone(), Either::Legacy(info)));
                    }
                }
                _ => {}
            }
        }
        report.files_read += 1;
        let Some(meta) = owner else {
            if !records.is_empty() {
                report
                    .diagnostics
                    .push("A rollout without session metadata was excluded".into());
            }
            continue;
        };
        grouped
            .entry(meta.id.clone())
            .or_insert_with(|| (meta, vec![]))
            .1
            .extend(records);
    }
    // Merge physical copies and resume segments before differencing. File path
    // order is not chronology (archived sessions can be copied elsewhere).
    for (_, (meta, mut records)) in grouped {
        records.sort_by_key(|(_, timestamp, _, _)| *timestamp);
        let has_records = records
            .iter()
            .any(|(_, _, _, r)| matches!(r, Either::Record(u) if u.thread_id == meta.id));
        let created = meta.timestamp.as_deref().and_then(parse_timestamp);
        let inherited = meta.parent_thread_id.is_some() || meta.forked_from_id.is_some();
        let mut previous = Tokens::default();
        for (ordinal, timestamp, context, entry) in records {
            let in_range = timestamp >= start && timestamp <= now;
            if meta
                .subagent_history_start_ordinal
                .is_some_and(|n| ordinal < n)
                || (inherited && created.is_some_and(|t| timestamp < t))
            {
                continue;
            }
            let (tokens, request_boundary_known, legacy, cache_write_tokens_known) = match entry {
                Either::Record(record) => {
                    if record.thread_id != meta.id
                        || !seen_responses.insert((record.thread_id, record.response_id))
                    {
                        continue;
                    }
                    let delta = record.thread_token_usage.difference(&previous);
                    let missing = delta
                        .as_ref()
                        .and_then(|d| d.difference(&record.usage.tokens));
                    if in_range && missing.as_ref().is_some_and(|d| !d.is_zero()) {
                        report.unattributed.push(UnattributedUsage {
                            timestamp,
                            tokens: missing,
                            reason:
                                "Cumulative usage contains responses absent from this rollout span"
                                    .into(),
                        });
                    }
                    previous = record.thread_token_usage;
                    // Individual native response usage fixes the request boundary;
                    // the cumulative cross-check reveals missing/reset spans.
                    (
                        record.usage.tokens,
                        true,
                        false,
                        record.usage.cache_write_tokens_known,
                    )
                }
                Either::Legacy(info) if !has_records => {
                    let key = (meta.id.clone(), timestamp, info.total_token_usage.values());
                    if !seen_events.insert(key) {
                        continue;
                    }
                    let delta = info.total_token_usage.difference(&previous);
                    previous = info.total_token_usage;
                    let Some(delta) = delta else {
                        if in_range {
                            report.unattributed.push(UnattributedUsage {
                                timestamp,
                                tokens: None,
                                reason:
                                    "Legacy cumulative counter reset; boundary usage is unknown"
                                        .into(),
                            });
                        }
                        continue;
                    };
                    // Native can emit local context estimates and repeated rate
                    // snapshots with no billing delta. Never bill those.
                    if delta.is_zero() {
                        continue;
                    }
                    let boundary = delta == info.last_token_usage.tokens;
                    if !boundary || inherited {
                        if in_range {
                            report.unattributed.push(UnattributedUsage {
                                timestamp,
                                tokens: Some(delta),
                                reason: "Legacy usage has no reliable request/fork boundary".into(),
                            });
                        }
                        continue;
                    }
                    (
                        delta,
                        true,
                        true,
                        info.last_token_usage.cache_write_tokens_known,
                    )
                }
                _ => continue,
            };
            if !in_range || tokens.is_zero() {
                continue;
            }
            if !tokens.valid() {
                report.unattributed.push(UnattributedUsage {
                    timestamp,
                    tokens: Some(tokens),
                    reason: "Inconsistent cache/input or reasoning/output token counts".into(),
                });
                continue;
            }
            let sample = UsageSample {
                timestamp,
                day: local_day(timestamp)?,
                model: context.model,
                mode: context
                    .collaboration_mode
                    .and_then(|m| m.mode)
                    .filter(|m| m == "plan" || m == "default"),
                effort: context.effort,
                tokens,
                request_boundary_known,
                cache_write_ttl: None,
                cache_write_tokens_known,
                service_tier: context.service_tier,
                model_context_window: context.model_context_window,
            };
            if sample.model.is_none() || sample.mode.is_none() || sample.effort.is_none() {
                report.unattributed.push(UnattributedUsage {
                    timestamp,
                    tokens: Some(sample.tokens),
                    reason: "Model, collaboration mode, or reasoning effort is missing".into(),
                });
            } else if legacy {
                legacy_samples.push((meta.id.clone(), sample));
            } else {
                report.requests.push(sample);
            }
        }
    }
    for (thread, sample) in legacy_samples {
        if parents.contains(&thread) {
            // Older total_token_usage can include guardian spending. When a
            // separate child exists, don't sum that aggregate with child usage.
            report.unattributed.push(UnattributedUsage {
                timestamp: sample.timestamp,
                tokens: Some(sample.tokens),
                reason:
                    "Legacy parent cumulative usage may overlap separately recorded child usage"
                        .into(),
            });
        } else {
            report.requests.push(sample);
        }
    }
    report.requests.sort_by_key(|r| r.timestamp);
    if report.requests.iter().any(|r| !r.cache_write_tokens_known) {
        report.diagnostics.push(
            "Some older metadata omits cache-write tokens; missing is unknown, not a verified zero"
                .into(),
        );
    }
    for (kind, count) in malformed {
        report.diagnostics.push(format!(
            "{count} malformed or incompatible {kind} accounting records excluded"
        ));
    }
    if missing_time > 0 {
        report.diagnostics.push(format!(
            "{missing_time} metadata lines with invalid timestamps excluded"
        ));
    }
    report.diagnostics.push("Legacy rate snapshots lack active-account attribution; collect account-bound compare snapshots for day estimates".into());
    Ok(report)
}

enum Either {
    Record(UsageRecord),
    Legacy(TokenInfo),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateObservation {
    pub timestamp: i64,
    /// Kept in local snapshot storage for matching; callers must not print it.
    pub account_id: Option<String>,
    pub bucket: String,
    pub window: String,
    pub reset_at: Option<i64>,
    pub window_minutes: Option<i64>,
    pub used_percent: Option<f64>,
    pub ordinary_usage_allowed: Option<bool>,
}

/// Accepts either an RPC envelope or the result payload. The multi-bucket view
/// is authoritative when present, including an empty map; legacy is fallback.
pub fn decode_rate_observations(response: &Value, timestamp: i64) -> Vec<RateObservation> {
    let result = response.get("result").unwrap_or(response);
    let mut buckets: Vec<(String, &Value)> =
        match result.get("rateLimitsByLimitId").and_then(Value::as_object) {
            Some(map) => map.iter().map(|(id, v)| (id.clone(), v)).collect(),
            None => result
                .get("rateLimits")
                .filter(|v| v.is_object())
                .map(|v| {
                    vec![(
                        v.get("limitId")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                        v,
                    )]
                })
                .unwrap_or_default(),
        };
    buckets.sort_by(|a, b| a.0.cmp(&b.0));
    let mut observations = vec![];
    for (bucket, value) in buckets {
        for window in ["primary", "secondary"] {
            let Some(v) = value.get(window).filter(|v| v.is_object()) else {
                continue;
            };
            observations.push(RateObservation {
                timestamp,
                account_id: result
                    .get("accountId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                bucket: bucket.clone(),
                window: window.to_owned(),
                reset_at: v.get("resetsAt").and_then(Value::as_i64),
                window_minutes: v
                    .get("windowDurationMins")
                    .or_else(|| v.get("windowMinutes"))
                    .and_then(Value::as_i64),
                used_percent: v
                    .get("usedPercent")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite() && (0.0..=100.0).contains(v)),
                ordinary_usage_allowed: result.get("ordinaryUsageAllowed").and_then(Value::as_bool),
            });
        }
    }
    observations
}

pub fn read_rate_observations(path: &Path) -> Result<Vec<RateObservation>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for line in BufReader::new(File::open(path)?).lines() {
        out.push(serde_json::from_str(&line?).context(
            "invalid account-bound rate observation; history was not silently discarded",
        )?);
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct DayEstimate {
    pub bucket: String,
    pub window: String,
    pub complete_observed_days: usize,
    pub zero_consumption_days: usize,
    pub incomplete_days: usize,
    pub missing_days: usize,
    pub reset_crossing_days: usize,
    pub remaining_workdays_range: Option<[f64; 2]>,
    pub reason: String,
}

/// A completed day requires bracketing midnight snapshots (within five minutes
/// on either side) in the same account/bucket/reset interval. Sparse polling
/// never manufactures a full day's consumption. Current day is always excluded.
pub fn estimate_days(
    observations: &[RateObservation],
    current: &RateObservation,
    now: i64,
) -> DayEstimate {
    let mut out = DayEstimate { bucket: current.bucket.clone(), window: current.window.clone(),
        complete_observed_days: 0, zero_consumption_days: 0, incomplete_days: 0, missing_days: 0,
        reset_crossing_days: 0, remaining_workdays_range: None,
        reason: "Insufficient complete account-bound observation days; other devices and shared ChatGPT use may be included".into() };
    if current.account_id.is_none() || current.reset_at.is_none() || current.used_percent.is_none()
    {
        out.reason = "Active account, reset interval, or current percentage is unavailable".into();
        return out;
    }
    let mut days: BTreeMap<String, Vec<&RateObservation>> = BTreeMap::new();
    let mut matching: Vec<_> = observations
        .iter()
        .filter(|o| {
            o.account_id == current.account_id
                && o.bucket == current.bucket
                && o.window == current.window
                && o.timestamp <= now
        })
        .collect();
    matching.sort_by_key(|o| o.timestamp);
    matching.dedup_by_key(|o| (o.timestamp, o.reset_at));
    for o in &matching {
        if let Ok(day) = local_day(o.timestamp) {
            days.entry(day).or_default().push(o);
        }
    }
    let mut consumption = vec![];
    let today = local_day(now).unwrap_or_default();
    for (day, samples) in &days {
        if day == &today {
            out.incomplete_days += 1;
            continue;
        }
        let first = samples[0];
        let Ok(start) = calendar_start(first.timestamp, 0) else {
            out.incomplete_days += 1;
            continue;
        };
        let Ok(end) = calendar_start(first.timestamp, 1) else {
            out.incomplete_days += 1;
            continue;
        };
        let before = matching
            .iter()
            .copied()
            .filter(|o| (o.timestamp - start).abs() <= 300)
            .min_by_key(|o| (o.timestamp - start).abs());
        let after = matching
            .iter()
            .copied()
            .filter(|o| (o.timestamp - end).abs() <= 300)
            .min_by_key(|o| (o.timestamp - end).abs());
        let (Some(a), Some(b)) = (before, after) else {
            out.incomplete_days += 1;
            continue;
        };
        if a.reset_at != b.reset_at
            || a.reset_at != current.reset_at
            || samples.iter().any(|s| s.reset_at != current.reset_at)
        {
            out.reset_crossing_days += 1;
            continue;
        }
        let (Some(a_used), Some(b_used)) = (a.used_percent, b.used_percent) else {
            out.incomplete_days += 1;
            continue;
        };
        if b_used < a_used
            || samples
                .windows(2)
                .any(|w| match (w[0].used_percent, w[1].used_percent) {
                    (Some(a), Some(b)) => b < a,
                    _ => true,
                })
        {
            out.reset_crossing_days += 1;
            continue;
        }
        let used = b_used - a_used;
        if used == 0.0 {
            out.zero_consumption_days += 1;
        } else {
            consumption.push(used);
            out.complete_observed_days += 1;
        }
    }
    if let (Some(first), Some(last)) = (matching.first(), matching.last()) {
        let mut t = calendar_start(first.timestamp, 0).unwrap_or(first.timestamp);
        let end = calendar_start(last.timestamp, 0).unwrap_or(last.timestamp);
        while t < end {
            if local_day(t).is_ok_and(|d| !days.contains_key(&d)) {
                out.missing_days += 1;
            }
            let Ok(next) = calendar_start(t, 1) else {
                break;
            };
            if next <= t {
                break;
            }
            t = next;
        }
    }
    if !consumption.is_empty() && current.reset_at.is_some_and(|r| r > now) {
        let min = consumption.iter().copied().fold(f64::INFINITY, f64::min);
        let max = consumption.iter().copied().fold(0.0, f64::max);
        let remaining = 100.0 - current.used_percent.unwrap();
        out.remaining_workdays_range = Some([remaining / max, remaining / min]);
        out.reason = "Same account/bucket/reset observed-day scenario only; not another model's endurance. Reset may occur earlier; shared usage may include other devices and ChatGPT".into();
    }
    out
}

fn local_tm(timestamp: i64) -> Result<libc::tm> {
    let stamp = timestamp as libc::time_t;
    let mut out = std::mem::MaybeUninit::uninit();
    // SAFETY: both pointers are valid and localtime_r initializes on success.
    if unsafe { libc::localtime_r(&stamp, out.as_mut_ptr()) }.is_null() {
        bail!("timestamp outside local calendar range");
    }
    Ok(unsafe { out.assume_init() })
}

pub fn local_day(timestamp: i64) -> Result<String> {
    let tm = local_tm(timestamp)?;
    Ok(format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    ))
}

pub(crate) fn calendar_start(timestamp: i64, offset_days: i64) -> Result<i64> {
    let mut tm = local_tm(timestamp)?;
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_mday += i32::try_from(offset_days)?;
    tm.tm_isdst = -1;
    // SAFETY: tm is initialized. mktime normalizes day offsets using local DST.
    let result = unsafe { libc::mktime(&mut tm) };
    if result == -1 {
        bail!("calendar day outside supported range");
    }
    Ok(result as i64)
}

pub(crate) fn parse_timestamp(value: &str) -> Option<i64> {
    let b = value.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    fn number(b: &[u8]) -> Option<i32> {
        b.iter().try_fold(0, |n, c| {
            c.is_ascii_digit().then(|| n * 10 + i32::from(c - b'0'))
        })
    }
    let year = number(&b[0..4])?;
    let month = number(&b[5..7])?;
    let day = number(&b[8..10])?;
    let hour = number(&b[11..13])?;
    let minute = number(&b[14..16])?;
    let second = number(&b[17..19])?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut pos = 19;
    if b.get(pos) == Some(&b'.') {
        pos += 1;
        let first = pos;
        while b.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if first == pos {
            return None;
        }
    }
    let offset = if b.get(pos) == Some(&b'Z') && pos + 1 == b.len() {
        0
    } else if matches!(b.get(pos), Some(b'+' | b'-')) && b.len() == pos + 6 && b[pos + 3] == b':' {
        let h = number(&b[pos + 1..pos + 3])?;
        let m = number(&b[pos + 4..])?;
        if h > 23 || m > 59 {
            return None;
        }
        (h * 3600 + m * 60) * if b[pos] == b'+' { 1 } else { -1 }
    } else {
        return None;
    };
    // SAFETY: zeroed tm is valid; all relevant calendar fields are set below.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_hour = hour;
    tm.tm_min = minute;
    tm.tm_sec = second;
    // SAFETY: valid initialized tm pointer. timegm uses UTC, independent of TZ.
    let stamp = unsafe { libc::timegm(&mut tm) };
    if tm.tm_mon != month - 1 || tm.tm_mday != day {
        return None;
    }
    Some(stamp as i64 - i64::from(offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usage(input: u64, output: u64) -> Value {
        json!({"input_tokens":input,"cached_input_tokens":0,"cache_write_input_tokens":0,
            "output_tokens":output,"reasoning_output_tokens":output,"total_tokens":input+output})
    }
    fn envelope(typ: &str, payload: Value, minute: u32) -> Value {
        json!({"timestamp":format!("2026-10-25T12:{minute:02}:00Z"),"type":typ,"payload":payload})
    }
    fn meta(id: &str) -> Value {
        envelope(
            "session_meta",
            json!({"id":id,"timestamp":"2026-10-25T12:00:00Z"}),
            0,
        )
    }
    fn turn(model: &str, mode: &str) -> Value {
        envelope(
            "turn_context",
            json!({"model":model,"effort":"medium",
            "collaboration_mode":{"mode":mode,"settings":{"developer_instructions":"must not appear"}},
            "user_instructions":"must not appear"}),
            0,
        )
    }
    fn response(thread: &str, id: &str, input: u64, total: u64, minute: u32) -> Value {
        envelope(
            "token_usage_record",
            json!({"thread_id":thread,"response_id":id,
            "usage":usage(input,10),"thread_token_usage":usage(total, minute as u64 * 10)}),
            minute,
        )
    }
    fn write(root: &Path, file: &str, records: &[Value]) {
        let dir = root.join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(file),
            records.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
    }
    fn now() -> i64 {
        parse_timestamp("2026-10-26T12:00:00Z").unwrap()
    }

    #[test]
    fn exact_response_ids_deduplicate_resume_fork_and_parent_child() {
        let dir = tempfile::tempdir().unwrap();
        let parent = vec![
            meta("p"),
            turn("sol", "plan"),
            response("p", "r1", 100, 100, 1),
            turn("luna", "default"),
            response("p", "r2", 200, 300, 2),
        ];
        write(dir.path(), "parent.jsonl", &parent);
        write(dir.path(), "resumed.jsonl", &parent);
        let child = vec![
            envelope(
                "session_meta",
                json!({"id":"c","parent_thread_id":"p",
            "forked_from_id":"p","timestamp":"2026-10-25T12:00:00Z"}),
                0,
            ),
            meta("p"),
            turn("sol", "plan"),
            response("p", "r1", 100, 100, 1),
            turn("luna", "default"),
            response("c", "child-response", 50, 50, 1),
        ];
        write(dir.path(), "child.jsonl", &child);
        let report = collect(dir.path(), 7, now()).unwrap();
        assert_eq!(report.requests.len(), 3);
        assert_eq!(
            report
                .requests
                .iter()
                .map(|s| s.tokens.input_tokens)
                .sum::<u64>(),
            350
        );
        assert!(report
            .requests
            .iter()
            .any(|s| s.mode.as_deref() == Some("plan")));
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("must not appear"));
    }

    #[test]
    fn archive_copies_and_out_of_order_resume_segments_share_one_counter() {
        let dir = tempfile::tempdir().unwrap();
        let initial = vec![
            meta("p"),
            turn("sol", "default"),
            response("p", "one", 100, 100, 1),
        ];
        let later = vec![
            meta("p"),
            turn("sol", "default"),
            response("p", "two", 200, 300, 2),
        ];
        write(dir.path(), "a-later.jsonl", &later);
        write(dir.path(), "z-initial.jsonl", &initial);
        let archive = dir.path().join("archived_sessions");
        std::fs::create_dir(&archive).unwrap();
        std::fs::copy(
            dir.path().join("sessions/z-initial.jsonl"),
            archive.join("copy.jsonl"),
        )
        .unwrap();
        let report = collect(dir.path(), 7, now()).unwrap();
        assert_eq!(report.requests.len(), 2);
        assert!(report.unattributed.is_empty());
    }

    #[test]
    fn valid_nonaccounting_records_are_ignored_and_absent_writes_stay_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let mut response = response("p", "one", 100, 100, 1);
        response["payload"]["usage"]
            .as_object_mut()
            .unwrap()
            .remove("cache_write_input_tokens");
        write(
            dir.path(),
            "root.jsonl",
            &[
                meta("p"),
                turn("sol", "default"),
                envelope(
                    "response_item",
                    json!({"type":"message","content":[{"text":"must not appear"}]}),
                    0,
                ),
                envelope("world_state", json!({"arbitrary":"must not appear"}), 0),
                envelope(
                    "event_msg",
                    json!({"type":"agent_message","message":"must not appear"}),
                    0,
                ),
                response,
            ],
        );
        let report = collect(dir.path(), 7, now()).unwrap();
        assert_eq!(report.requests.len(), 1);
        assert!(!report.requests[0].cache_write_tokens_known);
        assert!(report.diagnostics.iter().all(|s| !s.contains("malformed")));
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("must not appear"));
    }

    #[test]
    fn cumulative_missing_span_and_counter_reset_do_not_become_request_prices() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "root.jsonl",
            &[
                meta("p"),
                turn("sol", "default"),
                response("p", "one", 100, 100, 1),
                response("p", "two", 100, 500, 3),
                response("p", "reset", 40, 40, 4),
            ],
        );
        let report = collect(dir.path(), 7, now()).unwrap();
        assert_eq!(report.requests.len(), 3);
        assert_eq!(report.unattributed.len(), 1);
        assert_eq!(
            report.unattributed[0].tokens.as_ref().unwrap().input_tokens,
            300
        );
    }

    #[test]
    fn legacy_duplicate_rate_updates_recomputations_and_unknown_write_counts() {
        let dir = tempfile::tempdir().unwrap();
        let info = json!({"total_token_usage":usage(100,10),"last_token_usage":usage(100,10),"model_context_window":258400});
        let event = envelope(
            "event_msg",
            json!({"type":"token_count","info":info,"rate_limits":{"primary":{}}}),
            1,
        );
        let repeated = envelope(
            "event_msg",
            json!({"type":"token_count","info":info,"rate_limits":{"primary":{}}}),
            2,
        );
        write(
            dir.path(),
            "root.jsonl",
            &[
                meta("p"),
                turn("sol", "default"),
                event.clone(),
                event,
                repeated,
            ],
        );
        let report = collect(dir.path(), 7, now()).unwrap();
        assert_eq!(report.requests.len(), 1);
        assert_eq!(report.requests[0].model_context_window, Some(258400));
        assert_eq!(report.unknown_account_observations, 3);
        assert_eq!(report.requests[0].tokens.output_tokens, 10);
    }

    #[test]
    fn missing_metadata_and_invalid_counts_stay_unattributed() {
        let dir = tempfile::tempdir().unwrap();
        let bad = envelope(
            "token_usage_record",
            json!({"thread_id":"p","response_id":"bad",
            "usage":{"input_tokens":1,"cached_input_tokens":9,"output_tokens":2,"reasoning_output_tokens":0},
            "thread_token_usage":usage(100,10)}),
            1,
        );
        write(dir.path(), "root.jsonl", &[meta("p"), bad]);
        let report = collect(dir.path(), 7, now()).unwrap();
        assert!(report.requests.is_empty());
        assert!(!report.unattributed.is_empty());
        assert!(collect(dir.path(), 0, now()).is_err());
    }

    fn observation(timestamp: i64, used: f64, account: &str, reset: i64) -> RateObservation {
        RateObservation {
            timestamp,
            account_id: Some(account.into()),
            bucket: "codex".into(),
            window: "primary".into(),
            reset_at: Some(reset),
            window_minutes: Some(10080),
            used_percent: Some(used),
            ordinary_usage_allowed: Some(true),
        }
    }

    #[test]
    fn estimates_use_complete_same_account_bucket_reset_days_and_exclude_today() {
        let today = calendar_start(now(), 0).unwrap();
        let yesterday = calendar_start(now(), -1).unwrap();
        let reset = now() + 86400 * 5;
        let observations = vec![
            observation(yesterday, 10.0, "a", reset),
            observation(today, 30.0, "a", reset),
            observation(today + 1, 99.0, "other-account", reset),
        ];
        let current = observation(now(), 40.0, "a", reset);
        let estimate = estimate_days(&observations, &current, now());
        assert_eq!(estimate.remaining_workdays_range, Some([3.0, 3.0]));
        assert_eq!(estimate.complete_observed_days, 1);
        assert_eq!(estimate.incomplete_days, 1);
        let mut other = current.clone();
        other.account_id = None;
        assert!(estimate_days(&observations, &other, now())
            .remaining_workdays_range
            .is_none());
    }

    #[test]
    fn partial_zero_missing_and_reset_days_are_separate() {
        let today = calendar_start(now(), 0).unwrap();
        let day1 = calendar_start(now(), -5).unwrap();
        let day2 = calendar_start(now(), -4).unwrap();
        let day3 = calendar_start(now(), -3).unwrap();
        let reset = now() + 86400 * 5;
        let observations = vec![
            observation(day1, 10.0, "a", reset),
            observation(day2, 10.0, "a", reset),
            observation(day3, 5.0, "a", reset + 1),
            observation(today - 3600, 40.0, "a", reset),
            observation(today + 3600, 45.0, "a", reset),
        ];
        let estimate = estimate_days(&observations, &observation(now(), 50.0, "a", reset), now());
        assert_eq!(estimate.zero_consumption_days, 1);
        assert!(estimate.reset_crossing_days >= 1);
        assert!(estimate.missing_days >= 1);
        assert!(estimate.incomplete_days >= 1);
        assert!(estimate.remaining_workdays_range.is_none());
    }

    #[test]
    fn all_buckets_and_unavailable_permission_are_preserved() {
        let response = json!({"result":{"accountId":"a","ordinaryUsageAllowed":null,
            "rateLimits":{"limitId":"legacy","primary":{"usedPercent":99}},
            "rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":11,"resetsAt":100,"windowDurationMins":10080},
                "secondary":{"usedPercent":20,"resetsAt":200}},"other":{"primary":{"usedPercent":30}}}}});
        let observations = decode_rate_observations(&response, 1);
        assert_eq!(observations.len(), 3);
        assert_eq!(observations[0].bucket, "codex");
        assert_eq!(observations[0].used_percent, Some(11.0));
        assert_eq!(observations[0].window_minutes, Some(10080));
        assert_eq!(observations[0].ordinary_usage_allowed, None);
        let response =
            json!({"rateLimitsByLimitId":{},"rateLimits":{"primary":{"usedPercent":99}}});
        assert!(decode_rate_observations(&response, 1).is_empty());
    }

    #[test]
    fn timestamp_parser_checks_timezone_and_rejects_normalized_invalid_dates() {
        assert_eq!(
            parse_timestamp("2026-10-25T12:00:00.123Z"),
            parse_timestamp("2026-10-25T21:00:00+09:00")
        );
        assert_eq!(parse_timestamp("2026-02-30T12:00:00Z"), None);
        assert_eq!(parse_timestamp("2026-10-25T99:00:00Z"), None);
        assert_eq!(parse_timestamp("2026-10-25T12:00:00"), None);
    }
}
