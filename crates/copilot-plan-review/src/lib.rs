//! copilot-plan-review — ExitPlanMode 直前に GitHub Copilot CLI でプランを自動
//! レビューする hook(旧 `config/claude/hooks/copilot-plan-review.sh`、ADR-0024
//! Stage 4b、#412)。設計と根拠は docs/claude/copilot-plan-review.md。
//!
//! critic / judge / acceptance oracle の分離、ラウンド上限・closer・escalation、
//! 書式 gate precheck、fail-open の各規則は bash 版と同じ。judge・レンダリングは
//! [`judge`] に、プロンプト本文は [`prompts`] に逐語で写している。
//!
//! 使い方:
//!   hook として:      PreToolUse (matcher: ExitPlanMode) から stdin JSON で呼ばれる
//!   advisory として:  `copilot-plan-review --advisory <plan.md> [cwd]`
//!
//! bash 版との差: `--selftest` は `cargo test` に移した。書式 gate precheck の
//! sibling の既定パスは `argv[0]` と同じディレクトリの `plan-precedent-gate` /
//! `plan-scope-gate`(拡張子なし、Rust 版の配備名)で、`bash` を介さず直接起動する。

pub mod judge;
pub mod prompts;

use hook_io::jqfmt;
use hook_io::plan::{plan_source, PlanSource};
use hook_io::proc::{command_exists, date, spawn_group, wait_with_timeout};
use judge::{chomp, Judged};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// `${VAR:-default}`。
fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// 実行時設定(bash 版のグローバル変数)。
#[derive(Debug, Clone)]
pub struct Config {
    pub review_dir: String,
    pub copilot_bin: String,
    pub max_reviews_raw: String,
    pub max_reviews: i64,
    pub timeout_raw: String,
    pub timeout: Duration,
    pub gate_severities: String,
    pub parallel: String,
    pub retention_days: String,
    pub model: String,
    pub agent: String,
    pub precedent_bin: String,
    pub scope_bin: String,
}

/// `timeout(1)` の DURATION(数値 + 任意の s/m/h/d)。
fn parse_duration(s: &str) -> Option<Duration> {
    let (num, mult) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1.0),
        'm' => (&s[..s.len() - 1], 60.0),
        'h' => (&s[..s.len() - 1], 3600.0),
        'd' => (&s[..s.len() - 1], 86400.0),
        _ => (s, 1.0),
    };
    let n: f64 = num.parse().ok()?;
    (n >= 0.0 && n.is_finite()).then(|| Duration::from_secs_f64(n * mult))
}

/// argv[0] のディレクトリ(bash 版の SCRIPT_DIR 相当)。`/` を含まなければ
/// `$HOME/.claude/hooks`。
fn self_dir() -> String {
    let argv0 = std::env::args().next().unwrap_or_default();
    if argv0.contains('/') {
        let p = Path::new(&argv0);
        let dir = p.parent().unwrap_or(Path::new("."));
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        dir.to_string_lossy().into_owned()
    } else {
        format!("{}/.claude/hooks", home())
    }
}

impl Config {
    pub fn from_env() -> Self {
        let review_dir = env_or(
            "COPILOT_PLAN_REVIEW_DIR",
            &format!("{}/.claude/plan-reviews", home()),
        );
        let max_reviews_raw = env_or("MAX_PLAN_REVIEWS", "3");
        let timeout_raw = env_or("COPILOT_PLAN_REVIEW_TIMEOUT", "280");
        let dir = self_dir();
        Config {
            review_dir,
            copilot_bin: env_or("COPILOT_BIN", "copilot"),
            max_reviews: max_reviews_raw.trim().parse().unwrap_or(3),
            max_reviews_raw,
            timeout: parse_duration(&timeout_raw).unwrap_or(Duration::from_secs(280)),
            timeout_raw,
            gate_severities: env_or("COPILOT_PLAN_REVIEW_GATE_SEVERITIES", "BLOCKER,MAJOR"),
            parallel: env_or("COPILOT_PLAN_REVIEW_PARALLEL", "1"),
            retention_days: env_or("COPILOT_PLAN_REVIEW_RETENTION_DAYS", "30"),
            model: env_or("COPILOT_PLAN_REVIEW_MODEL", "gpt-6-astra"),
            agent: env_or("COPILOT_PLAN_REVIEW_AGENT", "plan-reviewer"),
            precedent_bin: env_or(
                "PLAN_PRECEDENT_GATE_BIN",
                &format!("{dir}/plan-precedent-gate"),
            ),
            scope_bin: env_or("PLAN_SCOPE_GATE_BIN", &format!("{dir}/plan-scope-gate")),
        }
    }

    fn state_dir(&self) -> String {
        format!("{}/state", self.review_dir)
    }

    fn backlog_dir(&self) -> String {
        format!("{}/backlog", self.review_dir)
    }
}

/// 本家 gate の register 登録値(home/modules/claude.nix)に合わせた precheck の timeout。
const PLAN_PRECEDENT_GATE_TIMEOUT: Duration = Duration::from_secs(10);
const PLAN_SCOPE_GATE_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// ディレクトリ衛生
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(mode));
}

/// `find dir -maxdepth 2 -type f ! -name skip` の対象ファイル。
fn files_depth2(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = fs::symlink_metadata(&p) else {
            continue;
        };
        if md.is_file() {
            out.push(p);
        } else if md.is_dir() {
            if let Ok(rd2) = fs::read_dir(&p) {
                for e2 in rd2.flatten() {
                    let p2 = e2.path();
                    if fs::symlink_metadata(&p2).is_ok_and(|m| m.is_file()) {
                        out.push(p2);
                    }
                }
            }
        }
    }
    out.retain(|p| p.file_name().is_some_and(|n| n != "skip"));
    out
}

/// `ensure_dirs`。
pub fn ensure_dirs(cfg: &Config) {
    let _ = fs::create_dir_all(cfg.state_dir());
    let _ = fs::create_dir_all(cfg.backlog_dir());
    for d in [cfg.review_dir.clone(), cfg.state_dir(), cfg.backlog_dir()] {
        chmod(Path::new(&d), 0o700);
    }
    use std::os::unix::fs::PermissionsExt;
    for f in files_depth2(Path::new(&cfg.review_dir)) {
        if fs::metadata(&f).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
            chmod(&f, 0o600);
        }
    }
}

/// `prune_old`: 保持期限(日)より古い生成物を消す。skip は消さない。
pub fn prune_old(cfg: &Config) {
    let days = &cfg.retention_days;
    if days.is_empty() || days == "0" || !days.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    let Ok(n) = days.parse::<u64>() else {
        return;
    };
    let now = SystemTime::now();
    for f in files_depth2(Path::new(&cfg.review_dir)) {
        let Ok(mtime) = fs::symlink_metadata(&f).and_then(|m| m.modified()) else {
            continue;
        };
        // find -mtime +N: 経過日数(切り捨て)が N より大きい
        let age_days = now
            .duration_since(mtime)
            .map(|d| d.as_secs() / 86400)
            .unwrap_or(0);
        if age_days > n {
            let _ = fs::remove_file(&f);
        }
    }
}

/// `mktemp "$dir/<prefix>XXXXXX<suffix>"`。
fn mktemp(dir: &str, prefix: &str, suffix: &str) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (u64::from(std::process::id()) << 20);
    const CH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..100 {
        let mut name = String::new();
        for _ in 0..6 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            name.push(CH[((seed >> 33) % CH.len() as u64) as usize] as char);
        }
        let path = format!("{dir}/{prefix}{name}{suffix}");
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .is_ok()
        {
            return Some(path);
        }
    }
    None
}

/// `ls -t "$dir"/*.md | head -1`(隠しファイルは glob に入らない)。
fn latest_md(dir: &str) -> Option<String> {
    let rd = fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, String)> = None;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".md") {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            continue;
        }
        let Ok(t) = md.modified() else { continue };
        let better = match &best {
            None => true,
            Some((bt, bn)) => t > *bt || (t == *bt && name < *bn),
        };
        if better {
            best = Some((t, name));
        }
    }
    best.map(|(_, n)| format!("{dir}/{n}"))
}

// ---------------------------------------------------------------------------
// プロンプト
// ---------------------------------------------------------------------------

fn prompt_lens(lens: &str) -> &'static str {
    match lens {
        "A" => prompts::LENS_A,
        "B" => prompts::LENS_B,
        "C" => prompts::LENS_C,
        "Z" => prompts::LENS_Z,
        _ => prompts::LENS_M,
    }
}

fn jq_plus(o: &Value, k: &str) -> String {
    match o.get(k) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

fn jq_alt(o: &Value, k: &str) -> String {
    match o.get(k) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

fn prompt_carryover(open: &[Value]) -> String {
    if open.is_empty() {
        return prompts::CARRYOVER_NONE.to_string();
    }
    let mut s = prompts::CARRYOVER_HEAD.to_string();
    for o in open {
        s += &format!(
            "- id: {} [{}] {}\n  失敗モード: {}\n  根拠: {}\n",
            jq_plus(o, "id"),
            jq_plus(o, "severity"),
            jq_plus(o, "summary"),
            jq_alt(o, "failure_mode"),
            jq_alt(o, "evidence"),
        );
    }
    s
}

/// `build_prompt`(コマンド置換と同じく末尾改行を落とす)。
pub fn build_prompt(lens: &str, plan_file: &str, open: &[Value]) -> String {
    let s = format!(
        "{}{}{}",
        prompts::PREAMBLE.replace("{plan_file}", plan_file),
        prompt_lens(lens),
        prompt_carryover(open)
    );
    chomp(&s).to_string()
}

// ---------------------------------------------------------------------------
// critic 実行
// ---------------------------------------------------------------------------

/// `cd -L` 相当の論理パス正規化(`.` と `..` を字句的に畳む)。
fn lexical(base: &Path, rel: &Path) -> PathBuf {
    let joined = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        base.join(rel)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

/// critic を 1 本回す。終了コード 0 なら true。
fn run_critic(
    cfg: &Config,
    lens: &str,
    plan_file: &str,
    workdir: &str,
    out: &str,
    open: &[Value],
) -> bool {
    let prompt = build_prompt(lens, plan_file, open);
    let cwd0 = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let cur_dir = if Path::new(workdir).is_dir() {
        lexical(&cwd0, Path::new(workdir))
    } else if Path::new(&home()).is_dir() {
        lexical(&cwd0, Path::new(&home()))
    } else {
        return false;
    };
    let plan_parent = Path::new(plan_file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let plan_dir_path = lexical(&cur_dir, plan_parent);
    let mut add_dir: Vec<String> = Vec::new();
    if plan_dir_path.is_dir() {
        let plan_dir = plan_dir_path.to_string_lossy().into_owned();
        let cur = cur_dir.to_string_lossy().into_owned();
        if !(plan_dir == cur || plan_dir.starts_with(&format!("{cur}/"))) {
            add_dir = vec!["--add-dir".into(), plan_dir];
        }
    }
    let Ok(out_file) = File::create(out) else {
        return false;
    };
    let mut cmd = Command::new(&cfg.copilot_bin);
    cmd.current_dir(&cur_dir)
        .env("AGENT_TURN_LOG", "0")
        .arg("-p")
        .arg(&prompt)
        .args(["--agent", &cfg.agent, "--model", &cfg.model])
        .args([
            "--silent",
            "--no-custom-instructions",
            "--disable-builtin-mcps",
            "--no-ask-user",
        ])
        .args(&add_dir)
        .stdin(Stdio::null())
        .stdout(out_file)
        .stderr(Stdio::null());
    let Ok(mut child) = spawn_group(&mut cmd) else {
        return false;
    };
    wait_with_timeout(&mut child, cfg.timeout).is_some_and(|s| s.success())
}

/// `run_critics` の結果: 成功した critic(`{lens, data}`)と失敗 lens。
#[derive(Debug, Default)]
pub struct CriticRun {
    pub critics: Vec<Value>,
    pub failed: Vec<String>,
}

pub fn run_critics(
    cfg: &Config,
    plan_file: &str,
    workdir: &str,
    open: &[Value],
    lenses: &[&str],
) -> CriticRun {
    let outs: Vec<Option<String>> = lenses
        .iter()
        .map(|_| mktemp(&cfg.review_dir, ".copilot-out.", ""))
        .collect();
    let rcs: Vec<bool> = if cfg.parallel == "1" && lenses.len() > 1 {
        std::thread::scope(|s| {
            let hs: Vec<_> = lenses
                .iter()
                .zip(&outs)
                .map(|(lens, out)| {
                    s.spawn(move || {
                        out.as_deref()
                            .is_some_and(|o| run_critic(cfg, lens, plan_file, workdir, o, open))
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap_or(false)).collect()
        })
    } else {
        lenses
            .iter()
            .zip(&outs)
            .map(|(lens, out)| {
                out.as_deref()
                    .is_some_and(|o| run_critic(cfg, lens, plan_file, workdir, o, open))
            })
            .collect()
    };
    let mut run = CriticRun::default();
    for ((lens, out), ok) in lenses.iter().zip(&outs).zip(rcs) {
        let data = out
            .as_deref()
            .and_then(|o| fs::read_to_string(o).ok())
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .filter(|v| !matches!(v, Value::Null | Value::Bool(false)));
        match data {
            Some(d) if ok && judge::valid_critic(&d) => {
                run.critics.push(json!({"lens": lens, "data": d}));
            }
            _ => run.failed.push(lens.to_string()),
        }
        if let Some(o) = out {
            let _ = fs::remove_file(o);
        }
    }
    run
}

// ---------------------------------------------------------------------------
// advisory モード
// ---------------------------------------------------------------------------

/// `--advisory <plan.md> [cwd]`。終了コードを返す。
pub fn advisory(cfg: &Config, args: &[String]) -> i32 {
    ensure_dirs(cfg);
    let Some(plan) = args.first().filter(|s| !s.is_empty()) else {
        eprintln!("copilot-plan-review: 2: usage: copilot-plan-review --advisory <plan.md> [cwd]");
        return 1;
    };
    let workdir = args
        .get(1)
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            std::env::var("PWD").unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
        });
    if !command_exists(&cfg.copilot_bin) {
        eprintln!(
            "copilot が見つかりません (COPILOT_BIN={})。",
            cfg.copilot_bin
        );
        return 1;
    }
    let lenses: &[&str] = if cfg.parallel == "1" {
        &["A", "B"]
    } else {
        &["M"]
    };
    let run = run_critics(cfg, plan, &workdir, &[], lenses);
    if run.critics.is_empty() {
        eprintln!("copilot レビューの実行に失敗しました（タイムアウト・未ログイン・モデル利用不可・ネットワーク等）。");
        return 1;
    }
    let j = judge::judge("1", &[], &cfg.gate_severities, &run.critics);
    println!("{}", judge::render_log(&j));
    if !run.failed.is_empty() {
        eprintln!(
            "（注意: lens {} が失敗したため残りの critic のみで判定しています）",
            run.failed.join(", ")
        );
    }
    let w = judge::warn_text(&j.warn);
    if !w.is_empty() {
        eprintln!("（注意: {w}）");
    }
    0
}

// ---------------------------------------------------------------------------
// hook モード
// ---------------------------------------------------------------------------

/// hook の stdout と終了コード(常に 0)。
fn pass_through(msg: &str) -> String {
    if msg.is_empty() {
        String::new()
    } else {
        jqfmt::system_message(msg)
    }
}

/// jq `-r` で取り出した値の文字列表現(null は "null")。
fn jq_raw(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// `.k // "d"` を `jq -r` で取り出した文字列。
fn jq_alt_raw(v: Option<&Value>, d: &str) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => d.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// `precheck_deny_reason`: sibling gate が deny を返せばその理由。
fn precheck_deny_reason(bin: &str, limit: Duration, input: &str) -> Option<String> {
    if !Path::new(bin).is_file() {
        return None;
    }
    // `<<<"$INPUT"`: INPUT は `$(cat)` で末尾改行が落ち、here-string が 1 個足す。
    let stdin = format!("{}\n", chomp(input));
    let (st, out) =
        hook_io::proc::output_with_timeout(&mut Command::new(bin), limit, Some(stdin.as_bytes()))?;
    if !st.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&out);
    if chomp(&out).is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&out).ok()?;
    let hso = v.get("hookSpecificOutput");
    let pick = |a: &str, b: &[&str]| -> Option<String> {
        let first = hso.and_then(|h| h.get(a));
        let second = b.iter().try_fold(hso?, |acc, k| acc.get(*k));
        match first {
            Some(x) if !matches!(x, Value::Null | Value::Bool(false)) => Some(jq_raw(Some(x))),
            _ => match second {
                Some(x) if !matches!(x, Value::Null | Value::Bool(false)) => Some(jq_raw(Some(x))),
                _ => None,
            },
        }
    };
    let decision = pick("permissionDecision", &["decision", "behavior"]).unwrap_or_default();
    if decision != "deny" {
        return None;
    }
    let reason = pick("permissionDecisionReason", &["decision", "message"]).unwrap_or_default();
    let reason = chomp(&reason).to_string();
    (!reason.is_empty()).then_some(reason)
}

/// ファイルへ書く(0600 で作る。既存なら切り詰め)。
fn write_file(path: &str, content: &str) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(content.as_bytes()))
        .is_ok()
}

fn append_file(path: &str, content: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(content.as_bytes()));
}

struct Hook<'a> {
    cfg: &'a Config,
    event: String,
    backlog_file: String,
    escalated_file: String,
}

impl Hook<'_> {
    fn deny(&self, reason: &str) -> String {
        jqfmt::deny_for_event(&self.event, reason)
    }

    /// `escalate_with`。
    fn escalate(&self, open: &[Value], situation: &str, log_path: &str) -> String {
        let residual = chomp(&judge::blocks(open)).to_string();
        let _ = write_file(&self.escalated_file, "");
        let log = if log_path.is_empty() {
            self.cfg.review_dir.as_str()
        } else {
            log_path
        };
        self.deny(&format!(
            "{situation}実装をブロックする指摘が {} 件未解消のまま残っています。

追加のレビューは行いません。**AskUserQuestion で、この状態のまま実装に進んでよいか (GO / NO-GO) をユーザーに確認すること。** あなたの判断で未解消のまま進めてはならない。

- GO なら、そのまま再度 ExitPlanMode を呼ぶ（次回は素通ります）。
- NO-GO なら、ExitPlanMode を呼ばずにプランの修正を続けること。

--- 未解消の指摘 ---

{residual}
MINOR/NIT は {} を参照。レビュー全文: {log}",
            open.len(),
            self.backlog_file
        ))
    }
}

/// `${warn:+\n\n（注意: $warn）}`。
fn warn_suffix(warn: &str) -> String {
    if warn.is_empty() {
        String::new()
    } else {
        format!("\n\n（注意: {warn}）")
    }
}

/// hook モード本体。stdin の生文字列を受け、stdout に出す文字列を返す。
pub fn hook(cfg: &Config, raw_input: &str) -> String {
    let input_text = chomp(raw_input);
    let parsed: Option<Value> = serde_json::from_str(input_text).ok();
    let v = parsed.clone().unwrap_or(Value::Null);
    let event = if parsed.is_some() {
        jq_alt_raw(v.get("hook_event_name"), "PreToolUse")
    } else {
        String::new()
    };
    let session_raw = if parsed.is_some() {
        jq_alt_raw(v.get("session_id"), "unknown")
    } else {
        String::new()
    };
    let session_id = hook_io::ledger::sanitize_session_id(&session_raw);
    let cwd_raw = if parsed.is_some() {
        match v.get("cwd") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
            x => jq_raw(x),
        }
    } else {
        String::new()
    };
    let cwd = if !cwd_raw.is_empty() && Path::new(&cwd_raw).is_dir() {
        cwd_raw
    } else {
        home()
    };

    // デバッグ用ダンプはメタデータのみ。プラン本文と transcript_path は残さない。
    if parsed.is_some() {
        let ti = v.get("tool_input");
        let plan_chars = match ti.and_then(|t| t.get("plan")) {
            Some(Value::String(s)) => s.chars().count(),
            _ => 0,
        };
        let has_pfp = match ti.and_then(|t| t.get("planFilePath")) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(_) => true,
        };
        let g = |k: &str| v.get(k).cloned().unwrap_or(Value::Null);
        let dump = jqfmt::J::obj(vec![
            ("session_id", (&g("session_id")).into()),
            ("cwd", (&g("cwd")).into()),
            ("hook_event_name", (&g("hook_event_name")).into()),
            ("tool_name", (&g("tool_name")).into()),
            ("permission_mode", (&g("permission_mode")).into()),
            ("plan_chars", jqfmt::J::Num(plan_chars.to_string())),
            ("has_plan_file_path", jqfmt::J::Bool(has_pfp)),
        ]);
        let _ = write_file(
            &format!("{}/debug-last-input.json", cfg.review_dir),
            &format!("{}\n", dump.pretty()),
        );
    } else {
        // bash 版: jq が失敗しても `>` のリダイレクトでファイルは空になる。
        let _ = write_file(&format!("{}/debug-last-input.json", cfg.review_dir), "");
    }

    let state_dir = cfg.state_dir();
    let h = Hook {
        cfg,
        event,
        backlog_file: format!("{}/{session_id}.md", cfg.backlog_dir()),
        escalated_file: format!("{state_dir}/{session_id}.escalated"),
    };

    // --- copilot 不在のマシンでは黙って素通り(ADR-0005) ---
    if !command_exists(&cfg.copilot_bin) {
        return pass_through("");
    }
    // --- エスケープハッチ ---
    if Path::new(&format!("{}/skip", cfg.review_dir)).exists()
        || std::env::var("SKIP_PLAN_REVIEW").is_ok_and(|s| s == "1")
    {
        return pass_through("");
    }

    // --- 書式 gate precheck ---
    let mut precheck =
        precheck_deny_reason(&cfg.precedent_bin, PLAN_PRECEDENT_GATE_TIMEOUT, input_text)
            .map(|r| ("plan-precedent-gate", r));
    if precheck.is_none() {
        precheck = precheck_deny_reason(&cfg.scope_bin, PLAN_SCOPE_GATE_TIMEOUT, input_text)
            .map(|r| ("plan-scope-gate", r));
    }
    if let Some((gate, reason)) = precheck {
        return h.deny(&format!(
            "(copilot-plan-review: {gate} の書式 gate precheck による deny。このラウンドのレビューは未実行・未消費です)\n\n{reason}"
        ));
    }

    let count_file = format!("{state_dir}/{session_id}.count");
    let open_file = format!("{state_dir}/{session_id}.open.json");
    let count: i64 = fs::read_to_string(&count_file)
        .ok()
        .map(|s| chomp(&s).to_string())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let open_set: Vec<Value> = fs::read_to_string(&open_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    // --- 既にエスカレーション済み → 無条件に素通る ---
    if Path::new(&h.escalated_file).exists() {
        let latest = latest_md(&cfg.review_dir).unwrap_or_else(|| cfg.review_dir.clone());
        return pass_through(&format!(
            "Copilot プランレビュー: このセッションでは既に人間の GO/NO-GO を要求したため素通しします。直近のレビュー: {latest}"
        ));
    }

    // --- ラウンド上限 ---
    let max_raw = &cfg.max_reviews_raw;
    if count >= cfg.max_reviews {
        let latest = latest_md(&cfg.review_dir).unwrap_or_default();
        if !open_set.is_empty() {
            return h.escalate(
                &open_set,
                &format!("Copilot プランレビューの上限 ({max_raw} ラウンド) に到達しましたが、"),
                &latest,
            );
        }
        let latest = if latest.is_empty() {
            cfg.review_dir.clone()
        } else {
            latest
        };
        return pass_through(&format!(
            "Copilot プランレビュー: このセッションの上限 ({max_raw} ラウンド) に達したため素通しします。直近のレビュー: {latest}"
        ));
    }

    // --- プラン本文の取得 ---
    let hook_input = hook_io::HookInput::parse(input_text).unwrap_or_default();
    let mut plan_tmp: Option<String> = None;
    let plan_file = match plan_source(&hook_input) {
        Some(PlanSource::Inline(t)) => {
            let Some(p) = mktemp(&cfg.review_dir, ".plan.", ".md") else {
                return pass_through("");
            };
            let _ = write_file(&p, &format!("{}\n", chomp(&t)));
            plan_tmp = Some(p.clone());
            p
        }
        Some(PlanSource::File(p)) => p.to_string_lossy().into_owned(),
        None => {
            return pass_through("Copilot プランレビュー: プラン本文を取得できなかったためスキップしました（tool_input.plan / planFilePath なし、~/.claude/plans/ も空）。");
        }
    };

    let round = count + 1;
    let closer = round >= cfg.max_reviews && round > 1;
    let lenses: &[&str] = if closer {
        &["Z"]
    } else if round == 1 {
        if cfg.parallel == "1" {
            &["A", "B"]
        } else {
            &["M"]
        }
    } else {
        &["C"]
    };

    let run = run_critics(cfg, &plan_file, &cwd, &open_set, lenses);
    if let Some(p) = plan_tmp {
        let _ = fs::remove_file(p);
    }
    if run.critics.is_empty() {
        return pass_through(&format!(
            "Copilot プランレビュー: 実行に失敗しました（タイムアウト {}s・未ログイン・ネットワーク等）。fail-open で通過させます。ラウンドは消費していません。",
            cfg.timeout_raw
        ));
    }
    let round_s = round.to_string();
    let judged: Judged = if closer {
        let mut j = judge::judge(&round_s, &open_set, "", &run.critics);
        j.suppress_closer_warn();
        j
    } else {
        judge::judge(&round_s, &open_set, &cfg.gate_severities, &run.critics)
    };

    // --- ここから先はレビューが成立した = ラウンドを消費する ---
    let _ = write_file(&count_file, &format!("{round}\n"));
    let ts = date("%Y%m%d-%H%M%S");
    let sid8: String = session_id.chars().take(8).collect();
    let review_log = format!("{}/{ts}-{sid8}.md", cfg.review_dir);
    let review_json = format!("{}/{ts}-{sid8}.json", cfg.review_dir);
    let _ = write_file(&review_log, &format!("{}\n", judge::render_log(&judged)));
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap_or_default();
    let _ = write_file(
        &review_json,
        &format!(
            "{}\n",
            pretty(&json!({"judged": judged.to_value(), "critics": run.critics}))
        ),
    );
    let _ = write_file(
        &open_file,
        &format!("{}\n", pretty(&Value::Array(judged.open.clone()))),
    );
    let backlog_md = chomp(&judge::render_backlog(&judged)).to_string();
    if !backlog_md.is_empty() {
        append_file(&h.backlog_file, &format!("{backlog_md}\n"));
    }

    let mut warn = chomp(&judge::warn_text(&judged.warn)).to_string();
    if !run.failed.is_empty() {
        let lead = if warn.is_empty() {
            String::new()
        } else {
            format!("{warn}。")
        };
        warn = format!(
            "{lead}lens {} が失敗したため残りの critic のみで判定しました",
            run.failed.join(", ")
        );
    }
    let backlog_file = &h.backlog_file;
    let open_n = judged.open.len();
    let backlog_n = judged.backlog.len();

    if judged.gate {
        if closer {
            return h.escalate(
                &judged.open,
                "Copilot プランレビューの最終ラウンド (closer) で改訂プランに対して再判定した結果、",
                &review_log,
            );
        }
        return h.deny(&format!(
            "Copilot によるプランレビューの結果、実装をブロックする指摘が {open_n} 件あります（ラウンド {round}/{max_raw}、新規 {} 件 / 前ラウンドから未解消 {} 件）。

対応の方針:

- **BLOCKER / MAJOR だけが対応対象**です。MINOR/NIT は {backlog_file} に退避済みで、いま直す必要はありません。
- [TECHNICAL] の指摘: リポジトリ等の証拠で検証し、妥当なら反映してください。**反証できた指摘は、反証の根拠をプランに明記して却下してよい**（却下は正当な帰結です）。
- [NEEDS_DECISION] の指摘: 勝手に採否を判断してプランに反映してはいけません。レビュアーの意見はユーザーの決定ではありません。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てから修正してください。
- 「もっと良いプランが存在する」ことは指摘理由になりません。任意の改善提案として書かれているものがあれば無視してよい。

対応が済んでから再度 ExitPlanMode を呼んでください。次のラウンドでは、ここに挙がった各指摘が解消されたか / 反証されたか / 未解消かが判定されます。{}

--- 未解消の指摘 ---

{}
レビュー全文: {review_log}",
            judged.new_eligible.len(),
            judged.carried.len(),
            warn_suffix(&warn),
            chomp(&judge::blocks(&judged.open)),
        ));
    }

    if closer {
        let serious_n = judged
            .backlog
            .iter()
            .filter(|f| {
                matches!(
                    f.get("severity").and_then(Value::as_str),
                    Some("BLOCKER" | "MAJOR")
                )
            })
            .count();
        let blocker_n = judged
            .backlog
            .iter()
            .filter(|f| f.get("severity").and_then(Value::as_str) == Some("BLOCKER"))
            .count();
        let mut note = format!(
            "前ラウンドの指摘はすべて解消 / 却下されました（closer ラウンド {round}/{max_raw}）。"
        );
        if serious_n != 0 {
            note += &format!("closer は carry-over 判定専用なので、新たに報告された BLOCKER/MAJOR {serious_n} 件（うち BLOCKER {blocker_n} 件）は gate 対象にせず backlog に退避しました。実装前に {backlog_file} を確認してください。");
        }
        return pass_through(&format!(
            "Copilot プランレビュー: {note}backlog は計 {backlog_n} 件。全文: {review_log}{}",
            warn_suffix(&warn)
        ));
    }

    pass_through(&format!(
        "Copilot プランレビュー: 実装をブロックする指摘はありません（ラウンド {round}/{max_raw}）。MINOR/NIT {backlog_n} 件は {backlog_file} に退避しました。全文: {review_log}{}",
        warn_suffix(&warn)
    ))
}

/// エントリポイント。終了コードを返す。
pub fn run(args: &[String]) -> i32 {
    // 生成物はプラン本文由来のテキストを含むため 0600 / 0700 に落とす(umask 077)。
    // SAFETY: umask はプロセス全体の属性を書き換えるだけで、メモリ安全性に関与しない。
    unsafe {
        libc::umask(0o077);
    }
    let cfg = Config::from_env();
    if args.first().map(String::as_str) == Some("--advisory") {
        return advisory(&cfg, &args[1..]);
    }
    if args.first().map(String::as_str) == Some("--selftest") {
        eprintln!(
            "copilot-plan-review: --selftest は cargo test -p copilot-plan-review に移行しました"
        );
        return 2;
    }
    ensure_dirs(&cfg);
    prune_old(&cfg);
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let out = hook(&cfg, &raw);
    print!("{out}");
    0
}
