//! 文字列の正規化・逐語照合・ハッシュ。記録の `sha` と、検出の引用の照合は、ここの 1 か所を使う。

use sha2::{Digest, Sha256};

/// 空白の連なり(改行を含む)を 1 個の空白にし、前後を落とす。照合とハッシュの前に通す。
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `needle` が `haystack` に、空白の違いを除いて逐語で含まれるか。`needle` が空なら偽
/// (空の引用が常に一致して、捏造を通してしまわないため)。
pub fn contains_verbatim(haystack: &str, needle: &str) -> bool {
    let n = normalize(needle);
    !n.is_empty() && normalize(haystack).contains(&n)
}

/// 正規化した本文の SHA-256 の先頭 32 桁。
pub fn sha(s: &str) -> String {
    let digest = Sha256::digest(normalize(s).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_differences_do_not_matter() {
        assert_eq!(normalize("  a \n\n b\t c "), "a b c");
        assert_eq!(sha("a  b"), sha("a\nb"));
        assert_ne!(sha("a b"), sha("a c"));
        assert_eq!(sha("x").len(), 32);
    }

    #[test]
    fn verbatim_match_ignores_whitespace_but_not_words() {
        let hay = "来週の火曜日か\n水曜日に 30 分ほど お電話できますか";
        assert!(contains_verbatim(
            hay,
            "水曜日に 30 分ほど お電話できますか"
        ));
        assert!(contains_verbatim(hay, "火曜日か 水曜日に"));
        assert!(!contains_verbatim(hay, "木曜日に"));
    }

    #[test]
    fn an_empty_needle_never_matches() {
        assert!(!contains_verbatim("anything", ""));
        assert!(!contains_verbatim("anything", "  \n"));
    }
}
