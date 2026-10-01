//! python `json.dumps` の文字列エンコード(既定の `ensure_ascii=True`)。
//!
//! 移植元の python heredoc は `json.dumps(request)` で送っていた。herdr 側は
//! JSON としてしか読まないが、送信行をバイト一致で固定するため同じ規則で書く:
//! `"` と `\` と `\b \f \n \r \t` は短い escape、それ以外の印字可能 ASCII
//! (0x20..=0x7e)はそのまま、残り(制御文字・DEL・非 ASCII)は UTF-16 の
//! code unit ごとに小文字 16 進 4 桁の `\u` escape。

pub fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(s: &str) -> String {
        let mut o = String::new();
        push_str(&mut o, s);
        o
    }

    #[test]
    fn ascii_and_short_escapes() {
        assert_eq!(enc("a/b c~"), "\"a/b c~\"");
        assert_eq!(enc("\"\\\n\r\t\u{8}\u{c}"), r#""\"\\\n\r\t\b\f""#);
    }

    #[test]
    fn control_del_and_non_ascii() {
        assert_eq!(enc("\u{1}\u{7f}"), r#""\u0001\u007f""#);
        assert_eq!(enc("\u{e9}"), "\"\\u00e9\"");
        // サロゲートペア
        assert_eq!(enc("\u{1f600}"), "\"\\ud83d\\ude00\"");
    }
}
