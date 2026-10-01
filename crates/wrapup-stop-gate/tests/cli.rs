//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//! 段 2 では同じファイルを crates/fixture-oracle から bash 版に向けていた。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "wrapup-stop-gate",
            trycmd::cargo::cargo_bin!("wrapup-stop-gate"),
        )
        .case("tests/cmd/*.toml");
}
