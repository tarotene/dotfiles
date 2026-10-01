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

/// bleep ADR-0003 までに書かれる `reason_id` と、閉語彙の `detail` 欄が
/// 読める(#675。未登録だと厳密な deserialize が失敗する)。`detail` が
/// 無いレコードも読める。
#[test]
fn bleep_adr_0003_reason_ids_and_detail_deserialize() {
    let base = |reason_id: &str, detail: Option<&str>| {
        let mut v = serde_json::json!({
            "v": 1,
            "ts": "2026-10-01T00:00:00Z",
            "tool": "bleep",
            "tool_version": "0.3.0",
            "repo": "tarotene/bleep",
            "host": "claude",
            "session_id": null,
            "verdict": "deny",
            "reason_id": reason_id,
            "match_class": "none",
            "term_hash": null,
            "tool_name": "Bash",
        });
        if let Some(d) = detail {
            v["detail"] = d.into();
        }
        v.to_string()
    };
    for id in [
        "lex-protocol-mismatch",
        "body-source-unresolved",
        "body-source-unreadable",
        "push-hook-bypass",
        "gh-noncanonical",
    ] {
        serde_json::from_str::<VerdictRecord>(&base(id, None))
            .unwrap_or_else(|e| panic!("{id} must be a known reason_id: {e}"));
    }
    let r: VerdictRecord =
        serde_json::from_str(&base("gh-noncanonical", Some("inline-body"))).unwrap();
    assert_eq!(r.detail.as_deref(), Some("inline-body"));
    // detail が無いレコードは None。書き戻しても欄は出ない。
    let r: VerdictRecord = serde_json::from_str(&base("push-hook-bypass", None)).unwrap();
    assert!(r.detail.is_none());
    assert!(!serde_json::to_string(&r).unwrap().contains("detail"));
}
