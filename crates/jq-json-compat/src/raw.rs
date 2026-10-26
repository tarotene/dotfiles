use serde_json::Value;

pub fn jq_raw(v: &Value) -> Option<String> {
    match v {
        Value::Null | Value::Bool(false) => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("true".into()),
        Value::Number(n) => Some(n.to_string()),
        other => serde_json::to_string_pretty(other).ok(),
    }
}
