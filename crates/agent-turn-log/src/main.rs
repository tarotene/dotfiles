//! agent-turn-log — Claude Code / Codex / Copilot の 1 ターン境界
//! (UserPromptSubmit / Stop)ごとに JSONL を 1 行追記する(#413 で
//! `config/claude/hooks/agent-turn-log.sh` から移植)。
//!
//! 出力は別リポジトリ(daily-report)が取り込む。このリポジトリの義務は
//! 出力契約だけで(docs/adr/0011)、パスとフィールド名・順序は下流が依存する:
//!
//! ```text
//! ${XDG_STATE_HOME:-$HOME/.local/state}/daily-report/agent-events.jsonl
//! {"kind":"prompt","agent":..,"ts":..,"session_id":..,"prompt_id":..,"cwd":..,"prompt":..}
//! {"kind":"turn_end","agent":..,"ts":..,"session_id":..}
//! ```
//!
//! イベントは argv ではなく stdin の `.hook_event_name` で見分ける(同じ
//! command を両イベントに登録する)。`AGENT_NAME`(既定 `claude-code`)で
//! エージェント名を差し替え、`AGENT_TURN_LOG=0` なら何もしない
//! (copilot-plan-review.sh が機械起動の Copilot に付ける)。
//!
//! 常に exit 0(fail-open、ADR-0005)。prompt 本文は出力ファイル以外の
//! どこにも出さない(argv・環境変数・stdout/stderr のいずれにも)。

use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// jq と同じ規則で JSON 文字列リテラルを書く(制御文字と DEL は `\u00xx`)。
fn jq_string(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                o.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
}

/// `jq -c` の 1 値。文字列は jq のエスケープ規則で書く。文字列以外
/// (prompt が文字列でない異常系だけ)は serde_json の表現で近似する
/// (object のキー順は jq の挿入順ではなく辞書順になる)。
fn jq_compact(v: &Value, o: &mut String) {
    match v {
        Value::String(s) => jq_string(s, o),
        Value::Array(items) => {
            o.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                jq_compact(it, o);
            }
            o.push(']');
        }
        Value::Object(m) => {
            o.push('{');
            for (i, (k, it)) in m.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                jq_string(k, o);
                o.push(':');
                jq_compact(it, o);
            }
            o.push('}');
        }
        other => o.push_str(&other.to_string()),
    }
}

/// jq -r が 1 値を出力するときの文字列。
fn jq_raw(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string_pretty(v).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// `jq -r '.a // .b // empty' file` を `$(...)` で受けた値。stdin の各 JSON 値
/// について最初の null/false でない候補を出力し、最後に末尾の改行を落とす。
fn query(docs: &[Value], keys: &[&str]) -> String {
    let mut out = String::new();
    for d in docs {
        if let Some(v) = keys
            .iter()
            .map(|k| &d[*k])
            .find(|v| !matches!(v, Value::Null | Value::Bool(false)))
        {
            out.push_str(&jq_raw(v));
            out.push('\n');
        }
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// `date -u +%Y-%m-%dT%H:%M:%SZ`。
fn utc_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil_from_days(Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn out_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("daily-report"))
}

/// 1 行ぶんの JSON object を組み立てる(フィールド順 = 出力契約の順)。
struct Line(String);

impl Line {
    fn field(&mut self, name: &str, value: &str) {
        self.0.push(if self.0.is_empty() { '{' } else { ',' });
        jq_string(name, &mut self.0);
        self.0.push(':');
        jq_string(value, &mut self.0);
    }
}

fn run() -> Option<()> {
    if std::env::var_os("AGENT_TURN_LOG").as_deref() == Some("0".as_ref()) {
        return None;
    }
    let agent = std::env::var("AGENT_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "claude-code".to_string());

    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    // jq と同じく不正な UTF-8 は U+FFFD に置き換え、JSON 値の並びとして読む。
    // 1 つでも壊れていれば(bash 版の `|| exit 0` と同じく)何もしない。
    let text = String::from_utf8_lossy(&raw);
    let docs: Vec<Value> = serde_json::Deserializer::from_str(&text)
        .into_iter::<Value>()
        .collect::<Result<_, _>>()
        .ok()?;
    // `.hook_event_name` は object / null 以外に対してはエラーになる
    if docs.iter().any(|d| !(d.is_object() || d.is_null())) {
        return None;
    }

    let event = query(&docs, &["hook_event_name"]);
    if event != "UserPromptSubmit" && event != "Stop" {
        return None;
    }

    let dir = out_dir()?;
    let file = dir.join("agent-events.jsonl");
    fs::create_dir_all(&dir).ok()?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    if !file.exists() {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)
            .ok()?;
    }
    let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o600));

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let ts = utc_timestamp(now.as_secs());
    let session_id = query(&docs, &["session_id"]);

    let mut line = Line(String::new());
    if event == "UserPromptSubmit" {
        let cwd = query(&docs, &["cwd"]);
        let mut prompt_id = query(&docs, &["prompt_id", "turn_id"]);
        if prompt_id.is_empty() {
            // bash 版の `$(date +%s%N)-$$`
            prompt_id = format!(
                "{}{:09}-{}",
                now.as_secs(),
                now.subsec_nanos(),
                std::process::id()
            );
        }
        line.field("kind", "prompt");
        line.field("agent", &agent);
        line.field("ts", &ts);
        line.field("session_id", &session_id);
        line.field("prompt_id", &prompt_id);
        line.field("cwd", &cwd);
        // `$payload[0].prompt // ""`: 先頭の JSON 値の prompt をそのまま埋め込む
        line.0.push_str(",\"prompt\":");
        match docs.first().map(|d| &d["prompt"]) {
            Some(v) if !matches!(v, Value::Null | Value::Bool(false)) => jq_compact(v, &mut line.0),
            _ => line.0.push_str("\"\""),
        }
    } else {
        line.field("kind", "turn_end");
        line.field("agent", &agent);
        line.field("ts", &ts);
        line.field("session_id", &session_id);
    }
    line.0.push_str("}\n");

    let mut f = OpenOptions::new().append(true).open(&file).ok()?;
    f.write_all(line.0.as_bytes()).ok()
}

fn main() {
    let _ = run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_format() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(1_790_812_799), "2026-09-30T23:59:59Z");
    }

    #[test]
    fn query_semantics() {
        let docs = vec![
            serde_json::json!({"a": null, "b": "x\n\n"}),
            serde_json::json!({"a": false}),
            Value::Null,
        ];
        assert_eq!(query(&docs, &["a", "b"]), "x");
        assert_eq!(query(&docs, &["zz"]), "");
        let docs = vec![serde_json::json!({"a": 1}), serde_json::json!({"a": "y"})];
        assert_eq!(query(&docs, &["a"]), "1\ny");
    }

    #[test]
    fn jq_compact_escaping() {
        let mut o = String::new();
        jq_compact(&serde_json::json!("a\u{7f}\u{1}\"\\"), &mut o);
        assert_eq!(o, r#""a\u007f\u0001\"\\""#);
    }
}
