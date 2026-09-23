//! 最小の POSIX シェル語分割。判定に使えない入力(複合コマンド・展開・
//! リダイレクト・glob)は `None` にし、呼び出し側は素通し(何も判定しない)に倒す。
//!
//! 拒否集合は `config/claude/hooks/git-worktree-allow.sh:43-55` の複合コマンド
//! 判定(`;` `&` `|` `$(` バッククォート `>` `<` 改行)を、クォートを理解する
//! 形に広げたもの。本文(`--body`)に `|` や `>` を含む Markdown を渡すのは
//! 日常的なので、クォート内の演算子文字は拒否しない。一方、`$` はダブル
//! クォート内でも展開されるため、クォートの内外を問わず拒否する。

/// `cmd` を語に分割する。安全に静的解釈できないときは `None`。
pub fn split(cmd: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        ch => cur.push(ch),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' => return None,
                        '\\' => match chars.next()? {
                            ch @ ('"' | '\\' | '$' | '`') => cur.push(ch),
                            '\n' => {}
                            ch => {
                                cur.push('\\');
                                cur.push(ch);
                            }
                        },
                        ch => cur.push(ch),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next()? {
                    '\n' => {}
                    ch => cur.push(ch),
                }
            }
            ';' | '&' | '|' | '<' | '>' | '\n' | '\r' | '$' | '`' | '(' | ')' | '*' | '?' | '['
            | '{' | '}' => return None,
            '#' | '~' if !in_word => return None,
            ch => {
                in_word = true;
                cur.push(ch);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::split;

    fn s(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|x| x.to_string()).collect())
    }

    #[test]
    fn plain_and_quoted() {
        assert_eq!(split("gh pr edit 12"), s(&["gh", "pr", "edit", "12"]));
        assert_eq!(
            split("gh pr edit 12 --title 'a b' --body \"x | y > z; w\""),
            s(&[
                "gh",
                "pr",
                "edit",
                "12",
                "--title",
                "a b",
                "--body",
                "x | y > z; w"
            ])
        );
        assert_eq!(split(r#"a "q\"x" b\ c"#), s(&["a", "q\"x", "b c"]));
        assert_eq!(split("a ''"), s(&["a", ""]));
        assert_eq!(split("  "), s(&[]));
    }

    #[test]
    fn single_quotes_are_literal() {
        assert_eq!(split("x '$(id)' '`id`'"), s(&["x", "$(id)", "`id`"]));
    }

    #[test]
    fn rejects_compound_and_expansion() {
        for c in [
            "gh pr edit 1; rm -rf ~",
            "gh pr edit 1 && x",
            "gh pr edit 1 | tee",
            "gh pr edit 1 > out",
            "gh pr edit 1 --body-file - <<EOF",
            "gh pr edit $N",
            "gh pr edit \"$N\"",
            "gh pr edit \"$(id)\"",
            "gh pr edit `id`",
            "gh pr edit 1\ngh pr edit 2",
            "gh pr edit *",
            "gh pr edit 1 # c",
            "gh pr edit ~/x",
            "gh pr edit 'unterminated",
            "gh pr edit \"unterminated",
            "gh pr edit trailing\\",
        ] {
            assert_eq!(split(c), None, "{c:?}");
        }
    }
}
