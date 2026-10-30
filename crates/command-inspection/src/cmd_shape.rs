//! コマンドの「形」の分類(閉じた語だけ。本文も長さも残さない)。
//!
//! `cmd-hash-log` が実行したコマンドのハッシュに添えて記録し、`session-retrospective` が
//! 「そのセッションで、その場のスクリプトを書いたか」の痕跡として使う(ADR-1276)。
//! ハッシュだけでは「同じコマンドを 2 回実行した」としか分からず、`cargo test` の再実行と
//! `python3 -c '…'` の書き直しを区別できないので、本文を残さずに形だけを分類する。
//!
//! 形の id は `config/agents/codify-signals/vocabulary.json` の `shapes` が正本で、
//! [`Shape::ALL`] と一致することを `cmd-hash-log` のテストが固定する。コードの拡張子の一覧も
//! 語彙が正本なので、[`classify`] は引数で受ける(ここに複写しない)。
//!
//! 数えないもの: `git commit -m "$(cat <<'EOF' …)"` や `gh pr create --body-file` のような、
//! 文章を流し込むだけのヒアドキュメント。PR を作るセッションなら必ず出るので、数えると常に発火する。

use crate::shell::{is_sep, split_heredoc, tokenize};

/// コマンドの形。1 つのコマンドが複数の形を持ちうる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shape {
    /// インタプリタに流すヒアドキュメント(`python3 - <<EOF`)。
    HeredocInterp,
    /// コードの拡張子のファイルへ書くヒアドキュメント(`cat > x.py <<EOF`)。
    HeredocScript,
    /// インタプリタのインライン実行(`python3 -c`・`node -e`・`bash -c`)。
    InlineInterp,
    /// 80 文字以上の jq・awk のプログラム。
    LongFilter,
    /// 単独の `|` が 2 個以上(3 段以上のパイプライン)。
    Pipeline,
}

impl Shape {
    pub const ALL: [Shape; 5] = [
        Shape::HeredocInterp,
        Shape::HeredocScript,
        Shape::InlineInterp,
        Shape::LongFilter,
        Shape::Pipeline,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Shape::HeredocInterp => "heredoc-interp",
            Shape::HeredocScript => "heredoc-script",
            Shape::InlineInterp => "inline-interp",
            Shape::LongFilter => "long-filter",
            Shape::Pipeline => "pipeline",
        }
    }
}

/// jq・awk のプログラムとみなす最小の文字数。典型的な select・map の 1 行は 20〜50 文字で、
/// 80 文字以上は分岐を持つ(その場で書いたプログラム)ことが多い。実測できない(本文を残さない)
/// ので、誤読は `misread-signal` の却下で較正する。
pub const LONG_FILTER_MIN_CHARS: usize = 80;

const INTERPRETERS: &[&str] = &[
    "python", "python3", "node", "deno", "perl", "ruby", "bash", "sh", "zsh",
];
const INLINE_FLAGS: &[&str] = &["-c", "-e", "-E", "--eval"];
const FILTER_PROGRAMS: &[&str] = &["jq", "jaq", "gojq", "awk", "gawk", "mawk"];

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// `python3.12` も `python3` として扱う。
fn is_interpreter(word: &str) -> bool {
    let w = basename(word);
    INTERPRETERS.contains(&w)
        || w.strip_prefix("python")
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
}

fn has_code_ext(path: &str, code_exts: &[&str]) -> bool {
    basename(path)
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && code_exts.contains(&ext))
}

/// 区切りトークンの直後(と先頭)の語を「コマンド位置」として、各単純コマンドの語の並びに分ける。
fn simple_commands(tokens: &[String]) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = vec![Vec::new()];
    for t in tokens {
        if is_sep(t) {
            if !out.last().is_some_and(Vec::is_empty) {
                out.push(Vec::new());
            }
        } else if let Some(cur) = out.last_mut() {
            cur.push(t);
        }
    }
    out.retain(|c| !c.is_empty());
    out
}

/// 引用符の外にある単独の `|`(`||` ではない)の数の、文ごとの最大。
/// 文は `;`・`&`・改行・`||` で区切る。
fn max_pipes_in_a_statement(cmd: &str) -> usize {
    let b = cmd.as_bytes();
    let (mut sq, mut dq) = (false, false);
    let (mut count, mut best) = (0usize, 0usize);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if sq {
            sq = c != b'\'';
        } else if dq {
            if c == b'\\' {
                i += 1;
            } else {
                dq = c != b'"';
            }
        } else {
            match c {
                b'\'' => sq = true,
                b'"' => dq = true,
                b'\\' => i += 1,
                b'|' if b.get(i + 1) == Some(&b'|') => {
                    best = best.max(count);
                    count = 0;
                    i += 1;
                }
                b'|' => count += 1,
                b';' | b'&' | b'\n' => {
                    best = best.max(count);
                    count = 0;
                }
                _ => {}
            }
        }
        i += 1;
    }
    best.max(count)
}

/// コマンドの形を分類する。`code_exts` はコードとみなすファイルの拡張子(語彙の
/// `traces.scratch_code_ext`)。結果は [`Shape`] の順に並べ、重複しない。
pub fn classify(cmd: &str, code_exts: &[&str]) -> Vec<Shape> {
    let split = split_heredoc(cmd);
    let has_body = !split.bodies.is_empty();
    let mut shapes = Vec::new();

    if let Some(tokens) = tokenize(&split.command) {
        let commands = simple_commands(&tokens);
        if has_body
            && commands
                .iter()
                .any(|c| c.first().is_some_and(|w| is_interpreter(w)))
        {
            shapes.push(Shape::HeredocInterp);
        }
        if has_body && writes_code_file(&tokens, code_exts) {
            shapes.push(Shape::HeredocScript);
        }
        if commands.iter().any(|c| {
            c.first().is_some_and(|w| is_interpreter(w))
                && c.iter().skip(1).any(|a| INLINE_FLAGS.contains(a))
        }) {
            shapes.push(Shape::InlineInterp);
        }
        if commands.iter().any(|c| {
            c.first()
                .is_some_and(|w| FILTER_PROGRAMS.contains(&basename(w)))
                && c.iter().skip(1).any(|a| {
                    !a.starts_with('-')
                        && !a.starts_with('/')
                        && a.chars().count() >= LONG_FILTER_MIN_CHARS
                })
        }) {
            shapes.push(Shape::LongFilter);
        }
    }
    if max_pipes_in_a_statement(&split.command) >= 2 {
        shapes.push(Shape::Pipeline);
    }
    shapes.sort();
    shapes.dedup();
    shapes
}

/// `> x.py`・`>>x.py`・`tee x.py` のように、コードの拡張子のファイルへ書く。
fn writes_code_file(tokens: &[String], code_exts: &[&str]) -> bool {
    for (i, t) in tokens.iter().enumerate() {
        if t.starts_with('>') {
            let target = t.trim_start_matches('>');
            let target = if target.is_empty() {
                tokens.get(i + 1).map(String::as_str).unwrap_or("")
            } else {
                target
            };
            if has_code_ext(target, code_exts) {
                return true;
            }
        }
        if basename(t) == "tee" && (i == 0 || is_sep(&tokens[i - 1])) {
            let target = tokens[i + 1..]
                .iter()
                .take_while(|a| !is_sep(a))
                .find(|a| !a.starts_with('-'));
            if target.is_some_and(|a| has_code_ext(a, code_exts)) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTS: &[&str] = &["py", "sh", "js", "jq", "awk"];

    fn ids(cmd: &str) -> Vec<&'static str> {
        classify(cmd, EXTS).into_iter().map(Shape::id).collect()
    }

    #[test]
    fn a_commit_message_heredoc_is_not_a_shape() {
        let cmd = "git commit -m \"$(cat <<'EOF'\nfeat: x\n\nbody\nEOF\n)\"";
        assert_eq!(ids(cmd), Vec::<&str>::new());
        let pr = "gh pr create --title t --body \"$(cat <<'EOF'\n## 課題\nEOF\n)\"";
        assert_eq!(ids(pr), Vec::<&str>::new());
    }

    #[test]
    fn a_heredoc_fed_to_an_interpreter_is_a_shape() {
        assert_eq!(
            ids("python3 - <<'EOF'\nprint(1)\nEOF\n"),
            ["heredoc-interp"]
        );
        assert_eq!(
            ids("/usr/bin/python3.12 - <<EOF\nx\nEOF"),
            ["heredoc-interp"]
        );
        assert_eq!(ids("bash <<EOF\nls\nEOF"), ["heredoc-interp"]);
        // 文章を cat に流すだけなら数えない。
        assert_eq!(ids("cat <<EOF\nhello\nEOF"), Vec::<&str>::new());
    }

    #[test]
    fn a_heredoc_written_to_a_code_file_is_a_shape() {
        assert_eq!(ids("cat > x.py <<'EOF'\nprint(1)\nEOF"), ["heredoc-script"]);
        assert_eq!(ids("cat >x.sh <<EOF\nls\nEOF"), ["heredoc-script"]);
        assert_eq!(ids("tee run.js <<EOF\nx\nEOF"), ["heredoc-script"]);
        // 拡張子がコードでなければ文章。
        assert_eq!(ids("cat > body.md <<EOF\ntext\nEOF"), Vec::<&str>::new());
        // heredoc が無い単なるリダイレクトは対象外。
        assert_eq!(ids("echo hi > x.py"), Vec::<&str>::new());
    }

    #[test]
    fn inline_interpreters_are_a_shape() {
        assert_eq!(ids("python3 -c 'print(1)'"), ["inline-interp"]);
        assert_eq!(ids("node -e \"console.log(1)\""), ["inline-interp"]);
        assert_eq!(ids("cd x && perl -E 'say 1'"), ["inline-interp"]);
        assert_eq!(ids("python3 script.py"), Vec::<&str>::new());
        // インタプリタでないコマンドの -c は対象外。
        assert_eq!(ids("git -c core.x=1 status"), Vec::<&str>::new());
        assert_eq!(ids("grep -c foo file"), Vec::<&str>::new());
    }

    #[test]
    fn long_filter_needs_eighty_chars() {
        let short = format!("jq '{}' f.json", "a".repeat(79));
        let long = format!("jq '{}' f.json", "a".repeat(80));
        assert_eq!(ids(&short), Vec::<&str>::new());
        assert_eq!(ids(&long), ["long-filter"]);
        let awk = format!("awk '{}' f", "b".repeat(80));
        assert_eq!(ids(&awk), ["long-filter"]);
        // フラグや絶対パスは数えない。
        let path = format!("jq -f /{} f.json", "a".repeat(90));
        assert_eq!(ids(&path), Vec::<&str>::new());
    }

    #[test]
    fn a_pipeline_needs_two_bare_pipes() {
        assert_eq!(ids("a | b"), Vec::<&str>::new());
        assert_eq!(ids("a | b | c"), ["pipeline"]);
        // `||` は区切りで、パイプではない。
        assert_eq!(ids("a || b || c"), Vec::<&str>::new());
        assert_eq!(ids("a | b || c | d"), Vec::<&str>::new());
        // 引用符の中の `|` は数えない(jq のフィルタなど)。
        assert_eq!(ids("jq '.a | .b | .c' f"), Vec::<&str>::new());
        // 文が違えば別の数え方。
        assert_eq!(ids("a | b; c | d"), Vec::<&str>::new());
    }

    #[test]
    fn several_shapes_can_apply_and_are_sorted() {
        let cmd = "python3 -c 'print(1)' | sort | uniq";
        assert_eq!(ids(cmd), ["inline-interp", "pipeline"]);
    }

    #[test]
    fn an_unbalanced_quote_does_not_panic() {
        assert_eq!(ids("echo 'oops"), Vec::<&str>::new());
        assert_eq!(ids(""), Vec::<&str>::new());
    }

    #[test]
    fn all_ids_are_unique() {
        let mut v: Vec<&str> = Shape::ALL.iter().map(|s| s.id()).collect();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), Shape::ALL.len());
    }
}
