//! 本文マーカーの判定(grep -E の行単位マッチを素直に移したもの)。
//!
//! bash 版は `grep -qE "$RE" <<< "$text"` で判定していた。grep は行単位なので
//! `[[:space:]]*` は改行をまたがない — ここでも改行をまたがない。

/// 改行以外の POSIX 空白(grep が 1 行の中で `[[:space:]]` に一致させるもの)。
fn is_inline_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | 0x0b | 0x0c | b'\r')
}

/// 理由必須の閉じたタグ(`<tag>[[:space:]]*[^[:space:]'"`)]`)があれば真。
///
/// 吸収元: attribution-guard.sh の `NO_ATTRIBUTION_RE`(`No-Attribution:`)、
/// stack-base-guard.sh の `INDEP_RE`(`Independent-PR:`)。
/// 理由を伴って初めて成立する — 空のタグは通さない。除外集合にクォートと
/// `)` があるのは、範囲文字列全体を見るフォールバックで
/// `--body '確認しました No-Attribution:'` の閉じクォートを理由と誤認しない
/// ため。
pub fn has_reasoned_tag(text: &str, tag: &str) -> bool {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(off) = text[from..].find(tag) {
        let mut i = from + off + tag.len();
        while i < b.len() && is_inline_space(b[i]) {
            i += 1;
        }
        if i < b.len()
            && !matches!(b[i], b'\n' | b'\'' | b'"' | b'`' | b')')
            && !is_inline_space(b[i])
        {
            return true;
        }
        from += off + tag.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// `<lead>[[:space:]]*\[?<name>` が 1 行の中にあれば真(`lead` のいずれか)。
///
/// 吸収元: attribution-guard.sh の
/// `ATTRIBUTION_RE="(Generated with|Filed from)[[:space:]]*\[?${NAME}"`。
/// 検出は緩め — 文言の軽微なズレで false deny しない(要求する文言そのものは
/// 厳密)。
pub fn has_lead_then_name(text: &str, leads: &[&str], name: &str) -> bool {
    let b = text.as_bytes();
    leads.iter().any(|lead| {
        let mut from = 0;
        while let Some(off) = text[from..].find(lead) {
            let mut i = from + off + lead.len();
            while i < b.len() && is_inline_space(b[i]) {
                i += 1;
            }
            if i < b.len() && b[i] == b'[' && text[i + 1..].starts_with(name) {
                return true;
            }
            if text[i..].starts_with(name) {
                return true;
            }
            from += off + lead.chars().next().map_or(1, char::len_utf8);
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoned_tag() {
        let t = "No-Attribution:";
        assert!(has_reasoned_tag("x No-Attribution: 理由", t));
        assert!(has_reasoned_tag("No-Attribution:理由", t));
        assert!(!has_reasoned_tag("x No-Attribution: ", t));
        assert!(!has_reasoned_tag("No-Attribution:'", t));
        assert!(!has_reasoned_tag("No-Attribution:  )", t));
        // 改行をまたいで次行の文字を理由と見なさない(grep の行単位)
        assert!(!has_reasoned_tag("No-Attribution:\n理由", t));
        assert!(has_reasoned_tag("No-Attribution:\nNo-Attribution: y", t));
        assert!(has_reasoned_tag("Independent-PR: 独立", "Independent-PR:"));
    }

    #[test]
    fn lead_then_name() {
        let l = ["Generated with", "Filed from"];
        let n = "Claude Code";
        assert!(has_lead_then_name(
            "🤖 Generated with [Claude Code](u)",
            &l,
            n
        ));
        assert!(has_lead_then_name("Generated withClaude Code", &l, n));
        assert!(has_lead_then_name(
            "🤖 Filed from Claude Code wrap-up",
            &l,
            n
        ));
        assert!(!has_lead_then_name("🤖 Filed from wrap-up inbox", &l, n));
        assert!(!has_lead_then_name("Generated with\nClaude Code", &l, n));
        assert!(!has_lead_then_name("Generated with [[Claude Code", &l, n));
    }
}
