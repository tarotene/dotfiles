//! bash 版が jq に委ねていた値の扱いの最小限の再現。
//!
//! jq のエラー(`.foo` を数値に適用した等)は `Err(JqError)` で表し、呼び出し側は
//! bash 版の `|| fallback` と同じ縮退値に倒す。

use serde_json::Value;

/// jq がエラーで止まる形に当たったこと(中身は持たない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JqError;

/// `jq -r` で値を出したときの文字列(文字列は生、null は "null"、他は compact)。
/// jq の文字列補間 `"\(x)"` も同じ規則。
pub fn raw(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `.key`。オブジェクトはフィールド(無ければ null)、null は null、他はエラー。
pub fn field(v: &Value, key: &str) -> Result<Value, JqError> {
    match v {
        Value::Object(m) => Ok(m.get(key).cloned().unwrap_or(Value::Null)),
        Value::Null => Ok(Value::Null),
        _ => Err(JqError),
    }
}

/// `.[]`。配列は要素、オブジェクトは値、他はエラー。
pub fn iter(v: &Value) -> Result<Vec<&Value>, JqError> {
    match v {
        Value::Array(a) => Ok(a.iter().collect()),
        Value::Object(m) => Ok(m.values().collect()),
        _ => Err(JqError),
    }
}

/// `length`(配列・オブジェクト・文字列・null のみ。数値などはエラー扱い)。
pub fn length(v: &Value) -> Result<usize, JqError> {
    match v {
        Value::Array(a) => Ok(a.len()),
        Value::Object(m) => Ok(m.len()),
        Value::String(s) => Ok(s.chars().count()),
        Value::Null => Ok(0),
        _ => Err(JqError),
    }
}

/// jq の等値(数値は値で比べる)。
pub fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// jq の配列差 `$a - $b`(`$b` に等しい要素をすべて落とす)。
pub fn subtract(a: &[Value], b: &[Value]) -> Vec<Value> {
    a.iter()
        .filter(|x| !b.iter().any(|y| eq(x, y)))
        .cloned()
        .collect()
}

/// jq の `join(", ")`(null は空文字、数値・真偽値は文字列化)。
pub fn join(items: &[Value], sep: &str) -> String {
    items
        .iter()
        .map(|v| match v {
            Value::Null => String::new(),
            other => raw(other),
        })
        .collect::<Vec<_>>()
        .join(sep)
}

/// `// default` 用: null と false を「無い」とみなす。
pub fn truthy(v: &Value) -> Option<&Value> {
    match v {
        Value::Null | Value::Bool(false) => None,
        other => Some(other),
    }
}

/// `.[0].key // empty` を `jq -r` で取った文字列(無ければ `None`)。
pub fn first_field(list: &Value, key: &str) -> Option<String> {
    let first = list.as_array().and_then(|a| a.first())?;
    truthy(first.get(key)?).map(raw)
}

/// jq のソート順での順位(null < false < true < 数値 < 文字列 < 配列 < オブジェクト)。
fn rank(v: &Value) -> u8 {
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

/// `group_by(.)` 用の比較(文字列はコードポイント順、数値は値順。配列・
/// オブジェクトは compact 表記で近似する — bucket に現れることは無い)。
pub fn cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        _ => a.to_string().cmp(&b.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_and_join() {
        assert_eq!(raw(&json!("a")), "a");
        assert_eq!(raw(&Value::Null), "null");
        assert_eq!(raw(&json!(3)), "3");
        assert_eq!(join(&[json!("a"), Value::Null, json!(1)], ", "), "a, , 1");
    }

    #[test]
    fn subtract_and_first_field() {
        assert_eq!(
            subtract(&[json!("a"), json!("b"), json!("a")], &[json!("a")]),
            vec![json!("b")]
        );
        let l = json!([{"number": 37, "isDraft": false}]);
        assert_eq!(first_field(&l, "number").as_deref(), Some("37"));
        assert_eq!(first_field(&l, "isDraft"), None);
        assert_eq!(first_field(&json!([]), "number"), None);
    }
}
