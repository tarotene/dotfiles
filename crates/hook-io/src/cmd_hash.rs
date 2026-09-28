//! コマンド文字列の正規化とハッシュ(ADR-543 段3)。`cmd-hash-log`(記録)と
//! `promotion-detect`(段4、SKILL.md コードブロックとの照合)が同じ正規化+
//! ハッシュを共有する必要があるため単一正本としてここに置く。
//!
//! 暗号学的ハッシュ(sha2 等)は導入しない: 目的は「同一コマンドの検出」で
//! あって改竄耐性は要らず、新規依存を増やすだけ(還元性)。
//! `std::hash::DefaultHasher` は Rust のバージョン間でアルゴリズムの安定性が
//! 保証されない(標準ライブラリのドキュメントに明記)ため、nix のリビルドを
//! 跨いで比較する用途には使えない——自前で FNV-1a(64-bit、仕様が単純で
//! 安定、依存ゼロ)を実装する。

/// 各行 trim → 連続空白を1つに → 空行除去 → 行末バックスラッシュ継続を結合。
pub fn normalize(cmd: &str) -> String {
    let joined = cmd.replace("\\\n", " ");
    let mut out = String::new();
    for line in joined.lines() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&collapsed);
    }
    out
}

/// FNV-1a 64-bit。仕様(オフセット/素数)は固定なので出力は安定。
pub fn fnv1a_hex(s: &str) -> String {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

/// 正規化してからハッシュする(`cmd-hash-log` / `promotion-detect` の
/// エントリポイント)。
pub fn hash(cmd: &str) -> String {
    fnv1a_hex(&normalize(cmd))
}

/// プレースホルダ(`<...>`)を含む正規化済みブロックは照合対象外
/// (段4 の promotion-detect が、SKILL.md の例文ブロックを誤って
/// 「逐語反復」と検出しないために使う)。
pub fn has_placeholder(normalized: &str) -> bool {
    normalized.contains('<') && normalized.contains('>')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace_and_blank_lines() {
        assert_eq!(
            normalize("  echo   hi  \n\n\n  echo bye  "),
            "echo hi\necho bye"
        );
    }

    #[test]
    fn normalize_joins_backslash_continuation() {
        assert_eq!(normalize("echo hi \\\n  --flag"), "echo hi --flag");
    }

    #[test]
    fn hash_is_stable_and_whitespace_insensitive() {
        let a = hash("echo   hi");
        let b = hash("echo hi");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert_ne!(hash("echo hi"), hash("echo bye"));
    }

    #[test]
    fn placeholder_detection() {
        assert!(has_placeholder("cmd <path> --flag"));
        assert!(!has_placeholder("cmd real/path --flag"));
        // 誤検出(false positive)は許容する — 保守的に「照合対象外」へ倒す
        // 方が、プレースホルダを本物のパスと誤って一致させるより安全。
        assert!(has_placeholder("cmd a < b > c"));
    }
}
