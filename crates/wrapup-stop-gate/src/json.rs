//! bash 版が `jq` に任せていた JSON 処理の、出力バイトまで合わせた再現。
//!
//! - `--add` の `jq -ce .`: キーの出現順を保ち(重複キーは後勝ちで最初の位置)、
//!   数値はリテラルを decNumber の to-scientific-string で正規化し(jq 1.7+ の
//!   挙動: `1.000` はそのまま、`1e2` は `1E+2`)、文字列は jq と同じ規則で
//!   再エスケープする。serde_json の `Map` はキーを並べ替えるので使わない
//!   (`preserve_order` は workspace 全体に効くため避ける)。
//! - hook 入力の `jq -r '.key // default'`。
//!
//! 入力は jq と同じく空白区切りの値の列として読む(`1 2` は 2 値)。jq の
//! 寛容さのうち、数値の先頭ゼロ(`00`)だけは受け付ける。

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    /// 正規化済みの数値リテラル。
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

/// パースエラー(位置などの詳細は持たない — 呼び出し側は jq の失敗と同じく
/// 「不正」としか扱わない)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError;

/// 空白区切りの JSON 値の列をパースする。1 つでも壊れていれば `Err`。
pub fn parse_stream(s: &str) -> Result<Vec<J>, ParseError> {
    let mut p = Parser {
        b: s.as_bytes(),
        i: 0,
    };
    let mut out = Vec::new();
    loop {
        p.ws();
        if p.i >= p.b.len() {
            return Ok(out);
        }
        out.push(p.value(0)?);
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

const MAX_DEPTH: usize = 10_000;

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(ParseError)
        }
    }

    fn lit(&mut self, word: &str, v: J) -> Result<J, ParseError> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            // `truex` のような続きは不正(jq も 1 トークンとして読む)
            if self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err(ParseError);
            }
            Ok(v)
        } else {
            Err(ParseError)
        }
    }

    fn value(&mut self, depth: usize) -> Result<J, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError);
        }
        self.ws();
        match self.peek().ok_or(ParseError)? {
            b'n' => self.lit("null", J::Null),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'"' => Ok(J::Str(self.string()?)),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(J::Arr(v));
                }
                loop {
                    v.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(v));
                        }
                        _ => return Err(ParseError),
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut m: Vec<(String, J)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(J::Obj(m));
                }
                loop {
                    self.ws();
                    if self.peek() != Some(b'"') {
                        return Err(ParseError);
                    }
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    match m.iter_mut().find(|(kk, _)| *kk == k) {
                        Some(slot) => slot.1 = v,
                        None => m.push((k, v)),
                    }
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(m));
                        }
                        _ => return Err(ParseError),
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(ParseError),
        }
    }

    fn digits(&mut self) -> &str {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[st..self.i]).unwrap_or("")
    }

    fn number(&mut self) -> Result<J, ParseError> {
        let neg = self.peek() == Some(b'-');
        if neg {
            self.i += 1;
        }
        let int = self.digits().to_string();
        if int.is_empty() {
            return Err(ParseError);
        }
        let mut frac = String::new();
        if self.peek() == Some(b'.') {
            self.i += 1;
            frac = self.digits().to_string();
            if frac.is_empty() {
                return Err(ParseError);
            }
        }
        let mut exp: i64 = 0;
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            let eneg = match self.peek() {
                Some(b'-') => {
                    self.i += 1;
                    true
                }
                Some(b'+') => {
                    self.i += 1;
                    false
                }
                _ => false,
            };
            let d = self.digits();
            if d.is_empty() {
                return Err(ParseError);
            }
            let v: i64 = d.parse().map_err(|_| ParseError)?;
            exp = if eneg { -v } else { v };
        }
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_')
        {
            return Err(ParseError);
        }
        Ok(J::Num(dec_to_sci(neg, &int, &frac, exp)))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let h = self.b.get(self.i..self.i + 4).ok_or(ParseError)?;
        let s = std::str::from_utf8(h).map_err(|_| ParseError)?;
        let v = u32::from_str_radix(s, 16).map_err(|_| ParseError)?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek().ok_or(ParseError)?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek().ok_or(ParseError)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) {
                                // サロゲートペア。後半が無ければ jq と同じく U+FFFD
                                if self.b[self.i..].starts_with(b"\\u") {
                                    let save = self.i;
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        char::from_u32(
                                            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00),
                                        )
                                        .unwrap_or('\u{FFFD}')
                                    } else {
                                        self.i = save;
                                        '\u{FFFD}'
                                    }
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return Err(ParseError),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
        // 入力は &str 由来なので UTF-8。エスケープ由来も char 経由。
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

/// decNumber の to-scientific-string(jq 1.7+ が数値リテラルを出力する形)。
pub fn dec_to_sci(neg: bool, int: &str, frac: &str, exp: i64) -> String {
    let mut coef: String = format!("{int}{frac}");
    let stripped = coef.trim_start_matches('0');
    coef = if stripped.is_empty() {
        "0".to_string()
    } else {
        stripped.to_string()
    };
    let e = exp - frac.len() as i64;
    let n = coef.len() as i64;
    let adj = e + n - 1;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    if e <= 0 && adj >= -6 {
        if e == 0 {
            s.push_str(&coef);
        } else if n > -e {
            let k = (n + e) as usize;
            s.push_str(&coef[..k]);
            s.push('.');
            s.push_str(&coef[k..]);
        } else {
            s.push_str("0.");
            for _ in 0..(-e - n) {
                s.push('0');
            }
            s.push_str(&coef);
        }
    } else {
        s.push_str(&coef[..1]);
        if n > 1 {
            s.push('.');
            s.push_str(&coef[1..]);
        }
        s.push('E');
        s.push(if adj >= 0 { '+' } else { '-' });
        let _ = write!(s, "{}", adj.abs());
    }
    s
}

/// jq の文字列エスケープ(`"` `\\` と制御文字・DEL。非 ASCII はそのまま)。
pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl J {
    /// `jq -c` の 1 値ぶん。
    pub fn compact(&self) -> String {
        let mut s = String::new();
        self.write_compact(&mut s);
        s
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => out.push_str(n),
            J::Str(s) => write_str(out, s),
            J::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write_compact(out);
                }
                out.push(']');
            }
            J::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
    }

    /// `jq -r` の 1 値ぶん(文字列は生、それ以外は compact)。
    pub fn raw(&self) -> String {
        match self {
            J::Str(s) => s.clone(),
            other => other.compact(),
        }
    }

    /// jq の `.key`。object なら値(無ければ null)、null なら null、
    /// それ以外は jq のエラー(`Cannot index ...`)。
    pub fn index(&self, key: &str) -> Result<&J, ParseError> {
        static NULL: J = J::Null;
        match self {
            J::Obj(m) => Ok(m.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, v)| v)),
            J::Null => Ok(&NULL),
            _ => Err(ParseError),
        }
    }

    /// jq の真偽(`null` と `false` だけが偽)。
    pub fn truthy(&self) -> bool {
        !matches!(self, J::Null | J::Bool(false))
    }
}

/// `jq -r '.key // <alt>'` を入力の各値に適用し、出力行を `\n` で連結する
/// (bash の `$(...)` と同じく末尾改行は付けない)。`alt` が `None` なら jq の
/// `empty`。どれかの値で index が失敗すれば `Err`(jq は exit 5)。
pub fn jq_r_alt(values: &[J], key: &str, alt: Option<&str>) -> Result<String, ParseError> {
    let mut outs = Vec::new();
    for v in values {
        let x = v.index(key)?;
        if x.truthy() {
            outs.push(x.raw());
        } else if let Some(a) = alt {
            outs.push(a.to_string());
        }
    }
    Ok(outs.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> String {
        parse_stream(s)
            .unwrap()
            .iter()
            .map(J::compact)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 期待値は `jq -c .`(jq 1.8.2)の実出力。
    #[test]
    fn numbers_match_jq() {
        let cases = [
            ("0.000", "0.000"),
            ("0.0000001", "1E-7"),
            ("0.000001", "0.000001"),
            ("1e-7", "1E-7"),
            ("123.456e5", "1.23456E+7"),
            ("123.456e-2", "1.23456"),
            ("-1.5e-10", "-1.5E-10"),
            ("1e1000", "1E+1000"),
            ("00", "0"),
            ("0e5", "0E+5"),
            ("12e0", "12"),
            ("1.20E+1", "12.0"),
            ("-0.0", "-0.0"),
            ("5e-1", "0.5"),
            ("99999999999999999999.5", "99999999999999999999.5"),
            ("1.000", "1.000"),
            ("1e2", "1E+2"),
            ("100000000000000000001", "100000000000000000001"),
            ("0.1e1", "1"),
            ("-0", "-0"),
            ("1.5e300", "1.5E+300"),
        ];
        for (i, want) in cases {
            assert_eq!(c(i), want, "{i}");
        }
    }

    #[test]
    fn strings_and_order() {
        assert_eq!(c(r#""\u00e9\/""#), "\"é/\"");
        assert_eq!(c(r#""\u007f\u001f""#), r#""\u007f\u001f""#);
        assert_eq!(c(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#);
        assert_eq!(c(r#""\ud83d\ude00""#), "\"😀\"");
        assert_eq!(c("[] {}"), "[]\n{}");
    }

    #[test]
    fn invalid() {
        for s in [
            "not-json", "{", "[1,]", "{\"a\"}", "01.", "1e", "truex", "\"\\x\"",
        ] {
            assert!(parse_stream(s).is_err(), "{s}");
        }
        assert_eq!(parse_stream("  ").unwrap(), vec![]);
    }

    #[test]
    fn jq_r() {
        let v = parse_stream(r#"{"a":"x","b":null,"c":5}"#).unwrap();
        assert_eq!(jq_r_alt(&v, "a", None).unwrap(), "x");
        assert_eq!(jq_r_alt(&v, "b", None).unwrap(), "");
        assert_eq!(jq_r_alt(&v, "b", Some("unknown")).unwrap(), "unknown");
        assert_eq!(jq_r_alt(&v, "c", None).unwrap(), "5");
        let a = parse_stream("[]").unwrap();
        assert!(jq_r_alt(&a, "a", None).is_err());
        let n = parse_stream("null").unwrap();
        assert_eq!(jq_r_alt(&n, "a", Some("d")).unwrap(), "d");
    }
}
