//! heredoc の分離とクォート解釈トークナイザ(attribution-guard.sh の
//! `split_heredoc` / `tokenize` / `is_sep`)。
//!
//! `hook_io::shell::split` とは別物: あちらは「安全に静的解釈できない入力は
//! `None`」の最小実装で、`;` `|` `$(` を含む入力を丸ごと拒否する。こちらは
//! 複合コマンドをそのまま受け、区切りを独立トークンとして残して「コマンド
//! 位置」を判定するための材料にする。
//!
//! bash 版は `LC_ALL=C` のバイト単位で走査していた。ここでもバイト単位で
//! 走査する — 区切り・クォート・空白はすべて ASCII で、UTF-8 の継続バイト
//! (0x80-0xBF)と衝突しないので、マルチバイト文字は 1 トークンの中で
//! 壊れずに残る。

/// コマンドを区切るトークン(この直後が「コマンド位置」になる)。
///
/// 対象コマンドの検出をコマンド位置に限るための材料。コマンド文字列全体を
/// 正規表現で見る実装は、コミットメッセージや docs に投稿コマンドの例を
/// 書いただけで発火する — bash 版 attribution-guard.sh 自身を commit しよう
/// として実際に踏んだ。
pub const CMD_SEPS: &[u8] = b";|&()\n";

/// POSIX `[[:space:]]`(C ロケール): 空白・`\t`・`\n`・`\v`・`\f`・`\r`。
pub fn is_posix_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// [`split_heredoc`] の結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeredocSplit {
    /// heredoc 本体を除いたコマンド文字列(bash の `CMD_NOHD`)。各行の末尾に
    /// `\n` が付く。コマンド位置の判定に使う。
    pub command: String,
    /// heredoc 本体の連結(bash の `HD_BODIES`)。各行の末尾に `\n` が付く。
    /// `--body "$(cat <<'TAG' … TAG)"` の本文候補として使う。
    pub bodies: String,
}

/// heredoc 本体を分離する(bash の `split_heredoc`)。
///
/// 本体を除かずにコマンド位置を判定すると、heredoc で流し込む文章の中に
/// 書いた投稿コマンドの例がコマンド位置に見えてしまう(本体の各行は改行の
/// 直後に来るため)。bash 版自身のコミットメッセージで実際に踏んだ。
///
/// 開始行の検出は bash の `\<\<-?[[:space:]]*['"]?([A-Za-z_][A-Za-z0-9_]*)`
/// と同じ(行内の最左一致)。bash 版のコメントは「`<<<`(herestring)は
/// タグの正規表現にマッチせず除外される」と書くが、実際には `<<<EOF` や
/// `<<< "x"` も 2 文字目からの `<<` で一致し heredoc として扱われる —
/// 移植では bash の実挙動を保つ(docs/claude/guard-core.md「bash の挙動が
/// 疑わしい箇所」)。
pub fn split_heredoc(cmd: &str) -> HeredocSplit {
    let mut out = HeredocSplit::default();
    let mut tag: Option<String> = None;
    // bash は `while read -r line … done <<< "$1"` で読む。herestring が末尾に
    // `\n` を足すので、読まれる行はちょうど `cmd.split('\n')` になる。
    for line in cmd.split('\n') {
        if let Some(t) = &tag {
            if trim_posix_space(line) == t {
                tag = None;
            } else {
                out.bodies.push_str(line);
                out.bodies.push('\n');
            }
            continue;
        }
        out.command.push_str(line);
        out.command.push('\n');
        tag = heredoc_tag(line);
    }
    out
}

fn trim_posix_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_ascii() && is_posix_space(c as u8))
}

/// 行内の最左の `<<-?[[:space:]]*['"]?TAG` の TAG。
fn heredoc_tag(line: &str) -> Option<String> {
    let b = line.as_bytes();
    let n = b.len();
    for p in 0..n.saturating_sub(1) {
        if b[p] != b'<' || b[p + 1] != b'<' {
            continue;
        }
        let mut i = p + 2;
        if i < n && b[i] == b'-' {
            i += 1;
        }
        // `[[:space:]]` は read が切り出した 1 行の中なので `\n` は来ない。
        while i < n && is_posix_space(b[i]) {
            i += 1;
        }
        if i < n && (b[i] == b'\'' || b[i] == b'"') {
            i += 1;
        }
        if i < n && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
            let start = i;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            return Some(line[start..i].to_string());
        }
    }
    None
}

/// クォートを解釈してトークンに分割する(bash の `tokenize`)。区切り
/// ([`CMD_SEPS`])は独立トークンとして残す。unmatched quote またはトークン
/// 0 件なら `None`。
///
/// xargs も `read -r -a` も使えない理由(bash 版の実測):
/// - GNU xargs はクォート内の改行を扱えず "unmatched single quote" で落ちる。
///   改行を含む本文は長文コメントの典型。
/// - 素朴な空白分割では Markdown 本文の `- 箇条書き` で本文が途中で切れ、
///   末尾のフッターを常に見失う。
///
/// クォートの痕跡は残らない(`';'` も `;` トークンになり [`is_sep`] が真に
/// なる)— bash 版と同じ。
pub fn tokenize(s: &str) -> Option<Vec<String>> {
    #[derive(PartialEq)]
    enum St {
        None,
        Sq,
        Dq,
    }
    let b = s.as_bytes();
    let n = b.len();
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut started = false;
    let mut st = St::None;
    let mut i = 0;
    while i < n {
        let c = b[i];
        match st {
            St::None => match c {
                b' ' | b'\t' | b'\r' => {
                    if started {
                        out.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                b';' | b'|' | b'&' | b'(' | b')' | b'\n' => {
                    if started {
                        out.push(std::mem::take(&mut cur));
                        started = false;
                    }
                    out.push(vec![c]);
                }
                b'\'' => {
                    st = St::Sq;
                    started = true;
                }
                b'"' => {
                    st = St::Dq;
                    started = true;
                }
                b'\\' => {
                    // bash の `${s:i:1}` は末尾を越えると空文字になる。
                    i += 1;
                    if i < n {
                        cur.push(b[i]);
                    }
                    started = true;
                }
                _ => {
                    cur.push(c);
                    started = true;
                }
            },
            St::Sq => {
                if c == b'\'' {
                    st = St::None;
                } else {
                    cur.push(c);
                }
            }
            St::Dq => match c {
                b'"' => st = St::None,
                b'\\' => {
                    i += 1;
                    if i < n {
                        cur.push(b[i]);
                    }
                }
                _ => cur.push(c),
            },
        }
        i += 1;
    }
    if st != St::None {
        return None;
    }
    if started {
        out.push(cur);
    }
    if out.is_empty() {
        return None;
    }
    Some(
        out.into_iter()
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .collect(),
    )
}

/// コマンドの区切りトークンなら真(bash の `is_sep`: 1 バイトで
/// [`CMD_SEPS`] に含まれる)。
pub fn is_sep(tok: &str) -> bool {
    tok.len() == 1 && CMD_SEPS.contains(&tok.as_bytes()[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<String> {
        tokenize(s).unwrap()
    }

    #[test]
    fn tokenize_quotes_and_seps() {
        assert_eq!(
            t("gh pr comment 1 --body 'a; b' && echo \"x\\\"y\""),
            ["gh", "pr", "comment", "1", "--body", "a; b", "&", "&", "echo", "x\"y"]
        );
        assert_eq!(t("a\nb"), ["a", "\n", "b"]);
        assert_eq!(t("''"), [""]);
        assert_eq!(t("a\\ b"), ["a b"]);
        // 末尾の `\` は空文字を足すだけ(bash の ${s:n:1})
        assert_eq!(t("a\\"), ["a"]);
        assert_eq!(t("'本文 - 箇条書き'"), ["本文 - 箇条書き"]);
    }

    #[test]
    fn tokenize_failures() {
        assert_eq!(tokenize("echo 'x"), None);
        assert_eq!(tokenize("echo \"x"), None);
        assert_eq!(tokenize("   "), None);
        assert_eq!(tokenize(""), None);
    }

    #[test]
    fn is_sep_one_byte_only() {
        for s in [";", "|", "&", "(", ")", "\n"] {
            assert!(is_sep(s));
        }
        assert!(!is_sep("&&"));
        assert!(!is_sep(""));
        assert!(!is_sep("x"));
    }

    #[test]
    fn heredoc_split() {
        let s = split_heredoc("cat <<'EOF' | gh x\nbody; 1\n  EOF  \nnext");
        assert_eq!(s.command, "cat <<'EOF' | gh x\nnext\n");
        assert_eq!(s.bodies, "body; 1\n");
        // 閉じタグが無ければ以降すべて本体
        let s = split_heredoc("a <<-TAG\nx\ny");
        assert_eq!(s.command, "a <<-TAG\n");
        assert_eq!(s.bodies, "x\ny\n");
        // 末尾改行は空行として 1 行増える(bash の herestring と同じ)
        assert_eq!(split_heredoc("a\n").command, "a\n\n");
        assert_eq!(split_heredoc("").command, "\n");
    }

    #[test]
    fn herestring_is_treated_as_heredoc_like_bash() {
        // bash の実挙動の固定(コメントの意図とは食い違う、モジュール doc 参照)
        let s = split_heredoc("cat <<<EOF\nfoo\nEOF");
        assert_eq!(s.command, "cat <<<EOF\n");
        assert_eq!(s.bodies, "foo\n");
        assert_eq!(heredoc_tag("cat <<< \"x\""), Some("x".into()));
        assert_eq!(heredoc_tag("cat << 5"), None);
    }
}
