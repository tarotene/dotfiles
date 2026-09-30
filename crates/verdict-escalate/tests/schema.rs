//! `docs/schemas/agent-verdict.schema.json` が `record::VerdictRecord` から
//! 生成される内容と一致していることを検証する(ADR-478 D8: 型が正本、
//! Schema は生成物)。

use verdict_escalate::record::VerdictRecord;

#[test]
fn schema_is_up_to_date() {
    let schema = schemars::schema_for!(VerdictRecord);
    let generated = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/schemas/agent-verdict.schema.json"
    );
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    assert_eq!(
        generated, on_disk,
        "\ndocs/schemas/agent-verdict.schema.json is stale. Regenerate with:\n  \
         cargo run -p verdict-escalate --example gen_schema > docs/schemas/agent-verdict.schema.json\n"
    );
}

/// bleep#34 が使い始めた `unresolved-var` が閉語彙 enum として読める
/// (#573。未登録だと厳密な deserialize が失敗する)。
#[test]
fn unresolved_var_reason_id_deserializes() {
    let line = serde_json::json!({
        "v": 1,
        "ts": "2026-09-30T00:00:00Z",
        "tool": "bleep",
        "tool_version": "0.2.0",
        "repo": "tarotene/bleep",
        "host": "claude",
        "session_id": "s",
        "verdict": "ask",
        "reason_id": "unresolved-var",
        "match_class": "none",
        "term_hash": "h",
        "tool_name": "Bash",
    })
    .to_string();
    serde_json::from_str::<VerdictRecord>(&line).expect("unresolved-var must be a known reason_id");
}
