//! CLI 表面(引数・stdin の受け取り方)を tests/cmd/*.toml で走らせる
//! (trycmd、#391 の移植方法論)。集約ロジック(閾値・dedup・複数ディレクトリ
//! への副作用)は非決定な生成物(タイムスタンプ)を含み、trycmd のディレクトリ
//! 完全一致比較とは相性が悪いため、`tests/run_integration.rs` で
//! `verdict_escalate::run()` を直接検証する。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "verdict-escalate",
            trycmd::cargo::cargo_bin!("verdict-escalate"),
        )
        .case("tests/cmd/*.toml");
}
