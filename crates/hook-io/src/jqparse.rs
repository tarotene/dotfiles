//! クラスタ K の入力側: jq と同じ挙動で JSON を [`J`](挿入順を保つ値型)へ読む
//! 単一のパーサ。
//!
//! 出力側(`jq -c` / `jq .` の整形)は [`crate::jqfmt`] が持つ。もとは
//! `wrapup-stop-gate`(`--add` の `jq -ce .` の再現)と `claude-plan-model`
//! (settings.json を挿入順のまま書き戻す)が、それぞれ同じ構造のパーサを
//! 持っていた。違いは数値の扱いだけなので、[`Numbers`] で切り替える。
//!
//! `serde_json::Value` は(`preserve_order` 無しでは)キーを辞書順に並べ直す。
//! `preserve_order` は workspace 全体に効き、辞書順に頼る他クレートの出力順に
//! 波及するため使わない。
//!
//! 共通の挙動(jq と同じ):
//! - 重複キーは後勝ち(位置は最初の出現のまま)。
//! - サロゲートペアは結合し、対にならない前半は U+FFFD にする。
//! - `truex` のように、リテラルや数値の直後に英数字が続く入力は不正。

use crate::jqfmt::J;
use std::fmt;
use std::fmt::Write as _;

/// 数値リテラルの扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numbers {
    /// 入力の表記をそのまま保つ(`1.50` は `1.50`、`1e2` は `1e2`)。
    /// 設定ファイルを書き戻すとき、利用者が書いた数値の表記を変えないため。
    Verbatim,
    /// decNumber の to-scientific-string に正規化する(jq 1.7+ が数値リテラルを
    /// 出力する形。`1.000` はそのまま、`1e2` は `1E+2`)。`jq -ce .` と同じバイト列を
    /// 出すため。
    Canonical,
}

/// パースエラー。`at` は入力の何バイト目で壊れたか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON at byte {}", self.at)
    }
}

impl std::error::Error for ParseError {}

/// 1 つの JSON 値をパースする。前後の空白は許し、それ以外の余りはエラー。
pub fn parse(text: &str, numbers: Numbers) -> Result<J, ParseError> {
    let mut p = Parser::new(text, numbers);
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.err());
    }
    Ok(v)
}

/// 空白区切りの JSON 値の列をパースする(jq の入力と同じく `1 2` は 2 値)。
/// 数値は [`Numbers::Canonical`]。1 つでも壊れていれば `Err`。
pub fn parse_stream(text: &str) -> Result<Vec<J>, ParseError> {
    let mut p = Parser::new(text, Numbers::Canonical);
    let mut out = Vec::new();
    loop {
        p.ws();
        if p.i >= p.b.len() {
            return Ok(out);
        }
        out.push(p.value(0)?);
    }
}

/// 入れ子の深さの上限(再帰でスタックを使い切らないため)。
const MAX_DEPTH: usize = 10_000;

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    numbers: Numbers,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, numbers: Numbers) -> Self {
        Parser {
            b: text.as_bytes(),
            i: 0,
            numbers,
        }
    }

    fn err(&self) -> ParseError {
        ParseError { at: self.i }
    }

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
            Err(self.err())
        }
    }

    fn lit(&mut self, word: &str, v: J) -> Result<J, ParseError> {
        if !self.b[self.i..].starts_with(word.as_bytes()) {
            return Err(self.err());
        }
        self.i += word.len();
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(self.err());
        }
        Ok(v)
    }

    fn value(&mut self, depth: usize) -> Result<J, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err());
        }
        self.ws();
        match self.peek().ok_or_else(|| self.err())? {
            b'n' => self.lit("null", J::Null),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'"' => Ok(J::Str(self.string()?)),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.err()),
        }
    }

    fn array(&mut self, depth: usize) -> Result<J, ParseError> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(J::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(J::Arr(items));
                }
                _ => return Err(self.err()),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<J, ParseError> {
        self.i += 1;
        let mut pairs: Vec<(String, J)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(J::Obj(pairs));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err());
            }
            let k = self.string()?;
            self.ws();
            self.eat(b':')?;
            let v = self.value(depth + 1)?;
            match pairs.iter_mut().find(|(ek, _)| *ek == k) {
                Some(slot) => slot.1 = v,
                None => pairs.push((k, v)),
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(J::Obj(pairs));
                }
                _ => return Err(self.err()),
            }
        }
    }

    fn digits(&mut self) -> &'a str {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[st..self.i]).unwrap_or("")
    }

    fn number(&mut self) -> Result<J, ParseError> {
        let start = self.i;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.i += 1;
        }
        let int = self.digits();
        if int.is_empty() {
            return Err(self.err());
        }
        let mut frac = "";
        if self.peek() == Some(b'.') {
            self.i += 1;
            frac = self.digits();
            if frac.is_empty() {
                return Err(self.err());
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
                return Err(self.err());
            }
            let v: i64 = d.parse().map_err(|_| self.err())?;
            exp = if eneg { -v } else { v };
        }
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_')
        {
            return Err(self.err());
        }
        Ok(J::Num(match self.numbers {
            Numbers::Verbatim => String::from_utf8_lossy(&self.b[start..self.i]).into_owned(),
            Numbers::Canonical => dec_to_sci(neg, int, frac, exp),
        }))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let h = self.b.get(self.i..self.i + 4).ok_or_else(|| self.err())?;
        let s = std::str::from_utf8(h).map_err(|_| self.err())?;
        let v = u32::from_str_radix(s, 16).map_err(|_| self.err())?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err())?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err())?;
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
                        b'u' => self.unicode_escape()?,
                        _ => return Err(self.err()),
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

    /// `\u` の後ろ。サロゲートペアを結合し、後半が無い前半は U+FFFD(jq と同じ)。
    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let hi = self.hex4()?;
        if !(0xD800..0xDC00).contains(&hi) {
            return Ok(char::from_u32(hi).unwrap_or('\u{FFFD}'));
        }
        if !self.b[self.i..].starts_with(b"\\u") {
            return Ok('\u{FFFD}');
        }
        let save = self.i;
        self.i += 2;
        let lo = self.hex4()?;
        if (0xDC00..0xE000).contains(&lo) {
            Ok(
                char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                    .unwrap_or('\u{FFFD}'),
            )
        } else {
            self.i = save;
            Ok('\u{FFFD}')
        }
    }
}

/// decNumber の to-scientific-string(jq 1.7+ が数値リテラルを出力する形)。
fn dec_to_sci(neg: bool, int: &str, frac: &str, exp: i64) -> String {
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
    fn verbatim_keeps_the_written_form() {
        let v = parse(r#"{"n": 1.50, "e": 1e2}"#, Numbers::Verbatim).unwrap();
        assert_eq!(v.compact(), r#"{"n":1.50,"e":1e2}"#);
    }

    #[test]
    fn strings_and_order() {
        assert_eq!(c(r#""\u00e9\/""#), "\"é/\"");
        assert_eq!(c(r#""\u007f\u001f""#), r#""\u007f\u001f""#);
        assert_eq!(c(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#);
        assert_eq!(c(r#""\ud83d\ude00""#), "\"😀\"");
        assert_eq!(c(r#""\ud83dx""#), "\"\u{FFFD}x\"");
        assert_eq!(c("[] {}"), "[]\n{}");
    }

    #[test]
    fn keeps_insertion_order_with_duplicate_keys() {
        let v = parse(
            r#"{"b":1.50,"a":[true,null,"xé😀"],"b":2}"#,
            Numbers::Verbatim,
        )
        .unwrap();
        assert_eq!(v.compact(), r#"{"b":2,"a":[true,null,"xé😀"]}"#);
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
    fn single_value_rejects_leftovers() {
        assert!(parse("{", Numbers::Verbatim).is_err());
        assert!(parse("{} x", Numbers::Verbatim).is_err());
        assert!(parse("", Numbers::Verbatim).is_err());
        assert_eq!(
            parse("{} x", Numbers::Verbatim).unwrap_err(),
            ParseError { at: 3 }
        );
    }

    /// 上限を超える入れ子はエラーになる。hook は主スレッド(8MB)で動き、上限ちょうどの
    /// 深さも通る(2026-10-23 に debug/release で確認)。テストスレッドの既定(2MB)では
    /// 足りないので、主スレッドと同じ大きさのスタックで走らせる。
    #[test]
    fn depth_is_bounded() {
        std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(|| {
                let at_limit = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
                assert!(parse_stream(&at_limit).is_ok());
                let over = "[".repeat(MAX_DEPTH + 2);
                assert!(parse_stream(&over).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
