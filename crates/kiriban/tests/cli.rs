//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd)。
//!
//! 文面の fixture(`comment-*.toml` と `.stdout`)は、人・bot・前後賞・入れ子の
//! 見た目をそのまま固定する。規則そのものの境界は src/lib.rs の unit test が持つ。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin("kiriban", trycmd::cargo::cargo_bin!("kiriban"))
        .case("tests/cmd/*.toml");
}
