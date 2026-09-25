//! `docs/schemas/agent-verdict.schema.json` が `record::VerdictRecord` から
//! 生成される内容と一致していることを検証する(ADR-0000 D8: 型が正本、
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
