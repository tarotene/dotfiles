//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//!
//! fixture は bash 版(旧 config/claude/hooks/plan-precedent-gate.sh)から生成したもの
//! (docs/rust-migration.md の段1〜3)。trycmd は出力の `\` を `/` に
//! 正規化して比較するため、JSON 文字列中の `\n` は fixture 上 `/n` に見える。
//! エスケープ自体のバイト一致は hook-io の jqfmt の単体テストが受け持つ。

#[test]
fn cli() {
    // 例文の取得日は固定する。`TRYCMD=overwrite` で更新しても実行日が
    // ゴールデンに入り込まない(#752)。
    trycmd::TestCases::new()
        .env("PLAN_PRECEDENT_GATE_TODAY", "2000-01-01")
        .register_bin(
            "plan-precedent-gate",
            trycmd::cargo::cargo_bin!("plan-precedent-gate"),
        )
        .case("tests/cmd/*.toml");
}
