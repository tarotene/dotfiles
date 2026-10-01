//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd)。
//!
//! fixture は bash 版 `scripts/pr-title-check` の --selftest 全 20 ケース
//! (`st*.toml`、終了コードと stderr)と、main() の入出力経路(stdin・
//! 引数なし・`--help`)の追加ケース(`io-*.toml`)。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "pr-title-check",
            trycmd::cargo::cargo_bin!("pr-title-check"),
        )
        .case("tests/cmd/*.toml");
}
