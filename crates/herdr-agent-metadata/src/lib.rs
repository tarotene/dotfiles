//! Claude Code / Codex CLI / Copilot CLI の状態を Herdr サイドバーに流す hook
//! (#413、ADR-0024 Stage 4d)。
//!
//! 移植元(3 本の POSIX sh + python3 heredoc):
//! - `config/claude/hooks/herdr-claude-metadata.sh` — permission mode(+ branch/oshi)
//! - `config/codex/hooks/herdr-codex-metadata.sh` — model(+ branch/oshi)
//! - `config/copilot/hooks/herdr-copilot-metadata.sh` — model(settings.json 由来)
//!   (+ branch/oshi)。action は argv(`report` / `clear`)
//!
//! 入力の差は [`hook_io::Agent`] で選ぶ `plan` の分岐に閉じ込め、socket 送信は
//! [`hook_io::herdr`] に任せる。送信 JSON は python 版 `json.dumps(request)` と
//! バイト一致させる([`pyjson`]、ensure_ascii と `", "` / `": "` 区切り)。
//! 詳細は docs/claude/herdr-sidebar-metadata.md。

pub mod pyjson;

use hook_io::Agent;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// jq の `(.<key> // "") | tostring`。null / false / 欠落は空文字、文字列は
/// そのまま、それ以外は compact JSON。
fn jq_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `gsub("[\r\n]"; " ")`。
fn flatten(s: String) -> String {
    s.replace(['\r', '\n'], " ")
}

/// `$(...)` の末尾改行除去。
fn strip_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// stdin JSON を jq と同じ縮退で読む。空入力は `null` 扱い(jq は何も出さず
/// 全フィールドが空になる)。オブジェクトでも null でもない値・不正 JSON は
/// `None`(jq がエラーになり bash 版は exit 0)。
pub fn parse_payload(input: &str) -> Option<Value> {
    if input.trim().is_empty() {
        return Some(Value::Null);
    }
    match serde_json::from_str::<Value>(input).ok()? {
        v @ (Value::Object(_) | Value::Null) => Some(v),
        _ => None,
    }
}

/// Claude / Codex の payload 4 フィールド(bash 版 `parse_payload`)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fields {
    pub event: String,
    /// Claude は `permission_mode`、Codex は `model`。
    pub value: String,
    pub subagent: bool,
    pub cwd: String,
}

pub fn fields(payload: &Value, value_key: &str) -> Fields {
    Fields {
        event: flatten(jq_field(payload, "hook_event_name")),
        value: flatten(jq_field(payload, value_key)),
        subagent: !matches!(
            payload.get("agent_id"),
            None | Some(Value::Null) | Some(Value::Bool(false))
        ),
        cwd: flatten(jq_field(payload, "cwd")),
    }
}

/// `jq -r '.<key> // ""'`(Copilot の payload_cwd / settings_model)。
fn jq_raw(payload: &Value, key: &str) -> String {
    strip_trailing_newlines(&jq_field(payload, key)).to_string()
}

/// `git -C <cwd> <args>` の stdout(末尾改行除去)。失敗は空文字。
fn git_out(cwd: &str, args: &[&str]) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| strip_trailing_newlines(&String::from_utf8_lossy(&o.stdout)).to_string())
        .unwrap_or_default()
}

/// 現在のブランチ名。`worktree/` プレフィクスは表示幅節約のため落とす。
pub fn branch_for(cwd: &str) -> String {
    let b = git_out(cwd, &["branch", "--show-current"]);
    b.strip_prefix("worktree/").map(str::to_string).unwrap_or(b)
}

/// worktree トップレベルのディレクトリ名 `worktree-<talent>-<hex4>` から
/// talent を取り出す(sh の `case worktree-*-*` + `${x#worktree-}` + `${x%-*}`)。
pub fn talent_of(toplevel: &str) -> Option<&str> {
    let base = toplevel.rsplit('/').next().unwrap_or("");
    let rest = base.strip_prefix("worktree-")?;
    let cut = rest.rfind('-')?;
    Some(&rest[..cut])
}

/// `awk -F'\t' -v n=<talent> '!/^#/ && $1==n {print $2; exit}' <marks>`。
pub fn lookup_mark(marks: &str, talent: &str) -> String {
    for line in marks.split('\n') {
        if line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        if cols.next() == Some(talent) {
            return cols.next().unwrap_or("").to_string();
        }
    }
    String::new()
}

/// `${XDG_CONFIG_HOME:-$HOME/.config}/herdr/oshi-marks.tsv`。
fn marks_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("herdr/oshi-marks.tsv")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// 推しマーク(ファンマーク)絵文字。非 worktree・未登録は空文字。
pub fn oshi_for(cwd: &str) -> String {
    let top = git_out(cwd, &["rev-parse", "--show-toplevel"]);
    oshi_for_toplevel(&top, &marks_path())
}

pub fn oshi_for_toplevel(top: &str, marks: &Path) -> String {
    let Some(talent) = talent_of(top) else {
        return String::new();
    };
    if !marks.is_file() {
        return String::new();
    }
    match std::fs::read(marks) {
        Ok(b) => lookup_mark(&String::from_utf8_lossy(&b), talent),
        Err(_) => String::new(),
    }
}

/// 送る内容。`tokens` は送信順。値 `None` は JSON の null(トークンのクリア)。
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub source: &'static str,
    pub tokens: Vec<(&'static str, Option<String>)>,
    pub ttl: bool,
    /// 送信に成功したら状態ファイルへ書く値(Claude のみ)。
    pub remember: Option<String>,
}

fn opt(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// 送信前の副作用(Claude の状態ファイル)を伴う判定の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    Skip,
    Send(Report),
}

const LABELS: [(&str, &str, &str); 4] = [
    ("plan", "mode_plan", "\u{25c7} plan"),
    ("default", "mode_default", "\u{25c6} default"),
    ("acceptEdits", "mode_accept", "\u{2713} accept"),
    ("bypassPermissions", "mode_bypass", "\u{25b2} bypass"),
];

/// Claude: permission mode をモード別トークンに振り分ける。
/// `last_mode` は状態ファイルの中身(無ければ `None`)。
pub fn plan_claude(
    f: &Fields,
    last_mode: Option<&str>,
    branch: impl FnOnce() -> String,
    oshi: impl FnOnce() -> String,
) -> Plan {
    if f.subagent {
        return Plan::Skip;
    }
    let mode = f.value.as_str();
    let end = match f.event.as_str() {
        "SessionEnd" => true,
        "SessionStart" | "Stop" => false,
        "UserPromptSubmit" | "PreToolUse" => {
            if last_mode == Some(mode) {
                return Plan::Skip;
            }
            false
        }
        _ => return Plan::Skip,
    };
    let mut tokens: Vec<(&'static str, Option<String>)> =
        LABELS.iter().map(|(_, name, _)| (*name, None)).collect();
    // bash 版は SessionEnd でも branch/oshi を取ってから python 側で捨てる。
    // 結果は同じなので、ここでは取らない。
    let (b, o) = if end || f.cwd.is_empty() {
        (String::new(), String::new())
    } else {
        (branch(), oshi())
    };
    tokens.push(("branch", if end { None } else { opt(b) }));
    tokens.push(("oshi", if end { None } else { opt(o) }));
    if !end {
        if let Some((_, name, label)) = LABELS.iter().find(|(m, _, _)| *m == mode) {
            set(&mut tokens, name, label.to_string());
        } else if !mode.is_empty() {
            // 未知のモード(auto / dontAsk など)は default 用トークンに実名で流す。
            set(&mut tokens, "mode_default", format!("\u{25c6} {mode}"));
        }
    }
    Plan::Send(Report {
        source: "claude-hook",
        tokens,
        ttl: !end,
        remember: (!end).then(|| mode.to_string()),
    })
}

fn set(tokens: &mut [(&'static str, Option<String>)], name: &str, v: String) {
    if let Some(t) = tokens.iter_mut().find(|(n, _)| *n == name) {
        t.1 = Some(v);
    }
}

/// Codex: model(+ branch/oshi)。debounce は持たない。
pub fn plan_codex(
    f: &Fields,
    branch: impl FnOnce() -> String,
    oshi: impl FnOnce() -> String,
) -> Plan {
    if f.subagent {
        return Plan::Skip;
    }
    let end = match f.event.as_str() {
        "SessionEnd" => true,
        "SessionStart" | "UserPromptSubmit" | "Stop" => false,
        _ => return Plan::Skip,
    };
    let (b, o) = if end || f.cwd.is_empty() {
        (String::new(), String::new())
    } else {
        (branch(), oshi())
    };
    // bash 版の挙動のまま: SessionEnd でも payload の model は送る。
    Plan::Send(Report {
        source: "codex-hook",
        tokens: vec![
            ("model", opt(f.value.clone())),
            ("branch", opt(b)),
            ("oshi", opt(o)),
        ],
        ttl: !end,
        remember: None,
    })
}

/// Copilot: action は argv(`report` / `clear`)、model は settings.json から読む。
pub fn plan_copilot(
    action: &str,
    payload: Option<&Value>,
    settings: impl FnOnce() -> String,
    branch: impl FnOnce(&str) -> String,
    oshi: impl FnOnce(&str) -> String,
) -> Plan {
    let report = match action {
        "report" => true,
        "clear" => false,
        _ => return Plan::Skip,
    };
    let (mut model, mut b, mut o) = (String::new(), String::new(), String::new());
    if report {
        let cwd = payload.map(|p| jq_raw(p, "cwd")).unwrap_or_default();
        if !cwd.is_empty() {
            b = branch(&cwd);
            o = oshi(&cwd);
        }
        model = settings();
    }
    Plan::Send(Report {
        source: "copilot-hook",
        tokens: vec![("model", opt(model)), ("branch", opt(b)), ("oshi", opt(o))],
        ttl: report,
        remember: None,
    })
}

/// `$HOME/.copilot/settings.json` の `.model`。欠落・不正 JSON は空文字。
pub fn copilot_settings_model() -> String {
    let p = home().join(".copilot/settings.json");
    if !p.is_file() {
        return String::new();
    }
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| parse_payload(&s))
        .map(|v| jq_raw(&v, "model"))
        .unwrap_or_default()
}

/// python 版 `json.dumps(request)` と同じ 1 行。
pub fn request_line(r: &Report, pane_id: &str, id: &str, seq: u128) -> String {
    let mut s = String::new();
    s.push_str("{\"id\": ");
    pyjson::push_str(&mut s, id);
    s.push_str(", \"method\": \"pane.report_metadata\", \"params\": {\"pane_id\": ");
    pyjson::push_str(&mut s, pane_id);
    s.push_str(", \"source\": ");
    pyjson::push_str(&mut s, r.source);
    s.push_str(&format!(", \"seq\": {seq}, \"tokens\": {{"));
    for (i, (k, v)) in r.tokens.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        pyjson::push_str(&mut s, k);
        s.push_str(": ");
        match v {
            Some(v) => pyjson::push_str(&mut s, v),
            None => s.push_str("null"),
        }
    }
    s.push('}');
    if r.ttl {
        s.push_str(&format!(", \"ttl_ms\": {}", hook_io::herdr::TTL_MS));
    }
    s.push_str("}}");
    s
}

/// hook 本体。戻り値は無く、常に exit 0(失敗は無音)。
pub fn run(agent: Agent, action: &str, input: &str) {
    if agent == Agent::Copilot && !matches!(action, "report" | "clear") {
        return;
    }
    let Some((socket, pane)) = hook_io::herdr::pane_env() else {
        return;
    };
    let payload = parse_payload(input);
    let mut state_file = None;
    let plan = match agent {
        Agent::Claude => {
            let Some(p) = payload else { return };
            let f = fields(&p, "permission_mode");
            if f.subagent {
                return;
            }
            let sf = hook_io::herdr::pane_state_file("herdr-claude-mode", &pane);
            let last = match f.event.as_str() {
                "SessionEnd" => {
                    let _ = std::fs::remove_file(&sf);
                    None
                }
                _ if sf.is_file() => Some(
                    std::fs::read(&sf)
                        .map(|b| strip_trailing_newlines(&String::from_utf8_lossy(&b)).to_string())
                        .unwrap_or_default(),
                ),
                _ => None,
            };
            let cwd = f.cwd.clone();
            let plan = plan_claude(&f, last.as_deref(), || branch_for(&cwd), || oshi_for(&cwd));
            state_file = Some(sf);
            plan
        }
        Agent::Codex => {
            let Some(p) = payload else { return };
            let f = fields(&p, "model");
            let cwd = f.cwd.clone();
            plan_codex(&f, || branch_for(&cwd), || oshi_for(&cwd))
        }
        Agent::Copilot => plan_copilot(
            action,
            payload.as_ref(),
            copilot_settings_model,
            branch_for,
            oshi_for,
        ),
    };
    let Plan::Send(report) = plan else { return };
    let line = request_line(
        &report,
        &pane,
        &hook_io::herdr::request_id(report.source),
        hook_io::herdr::seq_ns(),
    );
    if hook_io::herdr::send_line(&socket, &line).is_ok() {
        if let (Some(sf), Some(mode)) = (state_file, report.remember) {
            let _ = std::fs::write(sf, mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn f(event: &str, value: &str, cwd: &str) -> Fields {
        Fields {
            event: event.into(),
            value: value.into(),
            subagent: false,
            cwd: cwd.into(),
        }
    }

    #[test]
    fn fields_follow_jq_semantics() {
        let p = json!({"hook_event_name":"Stop","permission_mode":false,"agent_id":0,"cwd":"a\nb"});
        let got = fields(&p, "permission_mode");
        assert_eq!(got.event, "Stop");
        assert_eq!(got.value, "");
        assert!(got.subagent, "0 は jq では truthy");
        assert_eq!(got.cwd, "a b");
        let got = fields(&json!({"model": 5}), "model");
        assert_eq!(got.value, "5");
        assert_eq!(fields(&Value::Null, "model"), Fields::default());
    }

    #[test]
    fn parse_payload_degrades_like_jq() {
        assert_eq!(parse_payload(""), Some(Value::Null));
        assert_eq!(parse_payload(" \n"), Some(Value::Null));
        assert_eq!(parse_payload("null"), Some(Value::Null));
        assert!(parse_payload("{").is_none());
        assert!(parse_payload("[1]").is_none());
        assert!(parse_payload("\"x\"").is_none());
    }

    #[test]
    fn talent_extraction() {
        assert_eq!(talent_of("/w/worktree-suisei-ab12"), Some("suisei"));
        assert_eq!(
            talent_of("/w/worktree-hakos-baelz-ab12"),
            Some("hakos-baelz")
        );
        assert_eq!(talent_of("/w/worktree--"), Some(""));
        assert_eq!(talent_of("/w/worktree-x"), None);
        assert_eq!(talent_of("/w/dotfiles"), None);
        assert_eq!(talent_of(""), None);
    }

    #[test]
    fn mark_lookup() {
        let m = "# c\nsuisei\tA\textra\nmarine\nx\tB";
        assert_eq!(lookup_mark(m, "suisei"), "A");
        assert_eq!(lookup_mark(m, "marine"), "");
        assert_eq!(lookup_mark(m, "x"), "B");
        assert_eq!(lookup_mark(m, "# c"), "");
        assert_eq!(lookup_mark(m, "none"), "");
    }

    #[test]
    fn claude_debounce_only_on_frequent_events() {
        let none = String::new;
        assert_eq!(
            plan_claude(&f("PreToolUse", "plan", ""), Some("plan"), none, none),
            Plan::Skip
        );
        assert!(matches!(
            plan_claude(&f("Stop", "plan", ""), Some("plan"), none, none),
            Plan::Send(_)
        ));
        assert_eq!(
            plan_claude(&f("Notification", "plan", ""), None, none, none),
            Plan::Skip
        );
    }

    #[test]
    fn claude_does_not_look_up_branch_without_cwd() {
        let p = plan_claude(
            &f("Stop", "", ""),
            None,
            || panic!("branch"),
            || panic!("oshi"),
        );
        assert!(matches!(p, Plan::Send(_)));
    }

    #[test]
    fn request_line_matches_python_dumps() {
        let r = Report {
            source: "codex-hook",
            tokens: vec![("model", Some("gpt-5".into())), ("branch", None)],
            ttl: true,
            remember: None,
        };
        assert_eq!(
            request_line(&r, "p\u{e9}", "codex-hook:1:000002", 3),
            "{\"id\": \"codex-hook:1:000002\", \"method\": \"pane.report_metadata\", \"params\": {\"pane_id\": \"p\\u00e9\", \"source\": \"codex-hook\", \"seq\": 3, \"tokens\": {\"model\": \"gpt-5\", \"branch\": null}, \"ttl_ms\": 14400000}}"
        );
    }

    #[test]
    fn copilot_clear_skips_lookups() {
        let p = plan_copilot(
            "clear",
            None,
            || panic!("settings"),
            |_| panic!("branch"),
            |_| panic!("oshi"),
        );
        let Plan::Send(r) = p else { panic!() };
        assert!(!r.ttl);
        assert!(r.tokens.iter().all(|(_, v)| v.is_none()));
        assert_eq!(
            plan_copilot("x", None, String::new, |_| String::new(), |_| String::new()),
            Plan::Skip
        );
    }
}
