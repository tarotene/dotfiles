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

/// [`split`] が判定できない入力の1文。`words` が `Some` なら [`split`] と
/// 同じ意味で解析済み(引用符解決後の語配列)。`None` はこの文自体が
/// 展開・リダイレクト・未終端引用符等で静的解析できないことを意味する —
/// 呼び出し側はこの文だけ判定不能(fail-closed 等)に倒し、他の文の判定は
/// 妨げない。
pub struct Segment {
    /// この文の生テキスト(引用符・演算子含む、前後の区切り文字は含まない)。
    /// `words` が `None` のとき、粗い(語境界のみの)判定にはこちらを使う。
    pub text: String,
    pub words: Option<Vec<String>>,
}

/// `cmd` を `;` `&&` `||` `|` `|&` `&` 改行/CR(クォート外でのみ区切りとして
/// 働く)で区切った文の列に分割する。区切り文字の集合は Claude Code 公式
/// permissions ドキュメント(<https://code.claude.com/docs/en/permissions.md>、
/// "Compound commands" 節、取得 2026-09-29)の分割規則と一致させている。
///
/// 各文は独立に[`split`]と同じ規則で語分割を試みる。展開・リダイレクト・
/// 未終端引用符等でその文だけ解析できなくても、それ以前・以後の文の解析は
/// 妨げない — [`split`]がこれらを検出すると入力全体を`None`にするのに対し、
/// この関数は判定できる範囲を最大化する側に倒す(#548: `pkexec` という語を
/// 含む複合コマンドが、地の文で言及しただけの無関係な文まで含めて丸ごと
/// fail-closed になっていた誤検知への対処)。常に非空の `Vec` を返す —
/// 全体が失敗することはない(1文の中で未終端引用符に達したときは、その文が
/// 残りの入力全体を吸収して終わる)。
pub fn split_segments(cmd: &str) -> Vec<Segment> {
    fn finalize(
        segments: &mut Vec<Segment>,
        raw: &mut String,
        words: &mut Vec<String>,
        cur: &mut String,
        in_word: &mut bool,
        poisoned: &mut bool,
    ) {
        if *in_word {
            words.push(std::mem::take(cur));
            *in_word = false;
        }
        // 常に words を取り出して空にする(poisoned でも!) — でないと
        // 汚染された語の残骸(空文字列 push 済み等)が次の文の words に
        // 混入する。
        let taken = std::mem::take(words);
        let w = if *poisoned { None } else { Some(taken) };
        segments.push(Segment {
            text: std::mem::take(raw),
            words: w,
        });
        *poisoned = false;
    }

    let mut segments = Vec::new();
    let mut raw = String::new();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut poisoned = false;

    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' | '\r' | ';' => {
                finalize(
                    &mut segments,
                    &mut raw,
                    &mut words,
                    &mut cur,
                    &mut in_word,
                    &mut poisoned,
                );
            }
            '&' => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                }
                finalize(
                    &mut segments,
                    &mut raw,
                    &mut words,
                    &mut cur,
                    &mut in_word,
                    &mut poisoned,
                );
            }
            '|' => {
                if chars.peek() == Some(&'|') || chars.peek() == Some(&'&') {
                    chars.next();
                }
                finalize(
                    &mut segments,
                    &mut raw,
                    &mut words,
                    &mut cur,
                    &mut in_word,
                    &mut poisoned,
                );
            }
            ' ' | '\t' => {
                raw.push(c);
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                raw.push(c);
                in_word = true;
                loop {
                    match chars.next() {
                        None => {
                            poisoned = true;
                            break;
                        }
                        Some('\'') => {
                            raw.push('\'');
                            break;
                        }
                        Some(ch) => {
                            raw.push(ch);
                            if !poisoned {
                                cur.push(ch);
                            }
                        }
                    }
                }
            }
            '"' => {
                raw.push(c);
                in_word = true;
                loop {
                    match chars.next() {
                        None => {
                            poisoned = true;
                            break;
                        }
                        Some('"') => {
                            raw.push('"');
                            break;
                        }
                        Some(ch @ ('$' | '`')) => {
                            raw.push(ch);
                            poisoned = true;
                        }
                        Some('\\') => {
                            raw.push('\\');
                            match chars.next() {
                                None => {
                                    poisoned = true;
                                    break;
                                }
                                Some(ch2 @ ('"' | '\\' | '$' | '`')) => {
                                    raw.push(ch2);
                                    if !poisoned {
                                        cur.push(ch2);
                                    }
                                }
                                Some('\n') => {
                                    raw.push('\n');
                                }
                                Some(ch2) => {
                                    raw.push(ch2);
                                    if !poisoned {
                                        cur.push('\\');
                                        cur.push(ch2);
                                    }
                                }
                            }
                        }
                        Some(ch) => {
                            raw.push(ch);
                            if !poisoned {
                                cur.push(ch);
                            }
                        }
                    }
                }
            }
            '\\' => {
                raw.push(c);
                in_word = true;
                match chars.next() {
                    None => poisoned = true,
                    Some('\n') => raw.push('\n'),
                    Some(ch) => {
                        raw.push(ch);
                        if !poisoned {
                            cur.push(ch);
                        }
                    }
                }
            }
            '$' | '`' | '(' | ')' | '*' | '?' | '[' | '{' | '}' | '<' | '>' => {
                raw.push(c);
                poisoned = true;
            }
            '#' | '~' if !in_word => {
                raw.push(c);
                poisoned = true;
            }
            ch => {
                raw.push(ch);
                in_word = true;
                if !poisoned {
                    cur.push(ch);
                }
            }
        }
    }
    finalize(
        &mut segments,
        &mut raw,
        &mut words,
        &mut cur,
        &mut in_word,
        &mut poisoned,
    );
    segments
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

#[cfg(test)]
mod split_segments_tests {
    use super::split_segments;

    fn words_of(cmd: &str) -> Vec<Option<Vec<String>>> {
        split_segments(cmd).into_iter().map(|s| s.words).collect()
    }

    fn s(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|x| x.to_string()).collect())
    }

    #[test]
    fn single_simple_command_is_one_segment() {
        assert_eq!(
            words_of("gh pr edit 12"),
            vec![s(&["gh", "pr", "edit", "12"])]
        );
    }

    #[test]
    fn semicolon_ampersand_pipe_split_into_independent_segments() {
        assert_eq!(
            words_of("echo a; echo b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
        assert_eq!(
            words_of("echo a && echo b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
        assert_eq!(
            words_of("echo a || echo b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
        assert_eq!(
            words_of("echo a | echo b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
        assert_eq!(
            words_of("echo a & echo b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
        assert_eq!(
            words_of("echo a\necho b"),
            vec![s(&["echo", "a"]), s(&["echo", "b"])]
        );
    }

    #[test]
    fn quoted_separators_do_not_split() {
        // #548 の再現条件そのもの: 引用符内の `;` は区切りとして働かない。
        assert_eq!(
            words_of("echo 'a; b && c | d'"),
            vec![s(&["echo", "a; b && c | d"])]
        );
    }

    #[test]
    fn one_unparseable_segment_does_not_poison_its_siblings() {
        // #548: 展開・リダイレクトを含む文が1つあっても、他の文は独立に
        // 解析できる(split() は全体を None にしていた)。
        let segs = words_of("echo before; echo \"$(id)\"; echo after");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], s(&["echo", "before"]));
        assert_eq!(segs[1], None);
        assert_eq!(segs[2], s(&["echo", "after"]));
    }

    #[test]
    fn redirection_poisons_only_its_own_segment() {
        let segs = words_of("echo hi > /tmp/x; echo bye");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], None);
        assert_eq!(segs[1], s(&["echo", "bye"]));
    }

    #[test]
    fn unterminated_quote_absorbs_the_rest_as_one_segment() {
        let segs = words_of("echo before; echo 'unterminated; echo after");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], s(&["echo", "before"]));
        assert_eq!(segs[1], None);
    }

    #[test]
    fn comment_and_tilde_poison_only_their_own_segment() {
        let segs = words_of("echo a # comment; echo b");
        assert_eq!(segs[0], None);
        let segs2 = words_of("echo a; echo ~/x; echo c");
        assert_eq!(segs2.len(), 3);
        assert_eq!(segs2[0], s(&["echo", "a"]));
        assert_eq!(segs2[1], None);
        assert_eq!(segs2[2], s(&["echo", "c"]));
    }

    #[test]
    fn text_preserves_the_original_substring_including_quotes() {
        let segs = split_segments("echo 'a b'; echo c");
        assert_eq!(segs[0].text, "echo 'a b'");
        assert_eq!(segs[1].text, " echo c");
    }
}
