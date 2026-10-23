//! hook 入力の `jq -r '.key // default'` の再現。
//!
//! パーサと値型は `hook_io::jqparse` / `hook_io::jqfmt::J` が持つ(`--add` の
//! `jq -ce .` と同じバイト列の出力もそちら)。ここに残るのは、jq の `.key`・`-r`・
//! `//` の意味だけ。

use hook_io::jqfmt::J;
use hook_io::jqparse::ParseError;

/// `jq -r` の 1 値ぶん(文字列は生、それ以外は compact)。
fn raw(v: &J) -> String {
    match v {
        J::Str(s) => s.clone(),
        other => other.compact(),
    }
}

/// jq の `.key`。object なら値(無ければ null)、null なら null、
/// それ以外は jq のエラー(`Cannot index ...`)で、`None`。
pub fn index<'a>(v: &'a J, key: &str) -> Option<&'a J> {
    static NULL: J = J::Null;
    match v {
        J::Obj(m) => Some(m.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, v)| v)),
        J::Null => Some(&NULL),
        _ => None,
    }
}

/// jq の真偽(`null` と `false` だけが偽)。
pub fn truthy(v: &J) -> bool {
    !matches!(v, J::Null | J::Bool(false))
}

/// `jq -r '.key // <alt>'` を入力の各値に適用し、出力行を `\n` で連結する
/// (bash の `$(...)` と同じく末尾改行は付けない)。`alt` が `None` なら jq の
/// `empty`。どれかの値で index が失敗すれば `Err`(jq は exit 5)。
pub fn jq_r_alt(values: &[J], key: &str, alt: Option<&str>) -> Result<String, ParseError> {
    let mut outs = Vec::new();
    for v in values {
        let x = index(v, key).ok_or(ParseError { at: 0 })?;
        if truthy(x) {
            outs.push(raw(x));
        } else if let Some(a) = alt {
            outs.push(a.to_string());
        }
    }
    Ok(outs.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_io::jqparse::parse_stream;

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
