//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "claude-statusline",
            trycmd::cargo::cargo_bin!("claude-statusline"),
        )
        .case("tests/cmd/*.toml");
}
