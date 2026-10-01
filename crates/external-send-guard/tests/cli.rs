//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//! fixture は段 1-2 で bash 版(config/claude/hooks/external-send-guard.sh)から
//! 生成し、bash に対して緑を確認したものをそのまま移している。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "external-send-guard",
            trycmd::cargo::cargo_bin!("external-send-guard"),
        )
        .case("tests/cmd/*.toml");
}
