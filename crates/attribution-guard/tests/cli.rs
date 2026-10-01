//! tests/cmd/*.toml を実バイナリに向けて走らせる(trycmd、#391 の移植方法論)。
//!
//! fixture は bash 版から生成し、bash に対して緑を確認したものをそのまま
//! 移している:
//! - `st*.toml`: config/claude/hooks/attribution-guard.sh の --selftest 全 45
//!   アサーション(--check 経路。本文ファイルは `files/` を cwd にして相対参照)
//! - `codex-*.toml` / `copilot-*.toml`: 旧 Codex/Copilot adapter の --selftest
//!   各 4 件(stdin JSON、`--agent codex|copilot`)
//! - `hook-*.toml`: Claude の main() 分岐(MCP・不正 JSON 等)の追加ケース
//!
//! 他の guard の移植者向けのオラクルを兼ねる(docs/claude/guard-core.md)。

#[test]
fn cli() {
    trycmd::TestCases::new()
        .register_bin(
            "attribution-guard",
            trycmd::cargo::cargo_bin!("attribution-guard"),
        )
        .case("tests/cmd/*.toml");
}
