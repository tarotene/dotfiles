//! judge / render / warn_text の単体テスト(旧 `copilot-plan-review.sh --selftest`
//! の「judge:」節 1〜13 を写したもの)。

use copilot_plan_review::judge::{blocks, judge, render_backlog, valid_critic, warn_text};
use serde_json::{json, Value};

fn ready_all() -> Value {
    json!({"requirements":true,"scope":true,"implementation":true,"verification":true})
}
fn ready_no_r() -> Value {
    json!({"requirements":false,"scope":true,"implementation":true,"verification":true})
}

fn finding(sev: &str, summary: &str) -> Value {
    finding_ev(sev, summary, "evidence.md:1")
}
fn finding_ev(sev: &str, summary: &str, ev: &str) -> Value {
    json!({"severity": sev, "kind": "TECHNICAL", "readiness_axis": "IMPLEMENTATION",
           "summary": summary, "failure_mode": "壊れる", "trigger": "常に", "evidence": ev})
}
fn critic(lens: &str, f: Value, c: Value) -> Value {
    critic_r(lens, f, c, ready_all())
}
fn critic_r(lens: &str, f: Value, c: Value, r: Value) -> Value {
    json!({"lens": lens, "data": {"readiness": r, "findings": f, "carryover": c}})
}
fn open1() -> Vec<Value> {
    vec![
        json!({"id":"R1-A-1","severity":"BLOCKER","kind":"TECHNICAL",
                "readiness_axis":"IMPLEMENTATION","summary":"前ラウンドの指摘",
                "failure_mode":"壊れる","trigger":"常に","evidence":"a.sh:1"}),
    ]
}
const G: &str = "BLOCKER,MAJOR";

fn count_headers(s: &str) -> usize {
    s.lines().filter(|l| l.starts_with("### ")).count()
}

#[test]
fn blocker_one() {
    let c = critic(
        "A",
        json!([finding("BLOCKER", "存在しない関数を呼んでいる")]),
        json!([]),
    );
    let j = judge("1", &[], G, &[c]);
    assert!(j.gate);
    assert_eq!(j.open.len(), 1);
    assert_eq!(j.open[0]["id"], "R1-A-1");
    assert_eq!(count_headers(&blocks(&j.open)), 1);
}

#[test]
fn minor_nit_only() {
    let c = critic(
        "A",
        json!([
            finding("MINOR", "命名が惜しい"),
            finding("NIT", "句点が揺れている")
        ]),
        json!([]),
    );
    let j = judge("1", &[], G, &[c]);
    assert!(!j.gate);
    assert_eq!(j.backlog.len(), 2);
    assert_eq!(count_headers(&render_backlog(&j)), 2);
}

#[test]
fn empty_evidence_is_nonconforming() {
    let c = critic(
        "A",
        json!([finding_ev("BLOCKER", "根拠なし", "   ")]),
        json!([]),
    );
    let j = judge("1", &[], G, &[c]);
    assert!(!j.gate);
    assert_eq!(j.warn.nonconforming, 1);
    assert!(warn_text(&j.warn).contains("破棄"));
}

#[test]
fn readiness_inconsistent() {
    let c = critic_r("A", json!([]), json!([]), ready_no_r());
    let j = judge("1", &[], G, &[c]);
    assert!(!j.gate);
    assert!(j.warn.readiness_inconsistent);
    assert!(warn_text(&j.warn).contains("readiness"));
    let c = critic_r(
        "A",
        json!([finding("MAJOR", "要件が未定義")]),
        json!([]),
        ready_no_r(),
    );
    let j = judge("1", &[], G, &[c]);
    assert!(!j.warn.readiness_inconsistent);
}

#[test]
fn invalid_severity() {
    let c = critic(
        "A",
        json!([finding("CRITICAL", "未知の severity")]),
        json!([]),
    );
    let j = judge("1", &[], G, &[c]);
    assert!(!j.gate);
    assert_eq!(j.warn.invalid_severity, 1);
}

#[test]
fn dedup_across_lenses() {
    let a = critic(
        "A",
        json!([finding("BLOCKER", "同じ  問題を   指摘")]),
        json!([]),
    );
    let b = critic(
        "B",
        json!([finding("BLOCKER", "同じ 問題を 指摘")]),
        json!([]),
    );
    let j = judge("1", &[], G, &[a, b]);
    assert_eq!(j.open.len(), 1);
    assert_eq!(j.warn.dup_dropped, 1);
}

#[test]
fn carryover_unresolved_keeps_gate() {
    let c = critic(
        "C",
        json!([]),
        json!([{"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ直っていない"}]),
    );
    let j = judge("2", &open1(), G, &[c]);
    assert!(j.gate);
    assert_eq!(j.open.len(), 1);
    assert_eq!(j.open[0]["id"], "R1-A-1");
}

#[test]
fn carryover_resolved_and_refuted() {
    for st in ["RESOLVED", "REFUTED_BY_PLAN"] {
        let c = critic(
            "C",
            json!([]),
            json!([{"id":"R1-A-1","status":st,"rationale":"x"}]),
        );
        let j = judge("2", &open1(), G, &[c]);
        assert!(!j.gate, "{st}");
        assert_eq!(j.closed.len(), 1, "{st}");
    }
}

#[test]
fn carryover_duplicate_is_unresolved() {
    let c = critic(
        "C",
        json!([]),
        json!([{"id":"R1-A-1","status":"RESOLVED","rationale":"解消した"},
               {"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ残る"}]),
    );
    let j = judge("2", &open1(), G, &[c]);
    assert!(j.gate);
    assert_eq!(j.open[0]["_carry"], "UNRESOLVED");
}

#[test]
fn carryover_missing() {
    let c = critic("C", json!([]), json!([]));
    let j = judge("2", &open1(), G, &[c]);
    assert!(j.gate);
    assert_eq!(j.warn.missing_carryover, 1);
    assert!(blocks(&j.open).contains("前ラウンドから未解消"));
}

#[test]
fn carryover_unknown_id() {
    let c = critic(
        "C",
        json!([]),
        json!([{"id":"R1-A-1","status":"RESOLVED","rationale":"ok"},
               {"id":"R9-Z-9","status":"UNRESOLVED","rationale":"知らない id"}]),
    );
    let j = judge("2", &open1(), G, &[c]);
    assert!(!j.gate);
    assert_eq!(j.warn.unknown_carryover, 1);
}

#[test]
fn gate_threshold() {
    let c = critic("A", json!([finding("MAJOR", "手戻りが確実")]), json!([]));
    let j = judge("1", &[], "BLOCKER", std::slice::from_ref(&c));
    assert!(!j.gate);
    assert_eq!(j.backlog.len(), 1);
    let j = judge("1", &[], G, &[c]);
    assert!(j.gate);
}

#[test]
fn closer_empty_gate() {
    let c = critic(
        "Z",
        json!([
            finding("BLOCKER", "closer が見つけた新規"),
            finding("MAJOR", "もう一件")
        ]),
        json!([]),
    );
    let j = judge("3", &[], "", &[c]);
    assert!(!j.gate);
    assert_eq!(j.new_eligible.len(), 0);
    assert_eq!(j.backlog.len(), 2);

    let c = critic(
        "Z",
        json!([finding("BLOCKER", "closer が見つけた新規")]),
        json!([{"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ直っていない"}]),
    );
    let j = judge("3", &open1(), "", &[c]);
    assert!(j.gate);
    assert_eq!(j.open.len(), 1);
    assert_eq!(j.new_eligible.len(), 0);

    let c = critic(
        "Z",
        json!([]),
        json!([{"id":"R1-A-1","status":"RESOLVED","rationale":"直した"}]),
    );
    let j = judge("3", &open1(), "", &[c]);
    assert!(!j.gate);
    assert_eq!(j.closed.len(), 1);
}

#[test]
fn closer_suppresses_readiness_warn() {
    let c = critic_r(
        "Z",
        json!([]),
        json!([{"id":"R1-A-1","status":"RESOLVED","rationale":"直した"}]),
        ready_no_r(),
    );
    let mut j = judge("3", &open1(), "", &[c]);
    assert!(j.warn.readiness_inconsistent);
    j.suppress_closer_warn();
    assert!(!j.warn.readiness_inconsistent);
    assert_eq!(warn_text(&j.warn), "");
}

#[test]
fn legacy_markdown_is_not_json() {
    let legacy = "- [技術] 旧形式のレビュー本文\n\nVERDICT: REQUEST_CHANGES\n";
    assert!(serde_json::from_str::<Value>(legacy).is_err());
}

#[test]
fn schema_validator() {
    let good =
        json!({"readiness": ready_all(), "findings": [finding("BLOCKER","x")], "carryover": []});
    assert!(valid_critic(&good));
    let mut bad = good.clone();
    bad.as_object_mut().unwrap().remove("carryover");
    assert!(!valid_critic(&bad));
    let mut bad = good.clone();
    bad["extra"] = json!("unexpected");
    assert!(!valid_critic(&bad));
    let mut bad = good.clone();
    bad["findings"][0]["severity"] = json!("CRITICAL");
    assert!(!valid_critic(&bad));
    let mut bad = good.clone();
    bad["readiness"]["requirements"] = json!("true");
    assert!(!valid_critic(&bad));
    let mut bad = good.clone();
    bad["findings"][0]["extra"] = json!("nope");
    assert!(!valid_critic(&bad));
    let mut bad = good;
    bad["carryover"] = json!([{"id":"x","status":"MAYBE","rationale":"r"}]);
    assert!(!valid_critic(&bad));
}
