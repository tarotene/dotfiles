//! 文書を段落(空行で区切られた塊)に分け、日本語かどうかの判定と、記録の鍵になる
//! ハッシュを出す。
//!
//! 分割は可逆にする: 各段落は「本文」と「後ろの区切り(空行の連続と、本文末の改行)」を持ち、
//! すべてを順に連結すると元の文書に戻る。書き直しは本文だけを差し替えるので、空行の数や
//! 末尾の改行が動かない。
//!
//! コードのフェンス(``` と ~~~)と、文書の先頭の front matter(`---` で囲む部分)は、
//! 中に空行があっても 1 つの段落として扱い、`verbatim` を立てる。書き直しの対象にしない。

use sha2::{Digest, Sha256};

/// 段落 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 本文(前後の空行と、末尾の改行を含まない)。
    pub text: String,
    /// 本文の後ろの区切り。本文末の改行と、続く空行。
    pub sep: String,
    /// フェンスか front matter を含む。書き直しの対象にしない。
    pub verbatim: bool,
}

/// 日本語の字(かな・漢字)がこの数に届いた段落を、日本語の段落とみなす。短い見出しや
/// 「検証」のような 1〜3 字の語だけの段落を、対象に巻き込まないため。
pub const MIN_JP_CHARS: usize = 4;

/// 文書を段落に分ける。[`join`] で元に戻る。
pub fn split(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cur = String::new();
    let mut sep = String::new();
    let mut verbatim = false;
    let mut fence: Option<&'static str> = None;
    let mut in_sep = false;

    for (idx, line) in text.split_inclusive('\n').enumerate() {
        let content = line.trim_end_matches(['\n', '\r']);
        let blank = content.trim().is_empty();

        if in_sep {
            if blank {
                sep.push_str(line);
                continue;
            }
            blocks.push(Block {
                text: std::mem::take(&mut cur),
                sep: std::mem::take(&mut sep),
                verbatim,
            });
            verbatim = false;
            in_sep = false;
        }

        let trimmed = content.trim_start();
        if let Some(marker) = fence {
            let closes = if marker == "---" {
                trimmed.trim_end() == "---"
            } else {
                trimmed.starts_with(marker)
            };
            if closes {
                fence = None;
            }
            cur.push_str(line);
            continue;
        }

        let opens = if idx == 0 && content.trim_end() == "---" {
            Some("---")
        } else if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        if let Some(marker) = opens {
            fence = Some(marker);
            verbatim = true;
            cur.push_str(line);
        } else if blank {
            // 本文末の改行は区切りの側へ移す(本文は改行を含まない)。
            sep.push_str(&take_trailing_newline(&mut cur));
            sep.push_str(line);
            in_sep = true;
        } else {
            cur.push_str(line);
        }
    }

    if in_sep || !cur.is_empty() {
        if !in_sep {
            sep.push_str(&take_trailing_newline(&mut cur));
        }
        blocks.push(Block {
            text: cur,
            sep,
            verbatim,
        });
    }
    blocks
}

/// [`split`] の逆。
pub fn join(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        out.push_str(&b.text);
        out.push_str(&b.sep);
    }
    out
}

fn take_trailing_newline(s: &mut String) -> String {
    if s.ends_with("\r\n") {
        s.truncate(s.len() - 2);
        "\r\n".to_string()
    } else if s.ends_with('\n') {
        s.pop();
        "\n".to_string()
    } else {
        String::new()
    }
}

/// かな・漢字の字数(半角カナを含む)。
pub fn jp_count(s: &str) -> usize {
    s.chars().filter(|c| is_jp(*c)).count()
}

fn is_jp(c: char) -> bool {
    matches!(c,
        '\u{3041}'..='\u{3096}'   // ひらがな
        | '\u{30A1}'..='\u{30FA}' // カタカナ
        | '\u{30FC}'              // 長音
        | '\u{FF66}'..='\u{FF9F}' // 半角カナ
        | '\u{3400}'..='\u{4DBF}' // CJK 統合漢字拡張 A
        | '\u{4E00}'..='\u{9FFF}' // CJK 統合漢字
        | '\u{F900}'..='\u{FAFF}' // CJK 互換漢字
    )
}

/// ハッシュの前に空白をそろえる。折り返しの位置やインデントが変わっても、同じ段落として扱う。
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 段落の記録の鍵。正規化した本文の SHA-256 の先頭 32 桁(16 進)。
pub fn hash(text: &str) -> String {
    let digest = Sha256::digest(normalize(text).as_bytes());
    let mut hex = String::with_capacity(32);
    for b in digest.iter().take(16) {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_join_round_trip() {
        for src in [
            "",
            "a",
            "a\n",
            "一段落目です。\n二行目。\n\n二段落目。\n",
            "\n\n先頭に空行。\n\n\n\n三つの空行の後。",
            "前。\n\n```\ncode\n\nmore code\n```\n\n後。\n",
            "---\ntitle: x\n\nkey: y\n---\n\n本文です。\n",
            "未閉じのフェンス。\n\n```\ncode\n\nまだコード。\n",
            "CRLF の行。\r\n\r\n次。\r\n",
        ] {
            assert_eq!(join(&split(src)), src, "round trip: {src:?}");
        }
    }

    #[test]
    fn fence_with_blank_lines_is_one_verbatim_block() {
        let blocks = split("前。\n\n```\ncode\n\nmore\n```\n\n後。\n");
        assert_eq!(blocks.len(), 3);
        assert!(!blocks[0].verbatim);
        assert!(blocks[1].verbatim);
        assert_eq!(blocks[1].text, "```\ncode\n\nmore\n```");
        assert!(!blocks[2].verbatim);
    }

    #[test]
    fn front_matter_is_verbatim_only_at_the_start() {
        let blocks = split("---\ntitle: 題名\n---\n\n本文\n\n---\n\n罫線のあと\n");
        assert!(blocks[0].verbatim);
        assert!(!blocks[1].verbatim);
        assert!(
            !blocks[2].verbatim,
            "文書の途中の --- は front matter でない"
        );
    }

    #[test]
    fn block_text_has_no_trailing_newline() {
        let blocks = split("一行目\n二行目\n\n次\n");
        assert_eq!(blocks[0].text, "一行目\n二行目");
        assert_eq!(blocks[0].sep, "\n\n");
        assert_eq!(blocks[1].text, "次");
        assert_eq!(blocks[1].sep, "\n");
    }

    #[test]
    fn counts_kana_and_kanji_only() {
        assert_eq!(jp_count("Hello, world 123"), 0);
        assert_eq!(jp_count("日本語です。"), 5);
        assert_eq!(jp_count("カタカナ・ｶﾀｶﾅ"), 8);
        assert!(jp_count("検証") < MIN_JP_CHARS);
    }

    #[test]
    fn hash_ignores_wrapping_and_indentation() {
        assert_eq!(hash("これは\n  文です。"), hash("これは 文です。"));
        assert_ne!(hash("これは文です。"), hash("これは文でした。"));
        assert_eq!(hash("x").len(), 32);
    }
}
