//! claude-usage の描画コア(#413、移植元 `config/claude/statusline/claude-usage.sh`)。
//!
//! bash 版は jq の 2 プログラム(EXTRACT_JQ / CORE_JQ)と `date -d` で描画していた。
//! ここではその評価規則を再現する:
//!
//! - jq の index: null は null、オブジェクト以外はエラー(エラーは EXTRACT 全体を `[]`
//!   に、CORE 全体を「描画なし・state 更新なし」に倒す — bash 版の `|| base_items='[]'`
//!   と `|| return 0`)
//! - 型をまたぐ比較は jq の全順序(null < false < true < 数値 < 文字列 < 配列 < オブジェクト)
//! - `resets_at` の解釈は `date -d` の代わりに jiff(RFC 3339 / ISO 8601。オフセットが
//!   無ければローカル時刻)、表示はローカル TZ(`TZ` 環境変数に従う)
//! - state file は jq `-c` と同じ書式(`{"last_fetch":N,"last_line":"..."}`)

use jiff::tz::TimeZone;
use jiff::Timestamp;
use serde_json::Value;
use std::path::Path;

/// 最終成功 fetch からこの秒数以内の失敗は last_line を再出力する(RFC 5861 の
/// stale-if-error と同型)。
pub const STALE_TTL: i64 = 900;

#[derive(Debug, PartialEq, Eq)]
pub struct JqError;

/// jq の `.key`(null / 欠落は Null、オブジェクト以外はエラー)。
fn index<'a>(v: &'a Value, key: &str) -> Result<&'a Value, JqError> {
    static NULL: Value = Value::Null;
    match v {
        Value::Null => Ok(&NULL),
        Value::Object(m) => Ok(m.get(key).unwrap_or(&NULL)),
        _ => Err(JqError),
    }
}

fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

/// `a // b`
fn alt<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) {
        a
    } else {
        b
    }
}

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(false) => 1,
        Value::Bool(true) => 2,
        Value::Number(_) => 3,
        Value::String(_) => 4,
        Value::Array(_) => 5,
        Value::Object(_) => 6,
    }
}

/// jq の `v >= 100`(型をまたぐ比較は型の順序で決まる)。
fn ge_100(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.as_f64().is_some_and(|x| x >= 100.0),
        other => type_rank(other) > 3,
    }
}

/// EXTRACT_JQ 適用後の 1 エントリ(日時変換前)。
#[derive(Debug, Clone, PartialEq)]
pub struct Base {
    pub kind: Value,
    pub percent: Value,
    pub resets_at: Value,
    pub exceeded: bool,
    pub label: Value,
}

fn extract_one(l: &Value) -> Result<Option<Base>, JqError> {
    // select(.percent != null and .resets_at != null)
    if index(l, "percent")?.is_null() || index(l, "resets_at")?.is_null() {
        return Ok(None);
    }
    let kind = alt(index(l, "kind")?, &Value::String("unknown".into())).clone();
    let percent = index(l, "percent")?.clone();
    let resets_at = index(l, "resets_at")?.clone();
    // ($l.percent >= 100) or (...) — or は短絡する
    let exceeded = ge_100(&percent) || {
        let sev = alt(index(l, "severity")?, &Value::String(String::new())).clone();
        let Value::String(s) = sev else {
            return Err(JqError); // ascii_downcase の型エラー
        };
        let s = s.to_ascii_lowercase();
        s.contains("exceed") || s.contains("block") || s.contains("critical")
    };
    let label = if kind == "session" {
        Value::String("5h".into())
    } else if kind == "weekly_scoped" {
        let dn = index(index(index(l, "scope")?, "model")?, "display_name")?;
        alt(dn, &Value::String("wk".into())).clone()
    } else if kind == "weekly" {
        Value::String("wk".into())
    } else {
        alt(index(l, "group")?, &kind).clone()
    };
    Ok(Some(Base {
        kind,
        percent,
        resets_at,
        exceeded,
        label,
    }))
}

/// EXTRACT_JQ。評価エラーは `[]`(bash 版の `|| base_items='[]'`)。
pub fn extract(usage: &Value) -> Vec<Base> {
    let run = || -> Result<Vec<Base>, JqError> {
        let limits = alt(index(usage, "limits")?, &Value::Null);
        let elems: Vec<&Value> = match limits {
            Value::Null | Value::Bool(false) => vec![],
            Value::Array(a) => a.iter().collect(),
            Value::Object(m) => m.values().collect(),
            _ => return Err(JqError),
        };
        let mut out = Vec::new();
        for e in elems {
            if let Some(b) = extract_one(e)? {
                out.push(b);
            }
        }
        Ok(out)
    };
    run().unwrap_or_default()
}

/// 日時変換後の 1 エントリ。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub base: Base,
    pub reset_epoch: i64,
    pub reset_display: String,
}

/// `date -d "$resets_at"` の代わり。オフセット付き(RFC 3339 / ISO 8601)を優先し、
/// 無ければ `tz` のローカル時刻として読む。読めなければ `None`(エントリを捨てる)。
pub fn parse_reset(s: &str, tz: &TimeZone) -> Option<Timestamp> {
    let s = s.trim();
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Some(ts);
    }
    let dt = s.parse::<jiff::civil::DateTime>().ok()?;
    tz.to_ambiguous_zoned(dt)
        .compatible()
        .ok()
        .map(|z| z.timestamp())
}

/// augment_items。`now` は epoch 秒。
pub fn augment(base: Vec<Base>, now: i64, tz: &TimeZone) -> Vec<Item> {
    base.into_iter()
        .filter_map(|b| {
            let Value::String(s) = &b.resets_at else {
                return None;
            };
            let ts = parse_reset(s, tz)?;
            let epoch = ts.as_second();
            let z = ts.to_zoned(tz.clone());
            let display = if epoch - now <= 86400 {
                format!("{:02}:{:02}", z.hour(), z.minute())
            } else {
                format!("{}/{}", z.month(), z.day())
            };
            Some(Item {
                base: b,
                reset_epoch: epoch,
                reset_display: display,
            })
        })
        .collect()
}

/// jq が計算結果の整数値 double を出すときの文字列(`round | tostring`)。
fn jq_int(x: f64) -> String {
    let x = if x.is_infinite() {
        f64::MAX.copysign(x)
    } else {
        x
    };
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').expect("{:e} has an exponent");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp.parse::<i64>().expect("exponent") + 1;
    let n = digits.len() as i64;
    let body = if decpt <= -4 || decpt > n + 15 {
        let mut s = digits[..1].to_string();
        if n > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        let ex = decpt - 1;
        s.push('e');
        s.push(if ex < 0 { '-' } else { '+' });
        if ex.abs() < 10 {
            s.push('0');
        }
        s.push_str(&ex.abs().to_string());
        s
    } else if decpt >= n {
        format!("{digits}{}", "0".repeat((decpt - n) as usize))
    } else if decpt > 0 {
        format!(
            "{}.{}",
            &digits[..decpt as usize],
            &digits[decpt as usize..]
        )
    } else {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    };
    format!("{sign}{body}")
}

fn window_for(kind: &Value) -> Option<i64> {
    if kind == "session" {
        Some(18000)
    } else if kind == "weekly_scoped" || kind == "weekly" {
        Some(604800)
    } else {
        None
    }
}

/// CORE_JQ。評価エラーは `Err`(bash 版は描画も state 更新もせずに抜ける)。
pub fn core(items: &[Item], now: i64) -> Result<String, JqError> {
    let mut segs = Vec::new();
    for it in items {
        let win = window_for(&it.base.kind);
        let show = !it.base.exceeded
            && win.is_some_and(|w| {
                it.reset_epoch > now && ((now - (it.reset_epoch - w)) as f64) >= (w as f64) * 0.05
            });
        let pct = it.base.percent.as_f64().ok_or(JqError)?;
        let suffix = if show {
            let w = win.expect("show implies a window") as f64;
            let elapsed = (now - (it.reset_epoch - w as i64)) as f64 / w;
            let landing = (pct / elapsed).round();
            format!(
                " {}{}%",
                if landing >= 100.0 { "▲" } else { "▼" },
                jq_int(landing)
            )
        } else {
            String::new()
        };
        let label = match &it.base.label {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            _ => return Err(JqError), // 文字列との + の型エラー
        };
        let base = format!("{label} {}%→{}", jq_int(pct.round()), it.reset_display);
        segs.push(if it.base.exceeded {
            format!("!{base}")
        } else {
            base + &suffix
        });
    }
    Ok(segs.join(" · "))
}

/// jq の文字列エンコード(`-c` 出力)。
pub fn jq_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push('\\');
                out.push('u');
                out.push_str(&format!("{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `$(...)` と同じく末尾の改行を落とす。
pub fn subst(s: &str) -> String {
    s.trim_end_matches('\n').to_string()
}

/// `jq -e .` が成功する(有効な JSON 1 値で、null / false でない)なら値を返す。
pub fn parse_truthy(s: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(s).ok()?;
    truthy(&v).then_some(v)
}

/// `jq -r '<path> // <alt>'` の 1 段(state file の `.last_fetch` / `.last_line`)。
/// 値の raw 表示は文字列ならそのまま、整数ならその桁、それ以外は compact JSON。
pub fn raw_field(v: &Value, key: &str) -> Result<Option<String>, JqError> {
    let f = index(v, key)?;
    if !truthy(f) {
        return Ok(None);
    }
    Ok(Some(match f {
        Value::String(s) => subst(s),
        other => other.to_string(),
    }))
}

/// `last_fetch` が `*[!0-9]*` に当たらない(= 非負の 10 進整数表記)なら値。
pub fn digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(s.parse::<i64>().unwrap_or(i64::MAX))
}

fn state_json(now: i64, line: &str) -> String {
    format!("{{\"last_fetch\":{now},\"last_line\":{}}}", jq_string(line))
}

/// state file を一時ファイル + rename で atomic に更新する(mktemp と同じく 0600)。
fn write_state(state: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = state
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mut last = None;
    for i in 0..16u32 {
        let tmp = dir.join(format!(
            "claude-usage-tabbar.{}{:06}",
            std::process::id(),
            (nanos as u64 + i as u64 * 7919) % 1_000_000
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(mut f) => {
                f.write_all(body.as_bytes())?;
                drop(f);
                return std::fs::rename(&tmp, state);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no temp name")))
}

/// render_from_files。描画行があれば `"<line>\n"` を返し(無ければ空)、成功時は
/// state file を更新する。失敗・評価エラーは何もしない。
pub fn render_from_files(usage_file: &Path, state_file: &Path, now: &str) -> String {
    let Ok(now) = now.parse::<i64>() else {
        return String::new();
    };
    let Ok(bytes) = std::fs::read(usage_file) else {
        return String::new();
    };
    let text = subst(&String::from_utf8_lossy(&bytes));
    let Some(usage) = parse_truthy(&text) else {
        return String::new();
    };
    let tz = TimeZone::system();
    let items = augment(extract(&usage), now, &tz);
    let Ok(line) = core(&items, now) else {
        return String::new();
    };
    let line = subst(&line);
    if write_state(state_file, &state_json(now, &line)).is_err() {
        return String::new();
    }
    if line.is_empty() {
        String::new()
    } else {
        format!("{line}\n")
    }
}

/// emit_stale。最終成功 fetch から STALE_TTL 秒以内なら last_line を返す。
pub fn emit_stale(state_file: &Path, now: i64) -> String {
    let Ok(meta) = std::fs::metadata(state_file) else {
        return String::new();
    };
    if meta.len() == 0 {
        return String::new();
    }
    let Ok(bytes) = std::fs::read(state_file) else {
        return String::new();
    };
    let Some(v) = parse_truthy(&subst(&String::from_utf8_lossy(&bytes))) else {
        return String::new();
    };
    let Ok(Some(lf)) = raw_field(&v, "last_fetch") else {
        return String::new();
    };
    let Some(lf) = digits(&lf) else {
        return String::new();
    };
    let age = now.saturating_sub(lf);
    if !(0..=STALE_TTL).contains(&age) {
        return String::new();
    }
    match raw_field(&v, "last_line") {
        Ok(Some(l)) if !l.is_empty() => format!("{l}\n"),
        _ => String::new(),
    }
}

/// 30 秒の再取得ガード。ガードに掛かれば出力(空もありうる)を返す。
pub fn guard(state_file: &Path, now: i64) -> Option<String> {
    let meta = std::fs::metadata(state_file).ok()?;
    if meta.len() == 0 {
        return None;
    }
    let prev = std::fs::read(state_file)
        .map(|b| subst(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let v = parse_truthy(&prev)?;
    let lf = match raw_field(&v, "last_fetch") {
        Ok(Some(s)) => digits(&s).unwrap_or(0),
        Ok(None) => 0, // `// 0`
        Err(_) => 0,
    };
    if now.saturating_sub(lf) >= 30 {
        return None;
    }
    Some(match raw_field(&v, "last_line") {
        Ok(Some(l)) if !l.is_empty() => format!("{l}\n"),
        _ => String::new(),
    })
}

/// `.claudeAiOauth.accessToken // empty` の raw。読めない・無ければ空。
pub fn read_token(cred: &Path) -> String {
    let Ok(bytes) = std::fs::read(cred) else {
        return String::new();
    };
    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
        return String::new();
    };
    let Ok(o) = index(&v, "claudeAiOauth") else {
        return String::new();
    };
    match raw_field(o, "accessToken") {
        Ok(Some(t)) => t,
        _ => String::new(),
    }
}

/// state file の `.last_fetch // empty` の raw(読めなければ空)。
pub fn state_last_fetch(state_file: &Path) -> String {
    let Ok(bytes) = std::fs::read(state_file) else {
        return String::new();
    };
    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
        return String::new();
    };
    match raw_field(&v, "last_fetch") {
        Ok(Some(s)) => s,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn utc() -> TimeZone {
        TimeZone::UTC
    }

    #[test]
    fn extract_rules() {
        let u = json!({"limits":[
            {"kind":"session","percent":21,"resets_at":"x","severity":"normal"},
            {"kind":"weekly_scoped","percent":5,"resets_at":"x","scope":null},
            {"kind":"other","group":"g","percent":5,"resets_at":"x"},
            {"percent":null,"resets_at":"x"},
            null
        ]});
        let b = extract(&u);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].label, "5h");
        assert_eq!(b[1].label, "wk");
        assert_eq!(b[2].label, "g");
        // severity が文字列でなければ EXTRACT 全体がエラー → []
        assert!(
            extract(&json!({"limits":[{"percent":5,"resets_at":"x","severity":3}]})).is_empty()
        );
        // percent >= 100 なら severity は評価しない(or の短絡)
        assert_eq!(
            extract(&json!({"limits":[{"percent":100,"resets_at":"x","severity":3}]})).len(),
            1
        );
        assert!(extract(&json!({"limits":"x"})).is_empty());
        assert!(extract(&json!(5)).is_empty());
    }

    #[test]
    fn reset_parsing() {
        let tz = utc();
        assert_eq!(
            parse_reset("2026-09-01T09:19:59.944701+00:00", &tz)
                .unwrap()
                .as_second(),
            1788254399
        );
        assert!(parse_reset("not a date", &tz).is_none());
        assert_eq!(
            parse_reset("2023-11-14T22:13:20Z", &tz)
                .unwrap()
                .as_second(),
            1_700_000_000
        );
    }

    #[test]
    fn core_landing() {
        let tz = utc();
        let now = 1_700_000_000;
        let b = extract(
            &json!({"limits":[{"kind":"session","percent":21,"resets_at":"2023-11-14T23:13:20Z"}]}),
        );
        let items = augment(b, now, &tz);
        assert_eq!(core(&items, now).unwrap(), "5h 21%→23:13 ▼26%");
    }

    #[test]
    fn jq_strings() {
        let bs = '\\';
        assert_eq!(jq_string("a\"\u{7f}é"), format!("\"a{bs}\"{bs}u007fé\""));
        assert_eq!(jq_int(26.0), "26");
        assert_eq!(jq_int(-0.0), "-0");
    }
}
