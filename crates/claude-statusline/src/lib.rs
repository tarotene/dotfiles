//! claude-statusline の純粋な部分(#413、移植元 `config/claude/statusline/claude-statusline.sh`)。
//!
//! bash 版は jq / bash の printf / python3 の `json.dumps` に処理を任せていたため、
//! 表示のバイト列はそれぞれの癖で決まっていた。ここではその癖を明示的に再現する:
//!
//! - [`parse_payload`]: jq プログラム(`.model.display_name // .model.id // ""` ほか 6 項目)
//!   の評価。null の index は null、オブジェクト以外の index はエラー(= 何も出さない)、
//!   数値の `tostring` は jq 1.7+ のリテラル保持([`jq_number_literal`])
//! - [`printf_2f`]: `LC_ALL=C printf '%.2f' "$cost" || printf '%s' "$cost"`(bash の
//!   builtin printf は long double で丸める)
//! - [`py_json_str`]: python の `json.dumps`(ensure_ascii)
//! - [`posix_dirname`] / [`posix_basename`]

use serde_json::value::RawValue;
use std::collections::HashMap;

/// jq の評価エラー(bash 版では `parse_payload || exit 0` で何も出さずに終わる)。
#[derive(Debug, PartialEq, Eq)]
pub struct JqError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Null,
    False,
    True,
    Number,
    String,
    Array,
    Object,
}

fn kind(v: &RawValue) -> Kind {
    match v.get().trim_start().as_bytes().first() {
        Some(b'n') => Kind::Null,
        Some(b'f') => Kind::False,
        Some(b't') => Kind::True,
        Some(b'"') => Kind::String,
        Some(b'[') => Kind::Array,
        Some(b'{') => Kind::Object,
        _ => Kind::Number,
    }
}

/// jq の `.key`。null / 欠落は `None`、オブジェクト以外はエラー。
fn index(v: Option<&RawValue>, key: &str) -> Result<Option<Box<RawValue>>, JqError> {
    let Some(v) = v else { return Ok(None) };
    match kind(v) {
        Kind::Null => Ok(None),
        Kind::Object => {
            let m: HashMap<String, Box<RawValue>> =
                serde_json::from_str(v.get()).map_err(|_| JqError)?;
            Ok(m.get(key)
                .map(|r| r.to_owned())
                .filter(|r| kind(r) != Kind::Null))
        }
        _ => Err(JqError),
    }
}

/// jq の `a // b` の左辺判定(null と false 以外)。
fn truthy(v: &Option<Box<RawValue>>) -> bool {
    matches!(v, Some(r) if !matches!(kind(r), Kind::Null | Kind::False))
}

/// jq 1.7+ が数値リテラルをそのまま出すときの文字列(decNumber の to-scientific-string)。
/// 例: `1.50` → `1.50`、`1e2` → `1E+2`、`0.1e1` → `1`、`5e-7` → `5E-7`。
pub fn jq_number_literal(lit: &str) -> String {
    let lit = lit.trim();
    let (neg, rest) = match lit.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, lit),
    };
    let (mant, exp) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], rest[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (rest, 0),
    };
    let (int, frac) = match mant.find('.') {
        Some(i) => (&mant[..i], &mant[i + 1..]),
        None => (mant, ""),
    };
    let mut digits: String = format!("{int}{frac}").trim_start_matches('0').to_string();
    if digits.is_empty() {
        digits.push('0');
    }
    let exponent = exp - frac.len() as i64;
    let n = digits.len() as i64;
    let adjusted = exponent + n - 1;
    let body = if exponent <= 0 && adjusted >= -6 {
        if exponent == 0 {
            digits
        } else {
            let pos = n + exponent;
            if pos > 0 {
                format!("{}.{}", &digits[..pos as usize], &digits[pos as usize..])
            } else {
                format!("0.{}{}", "0".repeat((-pos) as usize), digits)
            }
        }
    } else {
        let mut s = digits[..1].to_string();
        if n > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('E');
        s.push(if adjusted >= 0 { '+' } else { '-' });
        s.push_str(&adjusted.abs().to_string());
        s
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// jq が計算結果の double を出すときの文字列(`jvp_dtoa_fmt`: 最短桁、
/// `decpt <= -4 || decpt > 桁数 + 15` なら指数表記)。
pub fn jq_double(x: f64) -> String {
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
        let a = ex.abs();
        if a < 10 {
            s.push('0');
        }
        s.push_str(&a.to_string());
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

/// jq の `tostring`。
fn tostring(v: &RawValue) -> String {
    match kind(v) {
        Kind::String => serde_json::from_str::<String>(v.get()).unwrap_or_default(),
        Kind::Number => jq_number_literal(v.get()),
        Kind::Null => "null".into(),
        Kind::True => "true".into(),
        Kind::False => "false".into(),
        // 配列・オブジェクトは compact JSON(キー順・数値の書式は jq と細部で異なりうる)
        Kind::Array | Kind::Object => serde_json::from_str::<serde_json::Value>(v.get())
            .map(|x| x.to_string())
            .unwrap_or_default(),
    }
}

/// statusline JSON から取り出す 6 項目(bash 版の `parse_payload` の変数)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Payload {
    pub model: String,
    pub ctx: String,
    pub cost: String,
    pub effort: String,
    pub fast: String,
    pub project_dir: String,
}

fn or_else(
    first: Option<Box<RawValue>>,
    rest: impl FnOnce() -> Result<Option<Box<RawValue>>, JqError>,
) -> Result<Option<Box<RawValue>>, JqError> {
    if truthy(&first) {
        Ok(first)
    } else {
        rest()
    }
}

fn eval_one(root: &RawValue) -> Result<Payload, JqError> {
    let r = Some(root);
    let model_obj = index(r, "model")?;
    let model = or_else(index(model_obj.as_deref(), "display_name")?, || {
        index(model_obj.as_deref(), "id")
    })?;
    let ctx_v = index(index(r, "context_window")?.as_deref(), "used_percentage")?;
    let ctx = if truthy(&ctx_v) {
        let v = ctx_v.unwrap();
        if kind(&v) != Kind::Number {
            return Err(JqError); // jq: round の型エラー
        }
        let x: f64 = v.get().trim().parse().map_err(|_| JqError)?;
        jq_double(x.round())
    } else {
        String::new()
    };
    let cost_v = index(index(r, "cost")?.as_deref(), "total_cost_usd")?;
    let effort_v = index(index(r, "effort")?.as_deref(), "level")?;
    let fast_v = index(r, "fast_mode")?;
    let fast = matches!(&fast_v, Some(v) if kind(v) == Kind::True);
    let ws = index(r, "workspace")?;
    let pd = or_else(index(ws.as_deref(), "project_dir")?, || {
        or_else(index(ws.as_deref(), "current_dir")?, || index(r, "cwd"))
    })?;
    let s = |v: Option<Box<RawValue>>| -> String {
        if truthy(&v) {
            tostring(&v.unwrap())
        } else {
            String::new()
        }
    };
    let clean = |x: String| -> String {
        x.chars()
            .filter(|c| *c != '\0')
            .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
            .collect()
    };
    Ok(Payload {
        model: clean(s(model)),
        ctx,
        cost: clean(s(cost_v)),
        effort: clean(s(effort_v)),
        fast: if fast { "1" } else { "0" }.into(),
        project_dir: clean(s(pd)),
    })
}

/// bash 版の `parse_payload`。stdin に JSON 値が無ければ(空・空白のみ)全項目が空、
/// 複数あれば先頭を使う(どれかが評価エラーならエラー)。
pub fn parse_payload(input: &str) -> Result<Payload, JqError> {
    let mut first = None;
    for v in serde_json::Deserializer::from_str(input).into_iter::<Box<RawValue>>() {
        let v = v.map_err(|_| JqError)?;
        let p = eval_one(&v)?;
        first.get_or_insert(p);
    }
    Ok(first.unwrap_or_default())
}

// ---- printf '%.2f' -----------------------------------------------------------

/// strtold が受け付ける数値の先頭部分を (符号, 係数の数字列, 10 の指数, 消費バイト数)
/// で返す。inf / nan は係数の代わりに `Special` を返す。
enum Num {
    Finite { neg: bool, coef: String, exp10: i64 },
    Inf(bool),
    Nan(bool),
}

fn strtold_prefix(s: &str) -> (Option<Num>, usize) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let lower: String = s[i..]
        .chars()
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    if lower.starts_with("infinity") {
        return (Some(Num::Inf(neg)), i + 8);
    }
    if lower.starts_with("inf") {
        return (Some(Num::Inf(neg)), i + 3);
    }
    if lower.starts_with("nan") {
        return (Some(Num::Nan(neg)), i + 3);
    }
    let mut coef = String::new();
    let mut frac_len = 0i64;
    let mut any = false;
    while i < b.len() && b[i].is_ascii_digit() {
        coef.push(b[i] as char);
        i += 1;
        any = true;
    }
    if i < b.len() && b[i] == b'.' {
        let save = i;
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            coef.push(b[i] as char);
            frac_len += 1;
            i += 1;
            any = true;
        }
        if !any {
            i = save;
        }
    }
    if !any {
        return (None, 0);
    }
    let mut exp10 = -frac_len;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        let mut eneg = false;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            eneg = b[j] == b'-';
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > start {
            let e: i64 = s[start..j].parse().unwrap_or(i64::MAX / 4);
            exp10 += if eneg { -e } else { e };
            i = j;
        }
    }
    (Some(Num::Finite { neg, coef, exp10 }), i)
}

fn bitlen(x: u128) -> i64 {
    128 - x.leading_zeros() as i64
}

/// `coef × 10^exp10` を long double(仮数 64 bit)に丸めてから `%.2f` で出したときの
/// 「セント単位の整数」。u128 で表せない大きさなら `None`。
fn cents_long_double(coef: &str, exp10: i64) -> Option<u128> {
    let coef = coef.trim_start_matches('0');
    if coef.is_empty() {
        return Some(0);
    }
    if coef.len() > 36 {
        return None;
    }
    let c: u128 = coef.parse().ok()?;
    let k = exp10 + 2; // cents = c × 10^k
    if k >= 0 {
        return c.checked_mul(10u128.checked_pow(k as u32)?);
    }
    let k = (-k) as u32;
    if k > 38 {
        return if (coef.len() as u32) < k {
            Some(0)
        } else {
            None
        };
    }
    let p = 10u128.pow(k);
    let (q, r) = (c / p, c % p);
    let twice = r.checked_mul(2)?;
    if twice < p {
        return Some(q);
    }
    if twice > p {
        return Some(q + 1);
    }
    // ちょうど x.xx5 の境界: long double に丸めた値が境界の上か下かで決まる。
    // 境界値 d = (2q+1)/200 を 64 bit 仮数 M = d × 2^s(2^63 ≤ M < 2^64)に丸める。
    let n = q.checked_mul(2)?.checked_add(1)?;
    let den0: u128 = 200;
    let mut s = 63 - (bitlen(n) - bitlen(den0));
    let ratio = |s: i64| -> Option<(u128, u128, u128)> {
        let (num, den) = if s >= 0 {
            (n.checked_shl(s as u32).filter(|v| v >> s == n)?, den0)
        } else {
            (
                n,
                den0.checked_shl((-s) as u32)
                    .filter(|v| v >> (-s) == den0)?,
            )
        };
        Some((num / den, num % den, den))
    };
    loop {
        let (m, _, _) = ratio(s)?;
        if m >= 1u128 << 64 {
            s -= 1;
        } else if m < 1u128 << 63 {
            s += 1;
        } else {
            break;
        }
    }
    let (m, r, den) = ratio(s)?;
    let up = if r == 0 {
        // long double でも境界ちょうど → printf の round-half-even
        q % 2 == 1
    } else if r * 2 > den {
        true
    } else if r * 2 < den {
        false
    } else {
        m % 2 == 1
    };
    Some(if up { q + 1 } else { q })
}

fn format_2f(n: &Num) -> String {
    match n {
        Num::Inf(neg) => if *neg { "-inf" } else { "inf" }.into(),
        Num::Nan(neg) => if *neg { "-nan" } else { "nan" }.into(),
        Num::Finite { neg, coef, exp10 } => {
            let sign = if *neg { "-" } else { "" };
            match cents_long_double(coef, *exp10) {
                Some(c) => format!("{sign}{}.{:02}", c / 100, c % 100),
                None => {
                    let v: f64 = format!("{coef}e{exp10}").parse().unwrap_or(0.0);
                    format!("{sign}{v:.2}")
                }
            }
        }
    }
}

/// `LC_ALL=C printf '%.2f' "$s" 2>/dev/null || printf '%s' "$s"`(bash builtin)。
/// 数値化に失敗すると、変換できた先頭部分(無ければ 0)を出してから生の値を連結する。
pub fn printf_2f(s: &str) -> String {
    let (num, used) = strtold_prefix(s);
    let n = num.unwrap_or(Num::Finite {
        neg: false,
        coef: "0".into(),
        exp10: 0,
    });
    let mut out = format_2f(&n);
    if used == 0 || used != s.len() {
        out.push_str(s);
    }
    out
}

// ---- python json.dumps ---------------------------------------------------------

/// python の `json.dumps(str)`(ensure_ascii=True)。
pub fn py_json_str(s: &str) -> String {
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
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push('\\');
                    out.push('u');
                    out.push_str(&format!("{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// python の `json.dumps(None | str)`。空文字は `os.environ[...] or None` で None になる。
pub fn py_json_opt(s: &str) -> String {
    if s.is_empty() {
        "null".into()
    } else {
        py_json_str(s)
    }
}

/// statusline が Herdr に送る 4 トークン。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub model: String,
    pub ctx: String,
    pub cost: String,
    pub effort: String,
}

impl Tokens {
    /// `json.dumps(tokens, sort_keys=True)`。
    pub fn fingerprint(&self) -> String {
        format!(
            "{{\"cost\": {}, \"ctx\": {}, \"effort\": {}, \"model\": {}}}",
            py_json_opt(&self.cost),
            py_json_opt(&self.ctx),
            py_json_opt(&self.effort),
            py_json_opt(&self.model)
        )
    }

    /// `json.dumps(request)`(`pane.report_metadata`)。
    pub fn request(&self, id: &str, pane_id: &str, seq: u128) -> String {
        format!(
            "{{\"id\": {}, \"method\": \"pane.report_metadata\", \"params\": {{\"pane_id\": {}, \"source\": \"claude-statusline\", \"seq\": {seq}, \"tokens\": {{\"model\": {}, \"ctx\": {}, \"cost\": {}, \"effort\": {}}}, \"ttl_ms\": {}}}}}",
            py_json_str(id),
            py_json_str(pane_id),
            py_json_opt(&self.model),
            py_json_opt(&self.ctx),
            py_json_opt(&self.cost),
            py_json_opt(&self.effort),
            hook_io::herdr::TTL_MS
        )
    }
}

// ---- dirname / basename --------------------------------------------------------

/// POSIX `dirname`。
pub fn posix_dirname(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.is_empty() { ".".into() } else { "/".into() };
    }
    match t.rfind('/') {
        None => ".".into(),
        Some(i) => {
            let d = t[..i].trim_end_matches('/');
            if d.is_empty() {
                "/".into()
            } else {
                d.into()
            }
        }
    }
}

/// POSIX `basename`(suffix なし)。
pub fn posix_basename(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.is_empty() {
            String::new()
        } else {
            "/".into()
        };
    }
    match t.rfind('/') {
        None => t.into(),
        Some(i) => t[i + 1..].into(),
    }
}

/// `[ "$x" -ge N ]` の左辺として bash が読める整数か(読めなければ比較は偽)。
pub fn shell_int(s: &str) -> Option<i64> {
    let t = s.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
    if t.is_empty() {
        return None;
    }
    let (neg, d) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i64 = d.parse().ok()?;
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(json: &str) -> Payload {
        parse_payload(json).unwrap()
    }

    #[test]
    fn selftest_cases() {
        // bash 版 --selftest の 6 ケース(#305)
        let full = p(
            r#"{"model":{"display_name":"Fable 5"},"context_window":{"used_percentage":42.4},"cost":{"total_cost_usd":1.5},"effort":{"level":"high"},"fast_mode":false,"workspace":{"project_dir":"/tmp/a"}}"#,
        );
        assert_eq!(
            full,
            Payload {
                model: "Fable 5".into(),
                ctx: "42".into(),
                cost: "1.5".into(),
                effort: "high".into(),
                fast: "0".into(),
                project_dir: "/tmp/a".into()
            }
        );
        let x = p(
            r#"{"model":{"display_name":"Fable 5"},"effort":{"level":"high"},"workspace":{"project_dir":"/tmp/a"}}"#,
        );
        assert_eq!((x.ctx.as_str(), x.cost.as_str()), ("", ""));
        assert_eq!(
            (x.effort.as_str(), x.project_dir.as_str()),
            ("high", "/tmp/a")
        );
        let x = p(r#"{"model":{"display_name":"Fable 5"},"cost":{"total_cost_usd":0}}"#);
        assert_eq!(x.cost, "0");
        let x = p(r#"{"context_window":{"used_percentage":7},"fast_mode":true}"#);
        assert_eq!((x.ctx.as_str(), x.fast.as_str()), ("7", "1"));
        let x = p(r#"{"model":{"id":"sonnet"}}"#);
        assert_eq!(x.model, "sonnet");
        assert_eq!(p("{}").fast, "0");
        assert_eq!(p("").fast, "");
        assert_eq!(p("null").fast, "0");
    }

    #[test]
    fn jq_errors() {
        assert!(parse_payload(r#"{"model":"x"}"#).is_err());
        assert!(parse_payload("[]").is_err());
        assert!(parse_payload(r#"{"context_window":{"used_percentage":"5"}}"#).is_err());
        assert!(parse_payload("{").is_err());
        // false は `//` で null 扱い
        assert_eq!(p(r#"{"context_window":{"used_percentage":false}}"#).ctx, "");
    }

    #[test]
    fn number_literals() {
        assert_eq!(jq_number_literal("1.50"), "1.50");
        assert_eq!(jq_number_literal("1e2"), "1E+2");
        assert_eq!(jq_number_literal("0.0000001"), "1E-7");
        assert_eq!(jq_number_literal("1.5e3"), "1.5E+3");
        assert_eq!(jq_number_literal("12e-1"), "1.2");
        assert_eq!(jq_number_literal("0e5"), "0E+5");
        assert_eq!(jq_number_literal("-1E2"), "-1E+2");
        assert_eq!(jq_number_literal("0.1e1"), "1");
        assert_eq!(jq_number_literal("1.0"), "1.0");
        assert_eq!(jq_number_literal("-0.0"), "-0.0");
        assert_eq!(jq_number_literal("1e-6"), "0.000001");
        assert_eq!(
            jq_number_literal("123456789012345678901234567890"),
            "123456789012345678901234567890"
        );
    }

    #[test]
    fn doubles() {
        assert_eq!(jq_double(42.0), "42");
        assert_eq!(jq_double(-0.0), "-0");
        assert_eq!(jq_double(0.0), "0");
        assert_eq!(jq_double(1e17), "1e+17");
        assert_eq!(jq_double(1e16), "1e+16");
        assert_eq!(jq_double(123456789012345678.0), "123456789012345680");
        assert_eq!(jq_double(1e300), "1e+300");
    }

    #[test]
    fn printf() {
        for (i, o) in [
            ("1.5", "1.50"),
            ("0", "0.00"),
            ("0.045", "0.05"),
            ("2.345", "2.35"),
            ("1.005", "1.00"),
            ("0.125", "0.12"),
            ("0.375", "0.38"),
            ("2.5", "2.50"),
            ("1E+2", "100.00"),
            ("-0.0", "-0.00"),
            ("-0.001", "-0.00"),
            ("abc", "0.00abc"),
            ("1.5x", "1.501.5x"),
            ("inf", "inf"),
            (" 3", "3.00"),
            ("true", "0.00true"),
            ("12345678.995", "12345678.99"),
            ("0.005", "0.00"),
            ("0.015", "0.01"),
            ("0.025", "0.03"),
            ("1.115", "1.12"),
            ("99.995", "100.00"),
            ("1000.005", "1000.01"),
            ("0.000005", "0.00"),
        ] {
            assert_eq!(printf_2f(i), o, "{i}");
        }
    }

    #[test]
    fn python_json() {
        assert_eq!(py_json_str("a\"b\\c"), r#""a\"b\\c""#);
        let bs = '\\';
        assert_eq!(py_json_str("é"), format!("\"{bs}u00e9\""));
        assert_eq!(py_json_str("😀"), format!("\"{bs}ud83d{bs}ude00\""));
        assert_eq!(py_json_str("\u{7f}"), format!("\"{bs}u007f\""));
        let t = Tokens {
            model: "M".into(),
            ctx: String::new(),
            cost: "$1.00".into(),
            effort: String::new(),
        };
        assert_eq!(
            t.fingerprint(),
            r#"{"cost": "$1.00", "ctx": null, "effort": null, "model": "M"}"#
        );
    }

    #[test]
    fn dir_base() {
        assert_eq!(posix_dirname("/a/b/.git"), "/a/b");
        assert_eq!(posix_dirname("/a/b/.git/"), "/a/b");
        assert_eq!(posix_dirname("/.git"), "/");
        assert_eq!(posix_dirname("x"), ".");
        assert_eq!(posix_basename("/a/b"), "b");
        assert_eq!(posix_basename("/"), "/");
        assert_eq!(posix_basename("/a/b//"), "b");
    }

    #[test]
    fn shell_ints() {
        assert_eq!(shell_int("80"), Some(80));
        assert_eq!(shell_int("-0"), Some(0));
        assert_eq!(shell_int("1e+17"), None);
        assert_eq!(shell_int(""), None);
    }
}
