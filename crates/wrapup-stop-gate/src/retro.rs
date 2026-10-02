//! 作業終了時のレトロスペクティブ(Stop で強制)。
//!
//! このセッションで PR を作成した後の最初の Stop で、セッション全体の振り返りを
//! 構造化した JSONL として書かせ、決定論的に取れる出来事(判定レッジャーの
//! deny/ask・pr-gate の block・更新した feedback memory)が全て行に引かれて
//! いなければ block する。`disposition: inbox` の行は既存の wrap-up inbox に
//! 流れ、起票・重複検査・wrapup-chores は既存の経路をそのまま使う(出口は共有、
//! 入口と強制だけが新設)。設計と根拠は docs/claude/retro.md。

use super::{
    add, attribution, env_nonempty, feedback_stamp_file, home, inbox_for, shquote,
    updated_feedback_memories,
};
use hook_io::SessionLedger;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// retro 系の block の上限(pr-gate と同じセッション単位カウンタ方式)。
pub const MAX_BLOCKS: u64 = 3;

/// PR コメントの先頭に置く目印。`--retro-close` が読み戻しで確かめる。
pub const COMMENT_MARKER: &str = "<!-- wrapup-retro -->";

const KINDS: &[&str] = &[
    "user-correction",
    "insight",
    "friction",
    "gate-hit",
    "none",
    "skipped",
];
const NEEDS_MECHANISM: &[&str] = &["user-correction", "friction", "gate-hit"];

fn retro_dir() -> PathBuf {
    if let Some(d) = env_nonempty("WRAPUP_RETRO_DIR") {
        return PathBuf::from(d);
    }
    if let Some(x) = env_nonempty("XDG_STATE_HOME") {
        return PathBuf::from(x).join("claude/wrapup/retro");
    }
    PathBuf::from(home()).join(".local/state/claude/wrapup/retro")
}

fn rows_ledger() -> SessionLedger {
    SessionLedger::new(retro_dir(), "jsonl")
}

fn marker(sid: &str, ext: &str) -> PathBuf {
    SessionLedger::new(retro_dir(), ext).path(sid)
}

// ---------------------------------------------------------------------------
// 時刻(chrono を入れない。1970-01-01 起点の日数 ↔ 暦日は Hinnant の civil 変換)

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-02T01:02:03Z` → epoch 秒。形式外は `None`。
pub fn parse_iso_epoch(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let mut t = time.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    Some(days_from_civil(y, m, day) * 86_400 + hh * 3600 + mm * 60 + ss)
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let r = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        r / 3600,
        r % 3600 / 60,
        r % 60
    )
}

// ---------------------------------------------------------------------------
// 行の検証(閉語彙。語彙外は書けない)

fn str_field<'a>(o: &'a serde_json::Map<String, Value>, k: &str) -> Option<&'a str> {
    o.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn valid_disposition(s: &str) -> bool {
    s == "inbox"
        || s.strip_prefix("issue:#")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        || s.strip_prefix("none:")
            .is_some_and(|r| !r.trim().is_empty())
}

fn valid_mechanism(s: &str) -> bool {
    matches!(s, "prose" | "script" | "gate")
        || s.strip_prefix("existing:")
            .is_some_and(|r| !r.trim().is_empty())
        || s.strip_prefix("none:")
            .is_some_and(|r| !r.trim().is_empty())
}

/// 行を検証する。エラー文は stderr にそのまま出す。
pub fn validate_row(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("row must be a JSON object")?;
    let kind = str_field(o, "kind").ok_or("kind is required")?;
    if !KINDS.contains(&kind) {
        return Err(format!("kind must be one of {}", KINDS.join(" | ")));
    }
    let what = str_field(o, "what").ok_or("what is required")?;
    str_field(o, "evidence").ok_or("evidence is required")?;
    let disp = str_field(o, "disposition").ok_or("disposition is required")?;
    if !valid_disposition(disp) {
        return Err("disposition must be inbox | issue:#<N> | none:<reason>".into());
    }
    if matches!(kind, "none" | "skipped") && !disp.starts_with("none:") {
        return Err(format!("kind {kind} requires disposition none:<reason>"));
    }
    match (str_field(o, "mechanism"), NEEDS_MECHANISM.contains(&kind)) {
        (None, true) => {
            return Err(format!(
                "kind {kind} requires mechanism: prose | script | gate | existing:<name> | none:<reason>"
            ))
        }
        (Some(m), _) if !valid_mechanism(m) => {
            return Err(
                "mechanism must be prose | script | gate | existing:<name> | none:<reason>".into(),
            )
        }
        (Some(m), _) => {
            if let Some(name) = m.strip_prefix("existing:") {
                if !what.contains(name.trim()) {
                    return Err(
                        "mechanism existing:<name> requires `what` to name that mechanism (the gap is its defect)".into(),
                    );
                }
            }
        }
        (None, false) => {}
    }
    if kind == "skipped" {
        str_field(o, "quote").ok_or("kind skipped requires quote (the user's words, verbatim)")?;
    }
    Ok(())
}

fn project_dir() -> String {
    env_nonempty("CLAUDE_PROJECT_DIR").unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// `--retro-add <session_id> <json>`: 検証して追記し、`disposition: inbox` なら
/// 既存の inbox にも流す。0 / 64(不正な行)。
pub fn retro_add(sid: &str, line: &str) -> i32 {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("wrapup-stop-gate: --retro-add: invalid JSON");
            return 64;
        }
    };
    if let Err(e) = validate_row(&v) {
        eprintln!("wrapup-stop-gate: --retro-add: {e}");
        return 64;
    }
    let mut row = v.clone();
    row["ts"] = json!(iso_now());
    if rows_ledger().append(sid, &row.to_string()).is_err() {
        eprintln!("wrapup-stop-gate: --retro-add: cannot write the retro ledger");
        return 1;
    }
    if v["disposition"] == "inbox" {
        let detail = format!(
            "{} (retro {}; mechanism: {})",
            v["evidence"].as_str().unwrap_or(""),
            v["kind"].as_str().unwrap_or(""),
            v["mechanism"].as_str().unwrap_or("-")
        );
        let item = json!({"ts": iso_now(), "title": v["what"], "detail": detail});
        let code = add(&inbox_for(&project_dir()), &item.to_string());
        if code != 0 {
            return code;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// 決定論的な出来事

/// 行の `evidence` に引かれていなければならない出来事 `(id, 説明)`。
pub fn events(sid: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(dir) = verdict_escalate::ledger_dir() {
        let mut seen = std::collections::BTreeSet::new();
        for r in verdict_escalate::read_session_records(&dir, sid) {
            let reason = serde_json::to_value(r.reason_id)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let class = serde_json::to_value(r.match_class)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let hash: String = r
                .term_hash
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(8)
                .collect();
            let id = format!("verdict:{}:{reason}:{class}:{hash}", r.tool);
            if seen.insert(id.clone()) {
                out.push((
                    id,
                    format!("{} denied/asked a tool call ({reason})", r.tool),
                ));
            }
        }
    }
    let pr_dir = env_nonempty("PR_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".claude/pr-gate"));
    let blocks = SessionLedger::new(pr_dir.join("state"), "count").path(sid);
    if fs::read_to_string(blocks)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_some_and(|n| n > 0)
    {
        out.push((
            "pr-gate".into(),
            "pr-gate blocked this session's Stop".into(),
        ));
    }
    for p in updated_feedback_memories(sid) {
        let name = Path::new(&p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push((
            format!("memory:{name}"),
            format!("feedback memory updated in this session: {p}"),
        ));
    }
    out
}

fn read_rows(sid: &str) -> Vec<Value> {
    rows_ledger()
        .records(sid)
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// 行に引かれていない出来事。`skipped` 行が 1 つでもあれば免除(PR コメントには載る)。
pub fn uncovered(sid: &str, rows: &[Value]) -> Vec<(String, String)> {
    if rows.iter().any(|r| r["kind"] == "skipped") {
        return Vec::new();
    }
    events(sid)
        .into_iter()
        .filter(|(id, _)| {
            !rows.iter().any(|r| {
                r["evidence"]
                    .as_str()
                    .is_some_and(|e| e.contains(id.as_str()))
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 発火条件: このセッションで PR を作成したか

/// gh-edit-allow の台帳(Claude のみ)に `pr ` 行があるか。
fn pr_in_ledger(sid: &str) -> bool {
    let dir = env_nonempty("GH_EDIT_ALLOW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".claude/gh-edit-allow"));
    SessionLedger::new(dir.join("state"), "ledger")
        .records(sid)
        .iter()
        .any(|r| r.starts_with("pr "))
}

/// 台帳が無い環境(Codex)向け: 現ブランチの PR に、セッション開始 stamp より
/// 後に作られたものがあるか。
fn pr_by_branch(sid: &str, project: &str) -> bool {
    let Ok(since) = fs::metadata(feedback_stamp_file(sid)).and_then(|m| m.modified()) else {
        return false;
    };
    let Ok(since) = since.duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    let Some(branch) = Command::new("git")
        .args(["-C", project, "rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty() && b != "HEAD")
    else {
        return false;
    };
    let Ok(out) = Command::new("gh")
        .current_dir(project)
        .args([
            "pr",
            "list",
            "--head",
            &branch,
            "--state",
            "all",
            "--json",
            "createdAt",
            "--jq",
            ".[].createdAt",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    out.status.success()
        && String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_iso_epoch)
            .any(|t| t >= since.as_secs() as i64)
}

pub fn session_pr_created(sid: &str, project: &str) -> bool {
    pr_in_ledger(sid) || pr_by_branch(sid, project)
}

// ---------------------------------------------------------------------------
// Stop

fn read_count(sid: &str) -> u64 {
    fs::read_to_string(marker(sid, "count"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Stop 時の判定。block するなら指示文を返す。`stop_hook_active` では抜けない
/// (複数往復が要るので pr-gate と同じカウンタ方式)。判定不能は黙って通す。
pub fn stop_check(self_path: &str, sid: &str, project: &str, transcript: &str) -> Option<String> {
    if sid.is_empty() || sid == "unknown" || project.is_empty() {
        return None;
    }
    if marker(sid, "closed").is_file() || !session_pr_created(sid, project) {
        return None;
    }
    if !transcript.is_empty() {
        let _ = fs::create_dir_all(retro_dir());
        let _ = fs::write(marker(sid, "transcript"), transcript);
    }
    let rows = read_rows(sid);
    let status = if rows.is_empty() {
        "not started".to_string()
    } else {
        let n = uncovered(sid, &rows).len();
        if n > 0 {
            format!("{n} event(s) not yet covered by a row")
        } else {
            "rows recorded, not yet posted to the PR".to_string()
        }
    };
    let n = read_count(sid);
    if n >= MAX_BLOCKS {
        if !marker(sid, "escalated").is_file() {
            let _ = fs::write(marker(sid, "escalated"), "");
            eprintln!(
                "wrapup-stop-gate: the retro blocked {MAX_BLOCKS} times without completing ({status}); letting this Stop through. Ask the user whether to finish it now."
            );
        }
        return None;
    }
    let _ = fs::create_dir_all(retro_dir());
    let _ = fs::write(marker(sid, "count"), (n + 1).to_string());
    let cmd = format!(
        "{} --retro-procedure {}",
        shquote::printf_q(self_path),
        shquote::printf_q(sid)
    );
    Some(format!(
        "<hook-directive source=\"wrapup-stop-gate\" kind=\"retro\">\n\
         This session created a PR, so the end-of-work retrospective is due ({status}).\n\
         Run `{cmd}` and follow its output.\n\
         </hook-directive>"
    ))
}

// ---------------------------------------------------------------------------
// 手順書

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn user_messages(sid: &str) -> Option<Vec<String>> {
    let path = fs::read_to_string(marker(sid, "transcript")).ok()?;
    let text = fs::read_to_string(path.trim()).ok()?;
    let mut lines: Vec<String> = hook_io::transcript::extract_user_text(&text)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('<'))
        .map(|l| truncate(l, 200))
        .collect();
    let keep = 80;
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }
    Some(lines)
}

/// `--retro-procedure <session_id>` の手順書(LLM 向けの書式は ADR-625)。
pub fn procedure_text(self_path: &str, sid: &str) -> String {
    let (name, url) = attribution();
    let q_sid = shquote::printf_q(sid);
    let q_self = shquote::printf_q(self_path);
    let rows = read_rows(sid);
    let missing = uncovered(sid, &rows);
    let all = events(sid);
    let mut ev = String::new();
    if all.is_empty() {
        ev.push_str("  (none recorded for this session)\n");
    }
    for (id, desc) in &all {
        let mark = if missing.iter().any(|(m, _)| m == id) {
            "TODO"
        } else {
            "covered"
        };
        ev.push_str(&format!("  - [{mark}] {id} — {desc}\n"));
    }
    let utter = match user_messages(sid) {
        Some(ls) if !ls.is_empty() => {
            let mut s = String::new();
            for (i, l) in ls.iter().enumerate() {
                s.push_str(&format!("  {}. {l}\n", i + 1));
            }
            s
        }
        _ => "  (not available: no transcript path was given to the hook, e.g. Codex)\n".into(),
    };
    format!(
        r#"<hook-directive source="wrapup-stop-gate" kind="retro-procedure">
Review this whole session and record what is worth turning into a mechanism.
Rows so far: {n_rows}. Step 1 and 2 may be repeated until every TODO is covered.
  1. Events the hook recorded deterministically. Every one must be cited by the
     `evidence` of at least one row (write the id verbatim):
{ev}  2. The user's own messages in this session (shown so a correction is not
     overlooked; judge yourself which ones are corrections or complaints):
{utter}  3. For each correction, insight, friction or gate hit, add one row:
       '{q_self}' --retro-add {q_sid} '<json>'
     json: {{"kind": "user-correction|insight|friction|gate-hit|none|skipped",
            "what": "<one line>", "evidence": "<event id, file, or the user's words>",
            "disposition": "inbox|issue:#<N>|none:<reason>",
            "mechanism": "prose|script|gate|existing:<name>|none:<reason>"}}
     - mechanism is required for user-correction / friction / gate-hit: the level
       that would stop it recurring (prose = skill/AGENTS text, script, gate =
       hook; existing:<name> = a mechanism already exists and leaked — name it
       in `what`, it is that mechanism's defect).
     - disposition "inbox" also appends the row to the wrap-up inbox; filing,
       duplicate check and recurrence comments then follow the normal
       '{q_self}' --procedure flow. Do not add it to the inbox yourself.
     - If there is truly nothing, add one row with kind "none" and
       disposition "none:<reason>". Rows are never skipped silently.
     - To skip the retro, the user must have said so: kind "skipped", the
       user's words in "quote" (verbatim), disposition "none:<reason>".
  4. When no TODO is left, post ONE comment on this session's PR (the top of the
     stack when stacked) whose first line is {marker}, then a table of all rows
     grouped by kind with the filed Issue number (#N) next to each, ending with
     「🤖 Generated with [{name}]({url})」. Use gh pr comment -R <owner/repo>
     <number> --body-file <absolute path>.
  5. Run '{q_self}' --retro-close {q_sid} <comment-url>. It reads the comment
     back and only then marks the retro done. With no GitHub target use
     none:<reason> instead of the URL and say the post is unconfirmed.
</hook-directive>
"#,
        n_rows = rows.len(),
        marker = COMMENT_MARKER,
    )
}

// ---------------------------------------------------------------------------
// 完了

/// `https://github.com/o/r/pull/N#issuecomment-ID` → `(o/r, ID)`。
pub fn parse_comment_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (path, frag) = rest.split_once("#issuecomment-")?;
    let mut p = path.split('/');
    let (o, r, kind, n) = (p.next()?, p.next()?, p.next()?, p.next()?);
    if !matches!(kind, "pull" | "issues") || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if frag.is_empty() || !frag.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((format!("{o}/{r}"), frag.to_string()))
}

/// `--retro-close <session_id> <comment-url|none:reason>`: 0 = 完了 / 1 = 未了 /
/// 3 = 読み戻せない。
pub fn retro_close(sid: &str, target: &str) -> i32 {
    let rows = read_rows(sid);
    if rows.is_empty() {
        eprintln!("wrapup-stop-gate: --retro-close: no rows recorded yet");
        return 1;
    }
    let missing = uncovered(sid, &rows);
    if !missing.is_empty() {
        eprintln!("wrapup-stop-gate: --retro-close: events not yet cited by any row:");
        for (id, _) in &missing {
            eprintln!("  - {id}");
        }
        return 1;
    }
    if target
        .strip_prefix("none:")
        .is_some_and(|r| !r.trim().is_empty())
    {
        let _ = fs::write(marker(sid, "closed"), target);
        println!("retro closed (unconfirmed: no comment was posted: {target})");
        return 0;
    }
    let Some((nwo, id)) = parse_comment_url(target) else {
        eprintln!(
            "wrapup-stop-gate: --retro-close: not a GitHub comment URL (…#issuecomment-<id>)"
        );
        return 1;
    };
    let out = Command::new("gh")
        .args([
            "api",
            &format!("repos/{nwo}/issues/comments/{id}"),
            "--jq",
            ".body",
        ])
        .stdin(Stdio::null())
        .output();
    let body = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            eprintln!(
                "wrapup-stop-gate: --retro-close: gh api failed: {}",
                String::from_utf8_lossy(&o.stderr).replace('\n', " ")
            );
            return 3;
        }
        Err(_) => {
            eprintln!("wrapup-stop-gate: --retro-close: gh: command not found");
            return 3;
        }
    };
    if !body.contains(COMMENT_MARKER) {
        eprintln!(
            "wrapup-stop-gate: --retro-close: the comment does not start with {COMMENT_MARKER}"
        );
        return 1;
    }
    let _ = fs::write(marker(sid, "closed"), target);
    println!("retro closed (read back {nwo} comment {id})");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_roundtrip() {
        let t = parse_iso_epoch("2026-10-02T01:02:03Z").unwrap();
        assert_eq!(t, 1_790_902_923);
        assert_eq!(parse_iso_epoch("2026-10-02 01:02:03"), None);
        let (y, m, d) = civil_from_days(t.div_euclid(86_400));
        assert_eq!((y, m, d), (2026, 10, 2));
    }

    #[test]
    fn comment_url() {
        assert_eq!(
            parse_comment_url("https://github.com/o/r/pull/7#issuecomment-123"),
            Some(("o/r".into(), "123".into()))
        );
        assert_eq!(parse_comment_url("https://github.com/o/r/pull/7"), None);
        assert_eq!(
            parse_comment_url("https://example.com/o/r/pull/7#issuecomment-1"),
            None
        );
    }

    #[test]
    fn vocabulary() {
        let ok = json!({"kind":"user-correction","what":"w","evidence":"e","disposition":"inbox","mechanism":"gate"});
        assert!(validate_row(&ok).is_ok());
        let mut bad = ok.clone();
        bad["mechanism"] = json!("vibes");
        assert!(validate_row(&bad).is_err());
        let mut no_mech = ok.clone();
        no_mech.as_object_mut().unwrap().remove("mechanism");
        assert!(validate_row(&no_mech).is_err());
        let none = json!({"kind":"none","what":"w","evidence":"e","disposition":"inbox"});
        assert!(validate_row(&none).is_err());
        let skipped = json!({"kind":"skipped","what":"w","evidence":"e","disposition":"none:r"});
        assert!(validate_row(&skipped).is_err());
        let existing = json!({"kind":"friction","what":"foo leaked","evidence":"e","disposition":"inbox","mechanism":"existing:bar"});
        assert!(validate_row(&existing).is_err());
    }
}
