//! モデルに壊させたくない記法を、送る前に伏せ字(`⟦P0⟧`)に置き換え、戻ってきた文で
//! 同じ伏せ字が残っているかを照合する。
//!
//! 伏せるもの: 見出しの行、`>` の引用行、`Closes #N` などの参照の行、末尾の生成元の行、`No-Issue:` などの
//! タグの接頭辞、`「…」`・`『…』` の引用(本人の言葉・原文の逐語。書き換えられると、保存した原文との照合が
//! 落ちる。F8・F18)、インラインコード、HTML タグ・自動リンク、リンクの宛先、URL、Issue/PR の参照。
//! フェンスと front matter は、段落の分割の時点で対象から外している([`crate::blocks`])。
//!
//! 見出しの行を丸ごと伏せるのは、PR 本文の `## 課題` や `## 要確認` のように、別の検査が
//! 見出しの語で節を探すものがあるため。
//!
//! 数値と行頭の記法は、伏せずに前後で比べる([`numbers`]、[`structure`])。

use regex::{Captures, Regex};
use std::sync::OnceLock;

const OPEN: char = '⟦';
const CLOSE: char = '⟧';

/// 伏せ字に置き換えた結果。`slots[n]` が `⟦Pn⟧` の元の文字列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub slots: Vec<String>,
}

macro_rules! lazy_re {
    ($name:ident, $pat:expr) => {
        fn $name() -> &'static Regex {
            static CELL: OnceLock<Regex> = OnceLock::new();
            CELL.get_or_init(|| Regex::new($pat).expect("valid regex"))
        }
    };
}

// 行ごとに丸ごと伏せるもの。
lazy_re!(
    whole_line_re,
    r"(?m)^[ \t]*(?:#{1,6}[ \t].*|>.*|🤖 Generated with .*|Co-Authored-By:.*|Signed-off-by:.*|(?:Closes|Fixes|Resolves)[ \t]+\S.*)$"
);
// 逐語の引用。入れ子は伏せず(内側の対だけが対象)、600 字までの範囲に限る(対にならない括弧が、遠くの括弧まで飲まないため)。
lazy_re!(quote_re, r"「[^「」]{0,600}」|『[^『』]{0,600}』");
// 接頭辞だけ伏せて、後ろの理由は書き直しの対象に残すもの。
lazy_re!(
    tag_prefix_re,
    r"(?m)^[ \t]*(?:No-Issue|No-Visual|No-Attribution|Independent-PR|Stack|Blocked-Upstream|Obsolete|User-Excluded|Reference-Only):"
);
lazy_re!(html_re, r"<[A-Za-z/!][^<>\n]*>");
lazy_re!(link_dest_re, r"\]\(([^)\n]*)\)");
lazy_re!(url_re, r"https?://[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+");
lazy_re!(ref_re, r"(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+");
lazy_re!(token_re, r"⟦P([0-9]+)⟧");
lazy_re!(number_re, r"[0-9]+(?:[.,][0-9]+)*");
lazy_re!(
    marker_re,
    r"^([ \t]*)(#{1,6}[ \t]|[-*+][ \t]|[0-9]+[.)][ \t]|>|\|)"
);

#[derive(Default)]
struct Masker {
    slots: Vec<String>,
}

impl Masker {
    fn put(&mut self, s: &str) -> String {
        let n = self.slots.len();
        self.slots.push(s.to_string());
        format!("{OPEN}P{n}{CLOSE}")
    }
}

/// 記法を伏せ字に置き換える。
pub fn mask(text: &str) -> Masked {
    let mut m = Masker::default();
    let mut t = text.to_string();

    t = whole_line_re()
        .replace_all(&t, |c: &Captures| m.put(&c[0]))
        .into_owned();
    t = tag_prefix_re()
        .replace_all(&t, |c: &Captures| m.put(&c[0]))
        .into_owned();
    // 引用は、コード片・URL より先に伏せる。引用の中のコード片が先に伏せ字になると、引用の伏せ字の元の文字列に
    // 伏せ字が入り、`unmask` の 1 回の置換では戻らない。
    t = quote_re()
        .replace_all(&t, |c: &Captures| m.put(&c[0]))
        .into_owned();
    t = mask_code_spans(&t, &mut m);
    t = html_re()
        .replace_all(&t, |c: &Captures| m.put(&c[0]))
        .into_owned();
    t = link_dest_re()
        .replace_all(&t, |c: &Captures| format!("]{}", m.put(&c[0][1..])))
        .into_owned();
    t = url_re()
        .replace_all(&t, |c: &Captures| {
            // 文末の句読点や閉じ括弧は URL に含めない。
            let full = &c[0];
            let url = full.trim_end_matches(['.', ',', ';', ':', ')', '\'', '!']);
            format!("{}{}", m.put(url), &full[url.len()..])
        })
        .into_owned();
    t = ref_re()
        .replace_all(&t, |c: &Captures| m.put(&c[0]))
        .into_owned();

    Masked {
        text: t,
        slots: m.slots,
    }
}

/// `` `code` `` と ``` ``code`` ```: 同じ長さのバッククォートの連なりで囲まれた範囲を伏せる。
fn mask_code_spans(text: &str, m: &mut Masker) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] == '`' {
            i += 1;
        }
        let n = i - start;
        // 閉じる側: ちょうど n 個のバッククォートの連なり。
        let mut j = i;
        let mut close = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let s = j;
                while j < chars.len() && chars[j] == '`' {
                    j += 1;
                }
                if j - s == n {
                    close = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                let span: String = chars[start..end].iter().collect();
                out.push_str(&m.put(&span));
                i = end;
            }
            None => out.extend(&chars[start..i]),
        }
    }
    out
}

/// 伏せ字を元に戻す。範囲外の番号は触らない。
pub fn unmask(masked: &str, slots: &[String]) -> String {
    token_re()
        .replace_all(masked, |c: &Captures| {
            c[1].parse::<usize>()
                .ok()
                .and_then(|n| slots.get(n))
                .cloned()
                .unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

/// 文中の伏せ字の番号(出現順)。
pub fn token_ids(s: &str) -> Vec<usize> {
    token_re()
        .captures_iter(s)
        .filter_map(|c| c[1].parse().ok())
        .collect()
}

/// 前後の文脈として見せるための文字列。伏せ字は番号を消して `⟦…⟧` にする(文脈の中の
/// 伏せ字を、対象の伏せ字と取り違えて出力に混ぜないため)。
pub fn redact(text: &str) -> String {
    let m = mask(text);
    token_re().replace_all(&m.text, "⟦…⟧").into_owned()
}

/// 伏せ字を除いた文に出てくる数値(全角の数字は半角にそろえる)。並べ替えて返すので、
/// 前後で `==` で比べれば、数値の集合が変わっていないか分かる。
pub fn numbers(masked: &str) -> Vec<String> {
    let without_tokens = token_re().replace_all(masked, " ");
    let ascii: String = without_tokens
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            _ => c,
        })
        .collect();
    let mut v: Vec<String> = number_re()
        .find_iter(&ascii)
        .map(|m| m.as_str().to_string())
        .collect();
    v.sort();
    v
}

/// 行頭(インデントの後)にある伏せ字の番号(出現順)。`No-Visual:` などのタグの接頭辞は、伏せ字になって行頭に
/// あり、書き直しで文中へ動くと PR 本文の骨組みが壊れる。伏せ字の個数の照合は、位置を見ない。
pub fn line_head_ids(s: &str) -> Vec<usize> {
    s.lines()
        .filter_map(|line| {
            let t = line.trim_start_matches([' ', '\t']);
            let c = token_re().captures(t)?;
            (c.get(0)?.start() == 0).then(|| c[1].parse().ok())?
        })
        .collect()
}

/// 行頭の記法(見出し・箇条書き・番号付き・引用・表)を、インデントの幅つきで順に並べたもの。
/// 前後で `==` なら、構造が変わっていない。番号付きの番号そのものは比べない。
pub fn structure(s: &str) -> Vec<String> {
    s.lines()
        .filter_map(|line| {
            let c = marker_re().captures(line)?;
            let indent = c[1].chars().count();
            let kind = if c[2].chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                "N".to_string()
                    + c[2]
                        .trim_start_matches(|ch: char| ch.is_ascii_digit())
                        .trim()
            } else {
                c[2].trim().to_string()
            };
            Some(format!("{indent}:{kind}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(src: &str) -> Masked {
        let m = mask(src);
        assert_eq!(unmask(&m.text, &m.slots), src, "unmask で元に戻る");
        m
    }

    #[test]
    fn masks_inline_code_and_restores() {
        let m = round_trip("`cargo test` を実行し、``a ` b`` も見る。");
        assert!(!m.text.contains("cargo"));
        assert!(!m.text.contains("a `"));
        assert_eq!(m.slots.len(), 2);
    }

    #[test]
    fn unbalanced_backtick_is_left_alone() {
        let m = round_trip("対にならない ` バッククォート。");
        assert!(m.slots.is_empty());
    }

    #[test]
    fn masks_urls_but_not_the_punctuation_after_them() {
        let m = round_trip("詳細は https://example.com/a/b?c=1 を参照。末尾は https://x.dev/y。");
        assert!(!m.text.contains("example.com"));
        assert!(m.text.contains("を参照"));
        assert!(m.slots.contains(&"https://example.com/a/b?c=1".to_string()));
        assert!(m.slots.contains(&"https://x.dev/y".to_string()));
    }

    #[test]
    fn keeps_link_text_but_masks_destination() {
        let m = round_trip("[設計メモ](docs/design.md) を読む。");
        assert!(m.text.starts_with("[設計メモ]"));
        assert!(!m.text.contains("docs/design.md"));
    }

    #[test]
    fn masks_quoted_spans_verbatim_and_restores() {
        // F8・F18: 「…」の中は、書き直しに渡さない。「することができる」が「できる」に書き換わる事故を、構造で消す。
        let m = round_trip(
            "彼は「することができる」と述べ、『再使用をすることができるもの』も挙げた。",
        );
        assert!(!m.text.contains("することができる"), "{}", m.text);
        assert!(!m.text.contains("再使用"));
        assert!(m.text.contains("と述べ"));
        assert_eq!(m.slots.len(), 2);
        // 引用の中のコード片は、引用の伏せ字の中にそのまま入る(先にコード片を伏せない)。
        let m = round_trip("手順は「`cargo test` を実行する」と書いた。");
        assert_eq!(m.slots, vec!["「`cargo test` を実行する」".to_string()]);
        // 対にならない括弧は、そのまま。入れ子は内側の対だけを伏せる。
        let m = round_trip("閉じていない「括弧の文です。");
        assert!(m.slots.is_empty());
        let m = round_trip("外側「内側「深い」の外」です。");
        assert_eq!(m.slots, vec!["「深い」".to_string()]);
    }

    #[test]
    fn masks_blockquote_lines_whole() {
        let m =
            round_trip("前の文です。\n> 引用された原文です。\n> 二行目の原文です。\n後の文です。");
        assert!(!m.text.contains("原文"), "{}", m.text);
        assert!(m.text.contains("前の文です。") && m.text.contains("後の文です。"));
        assert_eq!(m.slots.len(), 2);
        // 引用の行は伏せ字になって行頭に残る。書き直しで動くと、照合が外す(`line_head_ids`)。
        assert_eq!(line_head_ids(&m.text).len(), 2);
    }

    #[test]
    fn line_head_ids_lists_only_tokens_at_a_line_start() {
        let s = "⟦P0⟧ 理由です。\n  ⟦P3⟧ 字下げの後。\n文中の ⟦P1⟧ は数えない。\n- ⟦P2⟧ 箇条書きの後は行頭でない。";
        assert_eq!(line_head_ids(s), vec![0, 3]);
        assert!(line_head_ids("伏せ字の無い文。").is_empty());
    }

    #[test]
    fn masks_issue_refs() {
        let m = round_trip("#12 と tarotene/dotfiles#34 を直す。");
        assert!(!m.text.contains("#12"));
        assert!(!m.text.contains("dotfiles#34"));
    }

    #[test]
    fn masks_headings_and_trailers_whole() {
        let src = "## 課題\nCloses #5\nNo-Issue: 理由を書く\n🤖 Generated with [Claude Code](https://claude.com/claude-code)";
        let m = round_trip(src);
        assert!(!m.text.contains("課題"), "見出しは丸ごと伏せる: {}", m.text);
        assert!(!m.text.contains("Closes"));
        assert!(!m.text.contains("Generated"));
        // タグは接頭辞だけ。理由は書き直しの対象に残る。
        assert!(m.text.contains("理由を書く"));
        assert!(!m.text.contains("No-Issue"));
    }

    #[test]
    fn masks_html_tags_so_they_cannot_break_the_request_format() {
        let m = round_trip("手順は <para n=\"1\"> と </para> のように書く。");
        assert!(!m.text.contains("<para"));
        assert!(!m.text.contains("</para>"));
    }

    #[test]
    fn redact_hides_token_numbers() {
        let r = redact("`code` と https://example.com");
        assert!(!r.contains("P0"));
        assert!(r.contains("⟦…⟧"));
    }

    #[test]
    fn token_ids_list_in_order() {
        assert_eq!(token_ids("a ⟦P2⟧ b ⟦P0⟧ c ⟦P2⟧"), vec![2, 0, 2]);
    }

    #[test]
    fn numbers_ignore_tokens_and_normalize_fullwidth() {
        assert_eq!(
            numbers("３つの⟦P12⟧と 1,000 件、v0.67 と 2"),
            vec!["0.67", "1,000", "2", "3"]
        );
        assert_eq!(numbers("三つ"), Vec::<String>::new());
    }

    #[test]
    fn structure_tracks_markers_and_indent() {
        let a = "- 一つ目\n  - 入れ子\n1. 番号\n2) 括弧\n> 引用\n| 表 |";
        assert_eq!(
            structure(a),
            vec!["0:-", "2:-", "0:N.", "0:N)", "0:>", "0:|"]
        );
        // 番号の値や本文が変わっても同じ。
        let b = "- 変えた\n  - 入れ子も\n7. 別の番号\n9) 括弧\n> 引用2\n| 表2 |";
        assert_eq!(structure(a), structure(b));
        // 入れ子が外れたら違う。
        assert_ne!(structure(a), structure("- 一つ目\n- 入れ子\n"));
    }
}
