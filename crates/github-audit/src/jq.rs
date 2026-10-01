//! bash 版が jq / coreutils に委ねていた細部の再現(#414)。
//!
//! 出力をバイト単位で bash 版に一致させるため、次の 3 点をここに寄せる:
//!
//! - jq の値の全順序(`sort_by` / `unique` が使う。null < false < true <
//!   数値 < 文字列 < 配列 < オブジェクト、文字列はバイト順)
//! - `$(...)` が末尾改行を全部落とすこと([`sh_trim`])
//! - UTF-8 ロケールでの `[[:space:]]`(glibc の `iswspace`)。bash のパターン・
//!   `=~`・grep -E・gawk は UTF-8 ロケールで U+3000 なども空白に数える。
//!   `tr -s '[:space:]'` だけはバイト単位で ASCII のみ([`is_ascii_space`])。
//! - キー順を保つ JSON 読み込み([`parse_ordered`])。workflows ドメインの
//!   `quality-json-not-canonical` は `jq -c` の出力文字列を比べるため、
//!   キー順が違えば別物と判定する(bash の挙動どおり)。

use hook_io::jqfmt::J;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::cmp::Ordering;
use std::fmt;

/// `$(...)` と同じく末尾の改行をすべて落とす。
pub fn sh_trim(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// glibc の `iswspace`(UTF-8 ロケールの `[[:space:]]`)。
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' '
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// C ロケール(バイト単位)の `[:space:]`。`tr` はマルチバイトを扱わない。
pub fn is_ascii_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ')
}

/// bash の `${s#"${s%%[![:space:]]*}"}` / `${s%"${s##*[![:space:]]}"}`(両端の空白除去)。
pub fn trim_space(s: &str) -> &str {
    s.trim_matches(is_space)
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

/// jq の値比較(`sort` / `sort_by` / `unique` の順序)。
pub fn cmp(a: &Value, b: &Value) -> Ordering {
    let (ra, rb) = (type_rank(a), type_rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        (Value::String(x), Value::String(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Array(x), Value::Array(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                let o = cmp(p, q);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Object(x), Value::Object(y)) => {
            // jq はまずキー集合(ソート済み配列)を比べ、等しければ値を
            // キー順に比べる。
            let mut kx: Vec<&String> = x.keys().collect();
            let mut ky: Vec<&String> = y.keys().collect();
            kx.sort_by(|p, q| p.as_bytes().cmp(q.as_bytes()));
            ky.sort_by(|p, q| p.as_bytes().cmp(q.as_bytes()));
            let o = kx
                .iter()
                .map(|k| k.as_bytes())
                .cmp(ky.iter().map(|k| k.as_bytes()));
            if o != Ordering::Equal {
                return o;
            }
            for k in kx {
                let o = cmp(&x[k.as_str()], &y[k.as_str()]);
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        }
        _ => Ordering::Equal,
    }
}

/// 文字列集合の `unique`(バイト順に並べて重複を落とす)。
pub fn unique_strings(mut v: Vec<String>) -> Vec<String> {
    v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    v.dedup();
    v
}

/// jq の `.[]?` に近い反復: 配列は要素、オブジェクトは値、それ以外は空。
pub fn iter_values(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(m) => m.values().collect(),
        _ => Vec::new(),
    }
}

/// `jq -r` で 1 値を出したときの文字列(文字列はそのまま、他は JSON 表記)。
pub fn raw(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// jq の真偽(null と false だけが偽)。
pub fn truthy(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null) | Some(Value::Bool(false)))
}

/// 配列なら文字列要素を取り出す(それ以外は空)。
pub fn string_array(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// キー順を保ったまま JSON を読む(`jq -c .` の出力を再現する用途)。
pub fn parse_ordered(text: &str) -> Option<J> {
    let mut de = serde_json::Deserializer::from_str(text);
    let v = Ordered::deserialize(&mut de).ok()?;
    de.end().ok()?;
    Some(v.0)
}

struct Ordered(J);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(OrderedVisitor)
    }
}

struct OrderedVisitor;

impl<'de> Visitor<'de> for OrderedVisitor {
    type Value = Ordered;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_unit<E>(self) -> Result<Ordered, E> {
        Ok(Ordered(J::Null))
    }
    fn visit_none<E>(self) -> Result<Ordered, E> {
        Ok(Ordered(J::Null))
    }
    fn visit_bool<E>(self, b: bool) -> Result<Ordered, E> {
        Ok(Ordered(J::Bool(b)))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Ordered, E> {
        Ok(Ordered(J::Num(n.to_string())))
    }
    fn visit_u64<E>(self, n: u64) -> Result<Ordered, E> {
        Ok(Ordered(J::Num(n.to_string())))
    }
    fn visit_f64<E>(self, n: f64) -> Result<Ordered, E> {
        Ok(Ordered(J::Num(jq_number(n))))
    }
    fn visit_str<E>(self, s: &str) -> Result<Ordered, E> {
        Ok(Ordered(J::Str(s.to_string())))
    }
    fn visit_string<E>(self, s: String) -> Result<Ordered, E> {
        Ok(Ordered(J::Str(s)))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered, A::Error> {
        let mut out = Vec::new();
        while let Some(Ordered(v)) = seq.next_element()? {
            out.push(v);
        }
        Ok(Ordered(J::Arr(out)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
        let mut out: Vec<(String, J)> = Vec::new();
        while let Some((k, Ordered(v))) = map.next_entry::<String, Ordered>()? {
            // jq も重複キーは後勝ちで、位置は最初の出現のまま。
            if let Some(slot) = out.iter_mut().find(|(ek, _)| *ek == k) {
                slot.1 = v;
            } else {
                out.push((k, v));
            }
        }
        Ok(Ordered(J::Obj(out)))
    }
}

fn jq_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e17 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `J` のオブジェクトからキーを引く(`.key`、無ければ null 相当の `None`)。
pub fn j_get<'a>(v: &'a J, key: &str) -> Option<&'a J> {
    match v {
        J::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn order_matches_jq() {
        let mut v = vec![json!("b"), json!(null), json!(1), json!("a"), json!(false)];
        v.sort_by(cmp);
        assert_eq!(
            v,
            vec![json!(null), json!(false), json!(1), json!("a"), json!("b")]
        );
    }

    #[test]
    fn ordered_parse_keeps_key_order() {
        let j = parse_ordered(r#"{"b":1,"a":[true,null]}"#).unwrap();
        assert_eq!(j.compact(), r#"{"b":1,"a":[true,null]}"#);
        assert!(parse_ordered("{not json").is_none());
        assert!(parse_ordered("{} {}").is_none());
    }

    #[test]
    fn space_class() {
        assert!(is_space('\u{3000}'));
        assert!(!is_space('\u{a0}'));
        assert!(!is_ascii_space('\u{3000}'));
    }
}
