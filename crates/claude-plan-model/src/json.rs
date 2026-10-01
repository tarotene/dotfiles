//! settings.json を「jq が保つ挿入順のまま」読み書きするための最小 JSON パーサ。
//!
//! bash 版は `jq '.env.X = ... | del(...)'` で書き換えており、jq は入力のキー順を
//! 保つ。`serde_json::Value` は(`preserve_order` 無しでは)キーを辞書順に並べ直して
//! しまい、利用者の `~/.claude/settings.json` 全体の並びを毎回書き換える。
//! `preserve_order` を workspace に入れると他クレートの出力順に波及するため、
//! ここでは `hook_io::jqfmt::J`(挿入順を保つ値型)へ読む小さなパーサを持つ。
//! 数値は表記のまま保つ(jq 1.7 の入力表記保持と同じ)。

use hook_io::jqfmt::J;

pub fn parse(text: &str) -> Result<J, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("trailing characters at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn lit(&mut self, word: &str, v: J) -> Result<J, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("invalid literal at byte {}", self.i))
        }
    }

    fn value(&mut self) -> Result<J, String> {
        match self.peek() {
            None => Err("unexpected end of input".into()),
            Some(b'n') => self.lit("null", J::Null),
            Some(b't') => self.lit("true", J::Bool(true)),
            Some(b'f') => self.lit("false", J::Bool(false)),
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(J::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value()?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(items));
                        }
                        _ => return Err(format!("expected , or ] at byte {}", self.i)),
                    }
                }
            }
            Some(b'{') => {
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
                        return Err(format!("expected object key at byte {}", self.i));
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':') {
                        return Err(format!("expected : at byte {}", self.i));
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value()?;
                    // 重複キーは jq と同じく最後の値が勝つ(位置は最初のまま)。
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
                        _ => return Err(format!("expected , or }} at byte {}", self.i)),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let n = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                if n.parse::<f64>().is_err() {
                    return Err(format!("invalid number at byte {start}"));
                }
                Ok(J::Num(n.to_string()))
            }
            Some(_) => Err(format!("unexpected character at byte {}", self.i)),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or_else(|| format!("invalid \\u escape at byte {}", self.i))?;
        self.i += 4;
        Ok(h)
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| "unterminated string".to_string())?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self
                        .peek()
                        .ok_or_else(|| "unterminated string".to_string())?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{08}',
                        b'f' => '\u{0c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo & 0x3ff))
                                    .unwrap_or('\u{fffd}')
                            } else {
                                char::from_u32(hi).unwrap_or('\u{fffd}')
                            }
                        }
                        _ => return Err(format!("invalid escape at byte {}", self.i)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

/// オブジェクトのキーを引く(無ければ None)。
pub fn get<'a>(v: &'a J, key: &str) -> Option<&'a J> {
    match v {
        J::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// オブジェクトのキーを設定する(既存なら位置を保って置換、無ければ末尾に追加)。
pub fn set(pairs: &mut Vec<(String, J)>, key: &str, val: J) {
    match pairs.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = val,
        None => pairs.push((key.to_string(), val)),
    }
}

/// オブジェクトのキーを削除する。
pub fn del(pairs: &mut Vec<(String, J)>, key: &str) {
    pairs.retain(|(k, _)| k != key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_insertion_order_and_numbers() {
        let v = parse(r#"{"b":1.50,"a":[true,null,"xé😀"],"b":2}"#).unwrap();
        assert_eq!(v.compact(), r#"{"b":2,"a":[true,null,"xé😀"]}"#);
        let v = parse(r#"{"n": 1.50}"#).unwrap();
        assert_eq!(v.compact(), r#"{"n":1.50}"#);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{").is_err());
        assert!(parse("{} x").is_err());
        assert!(parse("").is_err());
    }
}
