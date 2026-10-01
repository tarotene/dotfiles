//! クラスタ K: `jq -n --arg m "$msg" '{...}'` と同じバイト列を出す JSON 出力
//! (ADR-0024 Stage 4b、#412)。
//!
//! 吸収元: `plan-scope-gate.sh` / `plan-precedent-gate.sh` / `plan-fresh-gate.sh` /
//! `copilot-plan-review.sh` の `pass_through()` / `deny_with()`。
//!
//! `serde_json::Value` は(`preserve_order` 無しでは)キーを辞書順に並べるため、
//! jq が保つ挿入順(`hookEventName` → `decision` 等)を再現できない。ここでは
//! 挿入順を保つ小さな値型 [`J`] と、jq 1.7 系と同じ整形(2 空白インデント、
//! `"k": v`、非 ASCII は生のまま、制御文字と DEL は `\uXXXX`)を持つ。

use serde_json::Value;

/// 挿入順を保つ JSON 値。
#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    /// 数値は表記のまま出す(jq の入力表記保持と同じ)。
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn obj<K: Into<String>>(pairs: Vec<(K, J)>) -> J {
        J::Obj(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// `jq .` と同じ整形(末尾改行なし)。
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    /// `jq -c .` と同じ 1 行表記(末尾改行なし)。
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }

    fn write_pretty(&self, out: &mut String, depth: usize) {
        match self {
            J::Arr(items) if !items.is_empty() => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('\n');
                    indent(out, depth + 1);
                    v.write_pretty(out, depth + 1);
                }
                out.push('\n');
                indent(out, depth);
                out.push(']');
            }
            J::Obj(pairs) if !pairs.is_empty() => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('\n');
                    indent(out, depth + 1);
                    out.push_str(&escape(k));
                    out.push_str(": ");
                    v.write_pretty(out, depth + 1);
                }
                out.push('\n');
                indent(out, depth);
                out.push('}');
            }
            other => other.write_compact(out),
        }
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => out.push_str(n),
            J::Str(s) => out.push_str(&escape(s)),
            J::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_compact(out);
                }
                out.push(']');
            }
            J::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&escape(k));
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
    }
}

impl From<&Value> for J {
    /// `serde_json::Value` からの変換。オブジェクトのキー順は Value 側の順
    /// (既定では辞書順)になる点に注意。
    fn from(v: &Value) -> J {
        match v {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Number(n) => J::Num(n.to_string()),
            Value::String(s) => J::Str(s.clone()),
            Value::Array(a) => J::Arr(a.iter().map(J::from).collect()),
            Value::Object(m) => J::Obj(m.iter().map(|(k, v)| (k.clone(), J::from(v))).collect()),
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// jq と同じ文字列エスケープ(両端の `"` 込み)。
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `jq -n --arg m "$msg" '{systemMessage: $m}'`(末尾改行込み)。
pub fn system_message(msg: &str) -> String {
    format!(
        "{}\n",
        J::obj(vec![("systemMessage", J::str(msg))]).pretty()
    )
}

/// plan gate 系の `deny_with()`: `PermissionRequest` なら `decision.behavior`、
/// それ以外は `permissionDecision` の形(末尾改行込み)。
pub fn deny_for_event(event: &str, reason: &str) -> String {
    let inner = if event == "PermissionRequest" {
        J::obj(vec![
            ("hookEventName", J::str("PermissionRequest")),
            (
                "decision",
                J::obj(vec![
                    ("behavior", J::str("deny")),
                    ("message", J::str(reason)),
                ]),
            ),
        ])
    } else {
        J::obj(vec![
            ("hookEventName", J::str("PreToolUse")),
            ("permissionDecision", J::str("deny")),
            ("permissionDecisionReason", J::str(reason)),
        ])
    };
    format!("{}\n", J::obj(vec![("hookSpecificOutput", inner)]).pretty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_matches_jq() {
        assert_eq!(
            system_message("a\nb"),
            "{\n  \"systemMessage\": \"a\\nb\"\n}\n"
        );
        assert_eq!(
            deny_for_event("PermissionRequest", "r"),
            "{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PermissionRequest\",\n    \"decision\": {\n      \"behavior\": \"deny\",\n      \"message\": \"r\"\n    }\n  }\n}\n"
        );
    }

    #[test]
    fn escape_matches_jq() {
        assert_eq!(
            escape("a\u{7f}b\u{1}c\u{1b}\u{b}é/<>\u{8}\u{c}\t\"\\"),
            "\"a\\u007fb\\u0001c\\u001b\\u000bé/<>\\b\\f\\t\\\"\\\\\""
        );
    }

    #[test]
    fn compact_and_empty() {
        let v = J::obj(vec![
            ("a", J::Num("1".into())),
            ("b", J::Arr(vec![])),
            ("c", J::Obj(vec![])),
            ("d", J::Arr(vec![J::Bool(true), J::Null])),
        ]);
        assert_eq!(v.compact(), r#"{"a":1,"b":[],"c":{},"d":[true,null]}"#);
        assert_eq!(
            v.pretty(),
            "{\n  \"a\": 1,\n  \"b\": [],\n  \"c\": {},\n  \"d\": [\n    true,\n    null\n  ]\n}"
        );
    }
}
