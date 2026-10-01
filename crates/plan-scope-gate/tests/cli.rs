//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//!
//! fixture は bash 版(旧 config/claude/hooks/plan-scope-gate.sh)から生成したもの
//! (docs/rust-migration.md の段1〜3)。trycmd は出力の `\` を `/` に
//! 正規化して比較するため、JSON 文字列中の `\n` は fixture 上 `/n` に見える。
//! エスケープ自体のバイト一致は hook-io の jqfmt の単体テストが受け持つ。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "plan-scope-gate",
            trycmd::cargo::cargo_bin!("plan-scope-gate"),
        )
        .case("tests/cmd/*.toml");
}
