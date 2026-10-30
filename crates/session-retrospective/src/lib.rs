//! 作業終了時のレトロスペクティブ(Stop で強制)。
//!
//! このセッションで PR を作成した後の最初の Stop で、セッション全体の振り返りを
//! 構造化した JSONL として書かせ、決定論的に取れる出来事(判定レッジャーの
//! deny/ask・pr-gate の block・更新した feedback memory)が全て行に引かれて
//! いなければ block する。`disposition: inbox` の行は既存の wrap-up inbox(vault の
//! ノート)に流れ、裁定は backlog-cleanup が担う(出口は共有、入口と強制だけが新設)。
//! 全行は、`--retro-close` が台帳から vault の `retro/` のノートに決定論的に書き出す
//! (PR コメントは投稿しない。ADR-1136)。設計と根拠は docs/agents/retro.md。

pub mod traces;

use agent_session::SessionLedger;
use issue_inbox::codify::Vocabulary;
use issue_inbox::{
    add, env_nonempty, feedback_stamp_file, home, inbox_for, shquote, updated_feedback_memories,
};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// retro 系の block の上限(pr-gate と同じセッション単位カウンタ方式)。
pub const MAX_BLOCKS: u64 = 3;

const KINDS: &[&str] = &[
    "user-correction",
    "insight",
    "friction",
    "gate-hit",
    "codify",
    "none",
    "skipped",
];
const NEEDS_MECHANISM: &[&str] = &["user-correction", "friction", "gate-hit", "codify"];

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

use agent_session::time::civil_from_days;
pub use agent_session::time::parse_iso_epoch;

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

/// `mechanism` の値の分類。段(eliminate|unrepresentable|gate|tool|prose)は語彙が正本(codify-signals)。
enum Mechanism<'a> {
    /// 段。`signal` / `existing` / `searched` が要る。
    Level(&'a str),
    /// すでにある仕組みが漏らした。名前は `what` に書く。
    Existing(&'a str),
    /// 決定論化しない。理由つき。
    None,
}

fn parse_mechanism(s: &str) -> Result<Mechanism<'_>, String> {
    let vocab = Vocabulary::get();
    let nonempty = |r: &str| !r.trim().is_empty();
    if vocab.level(s).is_some() {
        return Ok(Mechanism::Level(s));
    }
    if let Some(to) = vocab.canonical_level(s) {
        return Err(format!("mechanism `{s}` is retired: use `{to}`"));
    }
    if let Some(name) = s.strip_prefix("existing:").filter(|r| nonempty(r)) {
        return Ok(Mechanism::Existing(name));
    }
    if s.strip_prefix("none:").is_some_and(nonempty) {
        return Ok(Mechanism::None);
    }
    let levels: Vec<&str> = vocab.levels_desc().iter().map(|l| l.id.as_str()).collect();
    Err(format!(
        "mechanism must be {} | existing:<name> | none:<reason>",
        levels.join(" | ")
    ))
}

/// `existing` 欄の文法: `採用:<名前>` | `拡張:<名前>` | `自前`(語彙の verdicts)。
fn check_existing(s: &str) -> Result<&str, String> {
    let vocab = Vocabulary::get();
    let (verdict, name) = match s.split_once(':') {
        Some((v, n)) => (v.trim(), Some(n.trim())),
        None => (s.trim(), None),
    };
    if !vocab.is_existing_verdict(verdict) {
        return Err(format!(
            "existing must be {} (a name after `:` for all but 自前)",
            vocab.existing_verdicts.join(" | ")
        ));
    }
    match (verdict, name) {
        ("自前", None) => Ok(verdict),
        ("自前", Some(_)) => Err("existing 自前 takes no name".into()),
        (_, Some(n)) if !n.is_empty() => Ok(verdict),
        _ => Err(format!(
            "existing {verdict} requires `:<name>` (the existing mechanism)"
        )),
    }
}

/// `searched` 欄: 探した範囲のカンマ区切り。リポジトリ内は必ず探す。`自前` のときは外部も 1 回探す。
fn check_searched(s: &str, own: bool) -> Result<(), String> {
    let vocab = Vocabulary::get();
    let tokens: Vec<&str> = s
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    let has = |scope: &str| {
        tokens
            .iter()
            .any(|t| t.split(':').next().is_some_and(|head| head == scope))
    };
    for scope in &vocab.in_repo_scope {
        if !has(scope) {
            return Err(format!(
                "searched must name the in-repo search: {} (missing `{scope}`)",
                vocab.in_repo_scope.join(",")
            ));
        }
    }
    if own && !vocab.external_scope.iter().any(|scope| has(scope)) {
        return Err(format!(
            "existing 自前 requires one external search in `searched`: {} (e.g. upstream:<product>)",
            vocab.external_scope.join(" | ")
        ));
    }
    Ok(())
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
    if kind == "user-correction" && disp == "inbox" {
        return Err(
            "kind user-correction is feedback: file it as an Issue (disposition issue:#<N>) or give none:<reason> (project-specific or sensitive); it is not kept in the inbox"
                .into(),
        );
    }
    if matches!(kind, "none" | "skipped") && !disp.starts_with("none:") {
        return Err(format!("kind {kind} requires disposition none:<reason>"));
    }
    let codify_fields = ["signal", "level_reason", "existing", "searched"];
    match (str_field(o, "mechanism"), NEEDS_MECHANISM.contains(&kind)) {
        (None, true) => {
            return Err(format!(
                "kind {kind} requires mechanism: {} | existing:<name> | none:<reason>",
                level_names()
            ))
        }
        (None, false) => {
            if let Some(k) = codify_fields.iter().find(|k| o.contains_key(**k)) {
                return Err(format!(
                    "{k} is only valid on a row whose mechanism is a level"
                ));
            }
        }
        (Some(m), _) => match parse_mechanism(m)? {
            Mechanism::Existing(name) => {
                if !what.contains(name.trim()) {
                    return Err(
                        "mechanism existing:<name> requires `what` to name that mechanism (the gap is its defect)".into(),
                    );
                }
                if let Some(k) = ["level_reason", "searched"]
                    .iter()
                    .find(|k| o.contains_key(**k))
                {
                    return Err(format!(
                        "{k} is only valid on a row whose mechanism is a level"
                    ));
                }
                if let Some(e) = str_field(o, "existing") {
                    check_existing(e)?;
                }
            }
            Mechanism::None => {
                if let Some(k) = codify_fields.iter().find(|k| o.contains_key(**k)) {
                    return Err(format!(
                        "{k} is only valid on a row whose mechanism is a level (mechanism none:<reason> means not to codify)"
                    ));
                }
            }
            Mechanism::Level(level) => validate_level_row(o, level)?,
        },
    }
    if kind == "skipped" {
        str_field(o, "quote").ok_or("kind skipped requires quote (the user's words, verbatim)")?;
    }
    Ok(())
}

fn level_names() -> String {
    Vocabulary::get()
        .levels_desc()
        .iter()
        .map(|l| l.id.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 段を持つ行の必須欄: `signal`(兆候)・`existing`(既存手段)・`searched`(探した範囲)。
/// 兆候の既定の段と違う段を選ぶなら `level_reason` も要る。
fn validate_level_row(o: &serde_json::Map<String, Value>, level: &str) -> Result<(), String> {
    let vocab = Vocabulary::get();
    let signal_ids: Vec<&str> = vocab.signals.iter().map(|s| s.id.as_str()).collect();
    let id = str_field(o, "signal").ok_or_else(|| {
        format!(
            "mechanism {level} requires signal: {}",
            signal_ids.join(" | ")
        )
    })?;
    let signal = vocab
        .signal(id)
        .ok_or_else(|| format!("signal must be {}", signal_ids.join(" | ")))?;
    if level != signal.default_level && str_field(o, "level_reason").is_none() {
        return Err(format!(
            "signal {id} defaults to {}; mechanism {level} requires level_reason (why this level instead)",
            signal.default_level
        ));
    }
    // 文章(prose)には「新しく作る物」が無いので、既存手段の探索は求めない。
    // 上位の段を選ばなかった理由は、`level_reason`(既定の段と違うとき必須)が残す。
    if level == "prose" {
        for k in ["existing", "searched"] {
            if o.contains_key(k) {
                return Err(format!("{k} is not used on a prose row (nothing is built)"));
            }
        }
        return Ok(());
    }
    let existing = str_field(o, "existing")
        .ok_or("mechanism <level> requires existing: 採用:<name> | 拡張:<name> | 自前")?;
    let verdict = check_existing(existing)?;
    let searched = str_field(o, "searched")
        .ok_or("mechanism <level> requires searched: the places looked at, comma-separated")?;
    check_searched(searched, verdict == "自前")
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
    out.extend(traces::collect(sid).events(Vocabulary::get()));
    out
}

fn read_rows(sid: &str) -> Vec<Value> {
    rows_ledger()
        .records(sid)
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// 行に引かれていない出来事。`skipped` 行が 1 つでもあれば免除(ノートには載る)。
pub fn uncovered(sid: &str, rows: &[Value]) -> Vec<(String, String)> {
    if rows.iter().any(|r| r["kind"] == "skipped") {
        return Vec::new();
    }
    events(sid)
        .into_iter()
        .filter(|(id, _)| !covers(rows, id))
        .collect()
}

/// 行のどれかが、この出来事の id を `evidence` に引いているか。その場のコードの痕跡(`adhoc:*`)は、
/// 「決定論化できないか」を問う `codify` の行でしか消し込めない。
fn covers(rows: &[Value], id: &str) -> bool {
    let only_codify = id.starts_with("adhoc:");
    rows.iter().any(|r| {
        (!only_codify || r["kind"] == "codify")
            && r["evidence"].as_str().is_some_and(|e| e.contains(id))
    })
}

// ---------------------------------------------------------------------------
// 発火条件: このセッションで PR を作成したか

/// gh-edit-allow の台帳(Claude のみ)の最後の `pr <owner/repo> <N>` 行。stack のときは、最後に作った
/// PR(= 最上段)になる。
fn ledger_pr(sid: &str) -> Option<(String, String)> {
    let dir = env_nonempty("GH_EDIT_ALLOW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".claude/gh-edit-allow"));
    SessionLedger::new(dir.join("state"), "ledger")
        .records(sid)
        .iter()
        .rev()
        .find_map(|r| {
            let mut w = r.split_whitespace();
            (w.next()? == "pr").then(|| {
                (
                    w.next().unwrap_or("").to_string(),
                    w.next().unwrap_or("").to_string(),
                )
            })
        })
}

/// 台帳が無い環境(Codex)向け: 現ブランチの PR に、セッション開始 stamp より
/// 後に作られたものがあれば、その番号(無ければ空文字)。`gh pr list` の各行は
/// `<createdAt>` か `<createdAt> <number>`。
fn branch_pr(sid: &str, project: &str) -> Option<String> {
    let since = fs::metadata(feedback_stamp_file(sid))
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let branch = Command::new("git")
        .args(["-C", project, "rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty() && b != "HEAD")?;
    let out = Command::new("gh")
        .current_dir(project)
        .args([
            "pr",
            "list",
            "--head",
            &branch,
            "--state",
            "all",
            "--json",
            "createdAt,number",
            "--jq",
            r#".[] | "\(.createdAt) \(.number)""#,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let t = parse_iso_epoch(w.next()?)?;
            (t >= since.as_secs() as i64).then(|| (t, w.next().unwrap_or("").to_string()))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, n)| n)
}

pub fn session_pr_created(sid: &str, project: &str) -> bool {
    ledger_pr(sid).is_some() || branch_pr(sid, project).is_some()
}

/// このセッションの PR(`owner/repo` と番号)。台帳 → 現ブランチの PR の順。分からなければ `None`。
pub fn retro_pr(sid: &str, project: &str) -> Option<(String, String)> {
    if let Some((nwo, n)) = ledger_pr(sid) {
        return Some((nwo, n));
    }
    let n = branch_pr(sid, project)?;
    let nwo = git_repository::origin_nwo(Path::new(project))?;
    Some((nwo, n))
}

// ---------------------------------------------------------------------------
// Stop

fn read_count(sid: &str) -> u64 {
    fs::read_to_string(marker(sid, "count"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// 閉じた retro を開き直せる回数(1 セッションあたり)。
pub const MAX_REOPENS: u64 = 2;

/// 閉じた後に、まだ行に引かれていない `adhoc:*` の痕跡があるか。`skipped` の行があれば免除。
fn pending_adhoc(sid: &str, rows: &[Value]) -> bool {
    if rows.iter().any(|r| r["kind"] == "skipped") {
        return false;
    }
    traces::collect(sid)
        .events(Vocabulary::get())
        .iter()
        .any(|(id, _)| !covers(rows, id))
}

/// 閉じた retro を、新しい痕跡の分だけ開き直す(上限 [`MAX_REOPENS`])。「作業の終わり」を機械で
/// 推測しない(#854)ので、痕跡は最初の Stop で拾い、その後に書いたコードはここで拾う。
fn reopen(sid: &str) -> bool {
    if !pending_adhoc(sid, &read_rows(sid)) {
        return false;
    }
    let n = fs::read_to_string(marker(sid, "reopen"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if n >= MAX_REOPENS {
        return false;
    }
    let _ = fs::write(marker(sid, "reopen"), (n + 1).to_string());
    for ext in ["closed", "escalated", "count"] {
        let _ = fs::remove_file(marker(sid, ext));
    }
    true
}

/// Stop 時の判定。block するなら指示文を返す。`stop_hook_active` では抜けない
/// (複数往復が要るので pr-gate と同じカウンタ方式)。判定不能は黙って通す。
///
/// 発火は、このセッションで PR を作った、またはその場で書いたコードの痕跡がしきい値を超えた、のどちらか。
/// 検査は軽い順(閉じた印 → PR の台帳 → scratchpad とハッシュのログ → `gh pr list`)に行う。
pub fn stop_check(self_path: &str, sid: &str, project: &str, transcript: &str) -> Option<String> {
    if sid.is_empty() || sid == "unknown" || project.is_empty() {
        return None;
    }
    let why = if marker(sid, "closed").is_file() {
        if !reopen(sid) {
            return None;
        }
        "This session wrote more ad-hoc code after the retrospective was closed"
    } else if ledger_pr(sid).is_some() {
        "This session created a PR"
    } else if traces::collect(sid).fires(Vocabulary::get()) {
        "This session wrote ad-hoc code (a script or a one-off command worth asking \"can this be code?\")"
    } else if branch_pr(sid, project).is_some() {
        "This session created a PR"
    } else {
        return None;
    };
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
            "rows recorded, not yet closed into the vault note".to_string()
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
         {why}, so the end-of-work retrospective is due ({status}).\n\
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
    let mut lines: Vec<String> = agent_session::transcript::extract_user_text(&text)
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
    let vocab = Vocabulary::get();
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
     json: {{"kind": "{kinds}",
            "what": "<one line>", "evidence": "<event id, file, or the user's words>",
            "disposition": "inbox|issue:#<N>|none:<reason>",
            "mechanism": "{levels}|existing:<name>|none:<reason>",
            "signal": "<signal>", "level_reason": "<why, only if the level differs from the signal's default>",
            "existing": "採用:<name>|拡張:<name>|自前", "searched": "{searched}[,{external}:<product>]"}}
     - mechanism is required for user-correction / friction / gate-hit: the
       highest level that would really stop it recurring ({levels}, best first;
       existing:<name> = a mechanism already exists and leaked — name it in
       `what`, it is that mechanism's defect; none:<reason> = not worth codifying).
{level_guide}     - A level row also needs `signal` (why it looks codifiable), `existing` (is there
       something to adopt or extend before building: 採用|拡張|自前) and `searched`
       (where you looked, comma-separated). Look in this repository first, always:
       pipeline-map.toml node ids, the crates/ names, the config/agents/skills/ names.
       Look outside (nixpkgs, an upstream feature) once, and only when the answer would be 自前.
       A level other than the signal's default needs `level_reason`.
{signal_guide}     - The tables with examples: docs/agents/codify-signals.md.
     - kind codify: the `adhoc:*` events are traces of code written on the spot (a
       scratchpad script, a one-off command run again, an interpreter one-liner). Ask of
       each: could this run the same way every time (a tool, a gate, a declaration)?
       Cite its id in `evidence`. If it is not worth codifying, still add a codify row
       with mechanism none:<reason> (it needs judgment, it was one-off, ...). Only a
       codify row clears an `adhoc:*` event.
     - disposition "inbox" also appends the row to the wrap-up inbox (a note in
       the user's vault, or a jsonl line on a machine without one). No Issue is
       filed for it: the user adjudicates it later with /backlog-cleanup. Do not
       add it to the inbox yourself.
     - A user-correction is feedback and is never kept in the inbox: file it as a
       GitHub Issue first (gh issue create) and use "issue:#<N>", or use
       "none:<reason>" when it is project-specific or sensitive. "inbox" is refused.
     - If there is truly nothing, add one row with kind "none" and
       disposition "none:<reason>". Rows are never skipped silently.
     - To skip the retro, the user must have said so: kind "skipped", the
       user's words in "quote" (verbatim), disposition "none:<reason>".
  4. When no TODO is left, run '{q_self}' --retro-close {q_sid}. It renders all
     rows, grouped by kind, into one note in the user's vault (retro/…) and reads
     it back before marking the retro done. Nothing is posted to GitHub. On a
     machine without a vault the rows stay in the local ledger and the retro is
     marked done.
</hook-directive>
"#,
        n_rows = rows.len(),
        kinds = KINDS.join("|"),
        levels = vocab
            .levels_desc()
            .iter()
            .map(|l| l.id.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        searched = vocab.in_repo_scope.join(","),
        external = vocab.external_scope.join("|"),
        level_guide = vocab
            .levels_desc()
            .iter()
            .map(|l| format!("       {} = {}\n", l.id, l.means))
            .collect::<String>(),
        signal_guide = vocab
            .signals
            .iter()
            .map(|s| {
                format!(
                    "       signal {} (default {}): {}\n",
                    s.id, s.default_level, s.look_for
                )
            })
            .collect::<String>(),
    )
}

// ---------------------------------------------------------------------------
// 完了: 台帳から vault のノートを決定論的に書き出し、読み戻してから閉じる

const KIND_ORDER: &[&str] = &[
    "user-correction",
    "friction",
    "gate-hit",
    "codify",
    "insight",
    "none",
    "skipped",
];

/// 表の 1 マス。`|` と改行は壊れる元なので置き換える。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// 台帳の `mechanism` の表示。旧い `script` は今の `tool` と読み、元の値も残す
/// (過去の台帳は書き換えない。ADR-697 の語彙の改訂で段の名前が変わった)。
fn display_mechanism(m: Option<&str>) -> String {
    let Some(m) = m else { return "-".into() };
    let vocab = Vocabulary::get();
    match vocab.canonical_level(m) {
        Some(c) if c != m => cell(&format!("{c} ({m})")),
        _ => cell(m),
    }
}

/// 台帳の全行を kind ごとの表にした、ノートの本文。
pub fn render_body(rows: &[Value]) -> String {
    let mut out = String::new();
    for kind in KIND_ORDER {
        let of_kind: Vec<&Value> = rows.iter().filter(|r| r["kind"] == *kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        out.push_str(&format!("## {kind}\n\n"));
        out.push_str(
            "| what | evidence | disposition | mechanism | signal | 既存手段 |\n|---|---|---|---|---|---|\n",
        );
        for r in of_kind {
            let f = |k: &str| cell(r[k].as_str().unwrap_or("-"));
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                f("what"),
                f("evidence"),
                f("disposition"),
                display_mechanism(r["mechanism"].as_str()),
                f("signal"),
                f("existing")
            ));
            if let Some(q) = r["quote"].as_str() {
                out.push_str(&format!("|  | quote: {} |  |  |  |  |\n", cell(q)));
            }
        }
        out.push('\n');
    }
    out
}

/// ノートの vault 内のパス(拡張子つき)。PR が分からなければ番号の代わりに `s<session の先頭 8 文字>`。
fn note_rel(date: &str, nwo: &str, pr: &str, sid: &str, suffix: Option<&str>) -> String {
    let who = nwo.replace('/', "-");
    let tail = if pr.is_empty() {
        format!("s{}", sid.chars().take(8).collect::<String>())
    } else {
        pr.to_string()
    };
    let name = vault_note::note_name(&format!("{date}-{who}-{tail}"));
    let name = match suffix {
        Some(s) => format!("{name} ~{s}"),
        None => name,
    };
    format!("retro/{}/{name}.md", &date[..4.min(date.len())])
}

/// `--retro-close <session_id>`: 0 = 完了 / 1 = 未了 / 3 = ノートを書けない・読み戻せない。
/// vault を宣言していない機体(使えない場合を含む)では、台帳に残したまま完了にする。
pub fn retro_close(sid: &str) -> i32 {
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
    let ledger_only = |why: &str| {
        let _ = fs::create_dir_all(retro_dir());
        let _ = fs::write(marker(sid, "closed"), format!("ledger-only: {why}"));
        println!("retro closed (ledger only: {why})");
        0
    };
    let state = PathBuf::from(issue_inbox::state_root());
    let vault = match vault_note::VaultNotes::open(&state) {
        None => return ledger_only("this machine declares no vault"),
        Some(Err(e)) => {
            eprintln!("wrapup-stop-gate: --retro-close: vault を使えない({e}); 台帳に残す");
            return ledger_only("the declared vault is unusable");
        }
        Some(Ok(v)) => v,
    };
    let project = project_dir();
    let (nwo, pr) = retro_pr(sid, &project)
        .or_else(|| git_repository::origin_nwo(Path::new(&project)).map(|n| (n, String::new())))
        .unwrap_or_else(|| ("unknown".into(), String::new()));
    let date = issue_inbox::vault::jst_today();
    let g = match vault.lock() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("wrapup-stop-gate: --retro-close: vault をロックできない: {e}");
            return 3;
        }
    };
    let mut rel = note_rel(&date, &nwo, &pr, sid, None);
    // 同じ日に同じ PR を別のセッションが閉じていたら、そのノートを上書きしない。
    if let Ok(Some(text)) = g.read(&rel) {
        if vault_note::Note::parse(&text).get("session") != Some(sid) {
            rel = note_rel(&date, &nwo, &pr, sid, Some(&vault_note::short_hash(sid)));
        }
    }
    let note = vault_note::Note {
        front: vec![
            ("repo".into(), nwo.clone()),
            ("pr".into(), pr.clone()),
            ("session".into(), sid.to_string()),
            ("date".into(), date.clone()),
        ],
        body: render_body(&rows),
    };
    let text = note.render();
    if let Err(e) = g.replace(&rel, &text) {
        eprintln!("wrapup-stop-gate: --retro-close: ノートを書けない({rel}): {e}");
        return 3;
    }
    match g.read(&rel) {
        Ok(Some(back)) if back == text => {}
        other => {
            eprintln!(
                "wrapup-stop-gate: --retro-close: 読み戻した内容がノートと一致しない({rel}): {}",
                other.map_or_else(
                    |e| e.to_string(),
                    |o| o.map_or("無い".into(), |_| "違う".into())
                )
            );
            return 3;
        }
    }
    let _ = fs::create_dir_all(retro_dir());
    let _ = fs::write(marker(sid, "closed"), &rel);
    println!("retro closed (note: {rel})");
    0
}

pub fn cli(self_path: &str, args: &[String]) -> i32 {
    let need = |i: usize, text: &'static str| {
        args.get(i).filter(|a| !a.is_empty()).ok_or_else(|| {
            issue_inbox::usage_error(self_path, &issue_inbox::Usage { param: i + 1, text })
        })
    };
    let run = || -> Result<i32, i32> {
        Ok(match args.first().map(String::as_str) {
            Some("--retro-add") => {
                let text = "usage: wrapup-stop-gate --retro-add <session_id> <json>";
                retro_add(need(1, text)?, need(2, text)?)
            }
            Some("--retro-close") => {
                let text = "usage: wrapup-stop-gate --retro-close <session_id>";
                if let Some(extra) = args.get(2) {
                    eprintln!("session-retrospective: --retro-close: ignoring {extra} (nothing is posted to GitHub any more; ADR-1136)");
                }
                retro_close(need(1, text)?)
            }
            Some("--retro-procedure") => {
                let sid = need(1, "usage: wrapup-stop-gate --retro-procedure <session_id>")?;
                print!("{}", procedure_text(self_path, sid));
                0
            }
            Some("--retro-casebook") => {
                print!("{}", Vocabulary::get().casebook_markdown());
                0
            }
            _ => {
                eprintln!("session-retrospective: expected --retro-add, --retro-close, --retro-procedure or --retro-casebook and required arguments");
                1
            }
        })
    };
    run().unwrap_or_else(|code| code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1125: 手順書が案内するコマンドは、この実行ファイルの `cli` が受け付けるものだけ。
    /// 別の実行ファイルが持つ操作(かつての `--procedure`)を、自分のパスで案内しない。
    #[test]
    fn procedure_only_points_at_flags_the_cli_accepts() {
        let text = procedure_text("/bin/session-retrospective", "sid-1");
        let accepted = ["--retro-add", "--retro-close", "--retro-procedure"];
        let mut seen = 0;
        for word in text.split_whitespace() {
            if let Some(flag) = word.strip_prefix("--").map(|f| f.trim_end_matches('.')) {
                if flag.starts_with("retro-") {
                    seen += 1;
                    assert!(
                        accepted.contains(&format!("--{flag}").as_str()),
                        "unknown flag in the procedure: --{flag}"
                    );
                }
            }
        }
        assert!(
            seen >= 2,
            "the procedure should name --retro-add and --retro-close"
        );
        assert!(!text.contains("' --procedure"), "{text}");
    }

    #[test]
    fn iso_roundtrip() {
        let t = parse_iso_epoch("2026-10-02T01:02:03Z").unwrap();
        assert_eq!(t, 1_790_902_923);
        assert_eq!(parse_iso_epoch("2026-10-02 01:02:03"), None);
        let (y, m, d) = civil_from_days(t.div_euclid(86_400));
        assert_eq!((y, m, d), (2026, 10, 2));
    }

    /// 段 `gate` の行(兆候 repeated-violation の既定の段)。必須欄がすべて揃った正しい形。
    fn gate_row() -> Value {
        json!({"kind":"friction","what":"w","evidence":"e","disposition":"inbox","mechanism":"gate",
               "signal":"repeated-violation","existing":"拡張:pr-gate",
               "searched":"pipeline-map,crates,skills"})
    }

    #[test]
    fn a_level_row_needs_signal_existing_and_searched() {
        assert!(validate_row(&gate_row()).is_ok());
        for k in ["signal", "existing", "searched"] {
            let mut r = gate_row();
            r.as_object_mut().unwrap().remove(k);
            let e = validate_row(&r).unwrap_err();
            assert!(e.contains(k), "{k}: {e}");
        }
    }

    #[test]
    fn an_unknown_signal_or_existing_verdict_is_refused() {
        let mut r = gate_row();
        r["signal"] = json!("vibes");
        assert!(validate_row(&r).unwrap_err().contains("signal must be"));
        let mut r = gate_row();
        r["existing"] = json!("流用:x");
        assert!(validate_row(&r).unwrap_err().contains("existing must be"));
        let mut r = gate_row();
        r["existing"] = json!("拡張");
        assert!(validate_row(&r).unwrap_err().contains("requires `:<name>`"));
        let mut r = gate_row();
        r["existing"] = json!("自前:x");
        assert!(validate_row(&r).unwrap_err().contains("takes no name"));
    }

    #[test]
    fn a_level_other_than_the_signal_default_needs_a_reason() {
        let mut r = gate_row();
        r["mechanism"] = json!("tool"); // repeated-violation の既定は gate
        let e = validate_row(&r).unwrap_err();
        assert!(e.contains("level_reason"), "{e}");
        r["level_reason"] = json!("一回きりで gate は重い");
        assert!(validate_row(&r).is_ok());
    }

    #[test]
    fn the_in_repo_search_is_mandatory_and_own_code_needs_an_outside_search() {
        let mut r = gate_row();
        r["searched"] = json!("crates,skills");
        assert!(validate_row(&r).unwrap_err().contains("pipeline-map"));
        let mut r = gate_row();
        r["existing"] = json!("自前");
        let e = validate_row(&r).unwrap_err();
        assert!(e.contains("external search"), "{e}");
        r["searched"] = json!("pipeline-map,crates,skills,upstream:foo");
        assert!(validate_row(&r).is_ok());
    }

    #[test]
    fn script_is_retired_and_none_forbids_the_codify_fields() {
        let mut r = gate_row();
        r["mechanism"] = json!("script");
        let e = validate_row(&r).unwrap_err();
        assert!(e.contains("retired") && e.contains("tool"), "{e}");
        let mut n = gate_row();
        n["mechanism"] = json!("none:one-off");
        assert!(validate_row(&n).unwrap_err().contains("only valid"));
        let plain = json!({"kind":"friction","what":"w","evidence":"e","disposition":"inbox","mechanism":"none:one-off"});
        assert!(validate_row(&plain).is_ok());
    }

    #[test]
    fn a_prose_row_names_a_signal_but_builds_nothing() {
        let prose = json!({"kind":"friction","what":"w","evidence":"e","disposition":"inbox",
                           "mechanism":"prose","signal":"repeated-violation",
                           "level_reason":"機械から観測できない"});
        assert!(validate_row(&prose).is_ok());
        let mut r = prose.clone();
        r["existing"] = json!("自前");
        assert!(validate_row(&r).unwrap_err().contains("prose"));
        let mut r = prose.clone();
        r.as_object_mut().unwrap().remove("level_reason");
        assert!(validate_row(&r).unwrap_err().contains("level_reason"));
    }

    #[test]
    fn an_existing_row_may_carry_the_existing_field_only() {
        let mut r = json!({"kind":"friction","what":"gate X leaked","evidence":"e","disposition":"inbox","mechanism":"existing:gate X"});
        r["existing"] = json!("拡張:gate X");
        assert!(validate_row(&r).is_ok());
        r["searched"] = json!("crates");
        assert!(validate_row(&r).is_err());
    }

    #[test]
    fn the_procedure_is_generated_from_the_vocabulary() {
        let text = procedure_text("/bin/session-retrospective", "sid-1");
        let v = Vocabulary::get();
        for s in &v.signals {
            assert!(text.contains(&format!("signal {} (default {})", s.id, s.default_level)));
        }
        assert!(
            text.contains("eliminate|unrepresentable|gate|tool|prose"),
            "{text}"
        );
        assert!(text.contains("user-correction|insight|friction|gate-hit|codify|none|skipped"));
    }

    #[test]
    fn vocabulary() {
        let ok = gate_row();
        assert!(validate_row(&ok).is_ok());
        // user-correction は feedback: inbox には置けず、Issue か none:<理由>。
        let mut fb = gate_row();
        fb["kind"] = json!("user-correction");
        assert!(validate_row(&fb).unwrap_err().contains("Issue"));
        let mut fb_issue = fb.clone();
        fb_issue["disposition"] = json!("issue:#12");
        assert!(validate_row(&fb_issue).is_ok());
        fb_issue["disposition"] = json!("none:project-specific");
        assert!(validate_row(&fb_issue).is_ok());
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

    #[test]
    fn body_groups_rows_by_kind_and_escapes_the_table() {
        let rows = vec![
            json!({"kind":"insight","what":"a | b","evidence":"line1\nline2","disposition":"inbox","mechanism":"prose"}),
            json!({"kind":"user-correction","what":"w","evidence":"e","disposition":"issue:#9","mechanism":"gate"}),
            json!({"kind":"skipped","what":"s","evidence":"e","disposition":"none:r","quote":"不要"}),
        ];
        let body = render_body(&rows);
        let pos = |h: &str| body.find(h).unwrap_or_else(|| panic!("{h}\n{body}"));
        assert!(pos("## user-correction") < pos("## insight"));
        assert!(pos("## insight") < pos("## skipped"));
        assert!(
            body.contains("| a \\| b | line1 line2 | inbox | prose | - | - |"),
            "{body}"
        );
        assert!(body.contains("quote: 不要"), "{body}");
        assert!(!body.contains("## friction"));
    }

    #[test]
    fn note_names_carry_the_date_the_repo_and_the_pr() {
        assert_eq!(
            note_rel("2026-10-27", "o/r", "5", "abcdef012345", None),
            "retro/2026/2026-10-27-o-r-5.md"
        );
        assert_eq!(
            note_rel("2026-10-27", "o/r", "", "abcdef012345", None),
            "retro/2026/2026-10-27-o-r-sabcdef01.md"
        );
        assert!(note_rel("2026-10-27", "o/r", "5", "x", Some("h")).ends_with("-5 ~h.md"));
    }
}
