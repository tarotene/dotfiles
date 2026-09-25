//! `docs/schemas/agent-verdict.schema.json` を再生成する。
//!
//! 使い方(リポジトリルートから):
//!   cargo run -p verdict-escalate --example gen_schema > docs/schemas/agent-verdict.schema.json
//!
//! `tests/schema.rs` の `schema_is_up_to_date` がこの生成結果と
//! チェックイン済みファイルの一致を検証する。

fn main() {
    let schema = schemars::schema_for!(verdict_escalate::record::VerdictRecord);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
