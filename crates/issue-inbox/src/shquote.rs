//! bash の `printf %q`(Stop 出力の `procedure_cmd` を組み立てるのに使っていた)。
//!
//! bash 5.x の `lib/sh/shquote.c`: 空文字は `''`、表示できない文字(制御文字・
//! DEL)を含めば `$'...'`(`ansic_quote`)、それ以外は `sh_backslash_quote`
//! (シェルのメタ文字の前に `\`、先頭の `#`、語頭・`:`・`=` の直後の `~`)。
//! UTF-8 ロケールを前提にする(非 ASCII 文字は表示可能としてそのまま)。

fn is_backslash_char(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '\n'
            | '!'
            | '"'
            | '$'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | ','
            | ';'
            | '<'
            | '>'
            | '?'
            | '['
            | '\\'
            | ']'
            | '^'
            | '`'
            | '{'
            | '|'
            | '}'
    )
}

fn needs_ansic(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f)
}

fn ansic_quote(s: &str) -> String {
    let mut out = String::from("$'");
    for c in s.chars() {
        match c {
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\u{1b}' => out.push_str("\\E"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\{:03o}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// `printf '%q' "$s"`。
pub fn printf_q(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if needs_ansic(s) {
        return ansic_quote(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    let mut prev: Option<char> = None;
    for c in s.chars() {
        let first = prev.is_none();
        if is_backslash_char(c)
            || (c == '#' && first)
            || (c == '~' && (first || matches!(prev, Some(':' | '='))))
        {
            out.push('\\');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期待値は bash 5.x の `printf '%q|' ...` の実出力。
    #[test]
    fn matches_bash() {
        let cases = [
            ("Claude Code", r"Claude\ Code"),
            (
                "https://claude.com/claude-code",
                "https://claude.com/claude-code",
            ),
            ("a=b", "a=b"),
            ("~x", r"\~x"),
            ("a:~b", r"a:\~b"),
            ("x=~y", r"x=\~y"),
            ("#a", r"\#a"),
            ("a#b", "a#b"),
            ("a,b", r"a\,b"),
            ("it's", r"it\'s"),
            ("é", "é"),
            ("a\tb", r"$'a\tb'"),
            ("", "''"),
            ("a%b+c@d", "a%b+c@d"),
            ("{a}", r"\{a\}"),
            ("x]", r"x\]"),
            ("a~b", "a~b"),
            ("=~", r"=\~"),
        ];
        for (i, want) in cases {
            assert_eq!(printf_q(i), want, "{i:?}");
        }
    }
}
