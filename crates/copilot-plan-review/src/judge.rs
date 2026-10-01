//! judge(決定論的)・critic 出力のスキーマ検証・markdown レンダリング。
//!
//! 吸収元: `copilot-plan-review.sh` の `CRITIC_SCHEMA_JQ` / `JUDGE_JQ` /
//! `RENDER_DEFS`(`render_open` / `render_log` / `render_backlog`)/ `warn_text`。
//! jq の式を 1:1 で写している。`[[:space:]]` は `char::is_whitespace` で近似する。

use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// jq の `x // d`: null / false / 欠落なら `d`。
fn alt(v: Option<&Value>) -> Option<&Value> {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(x) => Some(x),
    }
}

/// jq の `tostring`。
fn tostring(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 文字列連結に使う `(.k // "d")`。
fn s_or(o: &Value, k: &str, d: &str) -> String {
    alt(o.get(k)).map(tostring).unwrap_or_else(|| d.to_string())
}

/// `def nonblank: (((. // "") | tostring | gsub("[[:space:]]+"; "")) | length) > 0;`
fn nonblank(v: Option<&Value>) -> bool {
    alt(v)
        .map(tostring)
        .is_some_and(|s| s.chars().any(|c| !c.is_whitespace()))
}

/// `def norm`: ascii_downcase → 空白の連続を 1 個の空白に → 前後の空白を除く。
fn norm(v: Option<&Value>) -> String {
    let s = alt(v)
        .map(tostring)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out.trim_matches(' ').to_string()
}

fn filled(f: &Value) -> bool {
    ["summary", "failure_mode", "trigger", "evidence"]
        .iter()
        .all(|k| nonblank(f.get(*k)))
}

fn merge(base: &Value, extra: Value) -> Value {
    let mut m: Map<String, Value> = base.as_object().cloned().unwrap_or_default();
    if let Value::Object(e) = extra {
        for (k, v) in e {
            m.insert(k, v);
        }
    }
    Value::Object(m)
}

fn sev_in(f: &Value, set: &[&str]) -> bool {
    f.get("severity")
        .and_then(Value::as_str)
        .is_some_and(|s| set.contains(&s))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Warn {
    pub invalid_severity: usize,
    pub nonconforming: usize,
    pub dup_dropped: usize,
    pub unknown_carryover: usize,
    pub missing_carryover: usize,
    pub readiness_inconsistent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Readiness {
    pub requirements: bool,
    pub scope: bool,
    pub implementation: bool,
    pub verification: bool,
}

/// `JUDGE_JQ` の出力。
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    pub gate: bool,
    pub round: Value,
    pub lenses: Vec<Value>,
    pub open: Vec<Value>,
    pub new_eligible: Vec<Value>,
    pub carried: Vec<Value>,
    pub closed: Vec<Value>,
    pub backlog: Vec<Value>,
    pub readiness: Readiness,
    pub warn: Warn,
}

impl Judged {
    pub fn to_value(&self) -> Value {
        let r = &self.readiness;
        let w = &self.warn;
        json!({
            "gate": self.gate,
            "round": self.round,
            "lenses": self.lenses,
            "open": self.open,
            "new_eligible": self.new_eligible,
            "carried": self.carried,
            "closed": self.closed,
            "backlog": self.backlog,
            "readiness": {
                "requirements": r.requirements, "scope": r.scope,
                "implementation": r.implementation, "verification": r.verification,
            },
            "warn": {
                "invalid_severity": w.invalid_severity,
                "nonconforming": w.nonconforming,
                "dup_dropped": w.dup_dropped,
                "unknown_carryover": w.unknown_carryover,
                "missing_carryover": w.missing_carryover,
                "readiness_inconsistent": w.readiness_inconsistent,
            },
        })
    }

    /// `suppress_closer_warn`: `.warn.readiness_inconsistent = false`。
    pub fn suppress_closer_warn(&mut self) {
        self.warn.readiness_inconsistent = false;
    }
}

const VALID: [&str; 4] = ["BLOCKER", "MAJOR", "MINOR", "NIT"];

/// `judge <round> <open> [gate]`。`critics` は `[{lens, data}]`。
pub fn judge(round: &str, open: &[Value], gate: &str, critics: &[Value]) -> Judged {
    let gates: Vec<&str> = gate.split(',').filter(|s| !s.is_empty()).collect();

    let mut flat_raw = Vec::new();
    for c in critics {
        let lens = c.get("lens").cloned().unwrap_or(Value::Null);
        let findings = alt(c.get("data").and_then(|d| d.get("findings")))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (i, f) in findings.iter().enumerate() {
            flat_raw.push(merge(f, json!({"_lens": lens, "_n": i + 1})));
        }
    }
    let flat: Vec<Value> = flat_raw
        .iter()
        .filter(|f| sev_in(f, &VALID))
        .cloned()
        .collect();
    let invalid_severity = flat_raw.len() - flat.len();
    let gsev: Vec<&Value> = flat.iter().filter(|f| sev_in(f, &gates)).collect();
    let conforming: Vec<&Value> = gsev.iter().copied().filter(|f| filled(f)).collect();
    let nonconforming = gsev.len() - conforming.len();
    let mut seen = HashSet::new();
    let dedup: Vec<&Value> = conforming
        .iter()
        .copied()
        .filter(|f| seen.insert(norm(f.get("summary"))))
        .collect();
    let dup_dropped = conforming.len() - dedup.len();
    let new_eligible: Vec<Value> = dedup
        .iter()
        .map(|f| {
            let id = format!(
                "R{}-{}-{}",
                round,
                s_or(f, "_lens", ""),
                f.get("_n").map(tostring).unwrap_or_default()
            );
            merge(f, json!({ "id": id }))
        })
        .collect();

    let co_raw: Vec<Value> = critics
        .iter()
        .flat_map(|c| {
            alt(c.get("data").and_then(|d| d.get("carryover")))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let open_ids: Vec<Value> = open
        .iter()
        .map(|o| o.get("id").cloned().unwrap_or(Value::Null))
        .collect();
    let co: Vec<&Value> = co_raw
        .iter()
        .filter(|c| open_ids.contains(c.get("id").unwrap_or(&Value::Null)))
        .collect();
    let unknown_carryover = co_raw.len() - co.len();
    let mut co_map: Map<String, Value> = Map::new();
    for c in co {
        let id = c.get("id").map(tostring).unwrap_or_default();
        if co_map.contains_key(&id) {
            co_map.insert(
                id.clone(),
                json!({"id": id, "status": "UNRESOLVED",
                       "rationale": "critic が同じ id に重複した判定を返した"}),
            );
        } else {
            co_map.insert(id, c.clone());
        }
    }
    let lookup = |o: &Value| -> Option<Value> {
        let id = o.get("id").map(tostring).unwrap_or_default();
        co_map.get(&id).filter(|v| !v.is_null()).cloned()
    };
    let status = |c: &Value| c.get("status").cloned().unwrap_or(Value::Null);
    let mut carried = Vec::new();
    let mut closed = Vec::new();
    for o in open {
        match lookup(o) {
            None => carried.push(merge(
                o,
                json!({"_carry": "UNRESOLVED", "_carry_missing": true, "_carry_rationale": ""}),
            )),
            Some(c) => {
                let st = status(&c);
                let rationale = alt(c.get("rationale"))
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
                if st == "UNRESOLVED" {
                    carried.push(merge(
                        o,
                        json!({"_carry": "UNRESOLVED", "_carry_missing": false,
                               "_carry_rationale": rationale}),
                    ));
                } else if st == "RESOLVED" || st == "REFUTED_BY_PLAN" {
                    closed.push(merge(
                        o,
                        json!({"_carry": st, "_carry_rationale": rationale}),
                    ));
                }
            }
        }
    }
    let missing_carryover = carried
        .iter()
        .filter(|c| c.get("_carry_missing") == Some(&Value::Bool(true)))
        .count();
    let mut newopen = carried.clone();
    newopen.extend(new_eligible.iter().cloned());
    let backlog: Vec<Value> = flat
        .iter()
        .filter(|f| !sev_in(f, &gates))
        .cloned()
        .collect();

    let mut readiness = Readiness {
        requirements: true,
        scope: true,
        implementation: true,
        verification: true,
    };
    for c in critics {
        let Some(r) = c.get("data").and_then(|d| d.get("readiness")) else {
            continue;
        };
        if r.is_null() {
            continue;
        }
        let ok = |k: &str| r.get(k) != Some(&Value::Bool(false));
        readiness.requirements &= ok("requirements");
        readiness.scope &= ok("scope");
        readiness.implementation &= ok("implementation");
        readiness.verification &= ok("verification");
    }
    let not_ready = !(readiness.requirements
        && readiness.scope
        && readiness.implementation
        && readiness.verification);

    Judged {
        gate: !newopen.is_empty(),
        round: round
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or(Value::Null),
        lenses: critics
            .iter()
            .map(|c| c.get("lens").cloned().unwrap_or(Value::Null))
            .collect(),
        open: newopen.clone(),
        new_eligible,
        carried,
        closed,
        backlog,
        warn: Warn {
            invalid_severity,
            nonconforming,
            dup_dropped,
            unknown_carryover,
            missing_carryover,
            readiness_inconsistent: not_ready && newopen.is_empty(),
        },
        readiness,
    }
}

// ---------------------------------------------------------------------------
// CRITIC_SCHEMA_JQ
// ---------------------------------------------------------------------------

fn has_exact_keys(v: &Value, spec: &[&str]) -> bool {
    let Some(m) = v.as_object() else {
        return false;
    };
    let mut a: Vec<&str> = m.keys().map(String::as_str).collect();
    let mut b: Vec<&str> = spec.to_vec();
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

fn is_str_in(v: Option<&Value>, set: &[&str]) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| set.contains(&s))
}

fn is_str(v: Option<&Value>) -> bool {
    v.is_some_and(Value::is_string)
}

fn is_bool(v: Option<&Value>) -> bool {
    v.is_some_and(Value::is_boolean)
}

fn valid_readiness(r: &Value) -> bool {
    const K: [&str; 4] = ["requirements", "scope", "implementation", "verification"];
    has_exact_keys(r, &K) && K.iter().all(|k| is_bool(r.get(*k)))
}

fn valid_finding(f: &Value) -> bool {
    has_exact_keys(
        f,
        &[
            "severity",
            "kind",
            "readiness_axis",
            "summary",
            "failure_mode",
            "trigger",
            "evidence",
        ],
    ) && is_str_in(f.get("severity"), &VALID)
        && is_str_in(f.get("kind"), &["TECHNICAL", "NEEDS_DECISION"])
        && is_str_in(
            f.get("readiness_axis"),
            &[
                "REQUIREMENTS",
                "SCOPE",
                "IMPLEMENTATION",
                "VERIFICATION",
                "NONE",
            ],
        )
        && ["summary", "failure_mode", "trigger", "evidence"]
            .iter()
            .all(|k| is_str(f.get(*k)))
}

fn valid_carry(c: &Value) -> bool {
    has_exact_keys(c, &["id", "status", "rationale"])
        && is_str(c.get("id"))
        && is_str_in(
            c.get("status"),
            &["RESOLVED", "UNRESOLVED", "REFUTED_BY_PLAN"],
        )
        && is_str(c.get("rationale"))
}

/// critic の生出力(パース済み)が契約(copilot-plan-review.schema.json)に適合するか。
pub fn valid_critic(v: &Value) -> bool {
    has_exact_keys(v, &["readiness", "findings", "carryover"])
        && v.get("readiness").is_some_and(valid_readiness)
        && v.get("findings")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().all(valid_finding))
        && v.get("carryover")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().all(valid_carry))
}

// ---------------------------------------------------------------------------
// RENDER_DEFS
// ---------------------------------------------------------------------------

fn block(f: &Value) -> String {
    let mut s = format!(
        "### [{}][{}] {}\n",
        s_or(f, "severity", "?"),
        s_or(f, "kind", "?"),
        s_or(f, "summary", "")
    );
    let id = s_or(f, "id", "");
    if !id.is_empty() {
        s += &format!("- id: {} / 軸: {}\n", id, s_or(f, "readiness_axis", "NONE"));
    }
    let carry = s_or(f, "_carry", "");
    if carry == "UNRESOLVED" {
        s += "- **前ラウンドから未解消**";
        if f.get("_carry_missing") == Some(&Value::Bool(true)) {
            s += "（critic が判定を返さなかったため保守的に未解消として扱った）";
        }
        s += "\n";
    }
    if carry == "RESOLVED" || carry == "REFUTED_BY_PLAN" {
        s += &format!("- {}: {}\n", carry, s_or(f, "_carry_rationale", ""));
    }
    s += &format!("- 失敗モード: {}\n", s_or(f, "failure_mode", ""));
    s += &format!("- 発生条件: {}\n", s_or(f, "trigger", ""));
    s += &format!("- 根拠: {}\n", s_or(f, "evidence", ""));
    s
}

/// `def blocks`。
pub fn blocks(items: &[Value]) -> String {
    if items.is_empty() {
        "（なし）\n".to_string()
    } else {
        items.iter().map(block).collect::<Vec<_>>().join("\n")
    }
}

fn readiness_line(r: &Readiness) -> String {
    let yn = |b: bool| if b { "yes" } else { "NO" };
    format!(
        "R={} S={} I={} T={}",
        yn(r.requirements),
        yn(r.scope),
        yn(r.implementation),
        yn(r.verification)
    )
}

/// `render_log` の jq 文字列(`jq -r` の末尾改行は含まない)。
pub fn render_log(j: &Judged) -> String {
    let lenses: Vec<String> = j.lenses.iter().map(tostring).collect();
    let w = &j.warn;
    format!(
        "## GATE: {}\n\n- ラウンド: {} / lens: {}\n- readiness: {}\n\
         - open set: {} 件 (新規 {} / 未解消 {})\n\
         - backlog (MINOR/NIT): {} 件\n\
         - 不適合で破棄: {} / 重複除去: {} / 不正 severity: {}\n\
         - carry-over: 未応答 {} / 未知 id {}\n\
         \n## open set（ゲート対象）\n\n{}\
         \n## 今ラウンドで解消 / 却下\n\n{}\
         \n## backlog (MINOR/NIT)\n\n{}",
        if j.gate { "DENY" } else { "PASS" },
        tostring(&j.round),
        lenses.join(", "),
        readiness_line(&j.readiness),
        j.open.len(),
        j.new_eligible.len(),
        j.carried.len(),
        j.backlog.len(),
        w.nonconforming,
        w.dup_dropped,
        w.invalid_severity,
        w.missing_carryover,
        w.unknown_carryover,
        blocks(&j.open),
        blocks(&j.closed),
        blocks(&j.backlog),
    )
}

/// `render_backlog` の jq 文字列(0 件なら空)。
pub fn render_backlog(j: &Judged) -> String {
    if j.backlog.is_empty() {
        String::new()
    } else {
        format!(
            "## ラウンド {}\n\n{}",
            tostring(&j.round),
            blocks(&j.backlog)
        )
    }
}

/// `warn_text`。
pub fn warn_text(w: &Warn) -> String {
    let mut parts = Vec::new();
    if w.nonconforming > 0 {
        parts.push(format!(
            "必須フィールドが空の BLOCKER/MAJOR {} 件を破棄しました",
            w.nonconforming
        ));
    }
    if w.invalid_severity > 0 {
        parts.push(format!(
            "未知の severity {} 件を破棄しました",
            w.invalid_severity
        ));
    }
    if w.missing_carryover > 0 {
        parts.push(format!(
            "carry-over {} 件に判定が返らなかったため未解消として扱いました",
            w.missing_carryover
        ));
    }
    if w.unknown_carryover > 0 {
        parts.push(format!(
            "open set に無い carry-over id {} 件を無視しました",
            w.unknown_carryover
        ));
    }
    if w.readiness_inconsistent {
        parts.push("readiness に未充足の軸があるのに、それを裏付ける証拠付き finding がありません（critic 出力の不備として素通しします）".to_string());
    }
    parts.join("。")
}

/// bash の `$(...)` が落とす末尾改行を同じく落とす。
pub fn chomp(s: &str) -> &str {
    s.trim_end_matches('\n')
}
