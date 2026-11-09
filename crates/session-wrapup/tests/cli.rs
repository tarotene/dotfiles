//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//! `issue-inbox` は別 package の bin なので、common の `sibling` で探す。

mod common;

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "session-wrapup",
            trycmd::cargo::cargo_bin!("session-wrapup"),
        )
        .register_bin("issue-inbox", common::sibling("issue-inbox"))
        .case("tests/cmd/*.toml");
}
