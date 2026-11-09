//! hook の入出力の不変条件: 入口(標準入力の読みと返答の形)は `agent-session` が持つ。
//!
//! - **返答の形**: hook の返答の封筒(`hookSpecificOutput`)を書く文字列は、`agent-session`
//!   (`Reply::render`)だけが持つ。ほかの crate の `src/` は、封筒の文字列を書かない。
//!   例外は [`ENVELOPE_EXCEPTIONS`](返答を書くのでなく、外から来た JSON を読む・読み替える crate)。
//! - **標準入力の読み**: hook の bin(`src/main.rs`・`src/entry.rs`・`src/bin/*.rs`)は、標準入力を
//!   `std::io::stdin()` で直接読まず、`agent_session::entry::read_stdin` を呼ぶ。端末かどうかを
//!   調べるだけの行(`is_terminal`)は読みではない。例外は [`STDIN_EXCEPTIONS`](hook でない CLI と、
//!   遅延して読む 1 つ)。
//!
//! - **Stop の block と再入**: Stop の block(`"decision": "block"`)を手で書く文字列と、`stop_hook_active` を読む
//!   文字列は、`agent-session`(`Reply::render` と `StopInput`)だけが持つ。ほかの crate の `src/` は、
//!   `StopOutcome` を返し、再入は `Reentry` を宣言する。例外は無い。
//!
//! 検査は、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を読まない。
//! 例外の一覧は、いずれも**恒久**の例外である。例外のファイルが、もう違反の形を持たないときは、
//! 一覧に残さず消す(古い例外は、それ自体が違反になる)。

use std::fs;
use std::path::Path;

use crate::manifest::Violation;

/// 返答の封筒を書く文字列。
const ENVELOPE_KEY: &str = "hookSpecificOutput";

/// 標準入力の直読み。
const STDIN_CALL: &str = "io::stdin()";

/// 端末かどうかを調べるだけの行は、読みではない。
const TERMINAL_CHECK: &str = "is_terminal";

/// 封筒の文字列を持ってよい crate のソース(リポジトリの根からの相対パス)と、その理由。
/// どれも、返答を**書く**のでなく、外から来た JSON を読む・読み替える。
pub const ENVELOPE_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "crates/codex-plan-gate/src/main.rs",
        "ExitPlanMode の gate(子のプロセス)が返した JSON から、拒否の理由を読む。自分の返答は StopOutcome で返す",
    ),
    (
        "crates/copilot-plan-review/src/lib.rs",
        "別の gate が返した JSON から、拒否かどうかを読む。自分の返答は Reply で書く",
    ),
    (
        "crates/hook-router/src/codex.rs",
        "Codex が実行できない ask を deny に書き換える。手元の hook(外から来る JSON)を読み替えるだけで、自前の返答の形ではない",
    ),
];

/// 標準入力を直接読んでよい bin のソースと、その理由。
pub const STDIN_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "crates/adr-number/src/main.rs",
        "プロジェクト dir が環境変数で分かるときは標準入力を読まない。最安の早期 exit を壊さないため、遅延して読む",
    ),
    (
        "crates/ja-polish/src/main.rs",
        "テキストを標準入力で受ける整形 CLI(`--stdin`、`-`)。読めない失敗を利用者に返す。hook の枝(`stop-gate`)は read_stdin を呼ぶ",
    ),
    (
        "crates/claude-statusline/src/main.rs",
        "statusLine の入力で、hook ではない",
    ),
    (
        "crates/codex-plan/src/main.rs",
        "対話の入力(利用者への質問の答え)で、hook ではない",
    ),
    (
        "crates/pr-title-check/src/main.rs",
        "CI 用 CLI のタイトルの入力で、hook ではない。依存ゼロに保つ crate",
    ),
];

/// 検査する Rust のソース 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// リポジトリの根からの相対パス(`/` 区切り)。
    pub path: String,
    pub text: String,
}

fn violation(path: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: path.to_string(),
        detail: detail.into(),
    }
}

/// 検査の対象になる行(1 から数えた行番号と、行)。コメントの行と、テストの節(`#[cfg(test)]` の行から後)は読まない。
pub(crate) fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .take_while(|(_, l)| *l != "#[cfg(test)]")
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
}

/// 入口の持ち主(`agent-session`)と、この検査自身(検査の対象の文字列を定数に持つ)は、検査しない。
pub(crate) fn is_owner(path: &str) -> bool {
    path.starts_with("crates/agent-session/") || path.starts_with("crates/repo-structure-check/")
}

fn exception(list: &[(&str, &str)], path: &str) -> bool {
    list.iter().any(|(p, _)| *p == path)
}

/// 返答の封筒の文字列が、`agent-session` と例外のほかに無いことを検査する。
pub fn check_envelope_literals(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !is_owner(&s.path)) {
        let hit = code_lines(&s.text).find(|(_, l)| l.contains(ENVELOPE_KEY));
        match (hit, exception(ENVELOPE_EXCEPTIONS, &s.path)) {
            (Some((n, _)), false) => out.push(violation(
                &s.path,
                format!(
                    "{n} 行目に `{ENVELOPE_KEY}` の文字列がある。返答は agent_session::Reply で作る(封筒の形は Reply::render が持つ)"
                ),
            )),
            (None, true) => out.push(violation(
                &s.path,
                format!(
                    "ENVELOPE_EXCEPTIONS にあるが、`{ENVELOPE_KEY}` の文字列がもう無い。例外から消す"
                ),
            )),
            _ => {}
        }
    }
    for (path, _) in ENVELOPE_EXCEPTIONS {
        if !sources.iter().any(|s| s.path == *path) {
            out.push(violation(
                path,
                "ENVELOPE_EXCEPTIONS にあるが、ファイルが無い。例外から消す",
            ));
        }
    }
    out
}

/// Stop の block を手で書いた行か。`\"decision\"` のようにエスケープした形も、バックスラッシュを除いて見る。
fn writes_stop_block(line: &str) -> bool {
    let plain = line.replace('\\', "");
    plain.contains("\"decision\"") && plain.contains("\"block\"")
}

/// `stop_hook_active` を読む文字列。
const REENTRY_KEY: &str = "stop_hook_active";

/// Stop の block を手で書く文字列と、`stop_hook_active` の読みが、`agent-session` のほかに無いことを検査する。
pub fn check_stop_literals(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !is_owner(&s.path)) {
        if let Some((n, _)) = code_lines(&s.text).find(|(_, l)| writes_stop_block(l)) {
            out.push(violation(
                &s.path,
                format!(
                    "{n} 行目で Stop の block(`decision` と `block`)を手で書いている。StopOutcome を返す(描画は Reply::render が持つ)"
                ),
            ));
        }
        if let Some((n, _)) = code_lines(&s.text).find(|(_, l)| l.contains(REENTRY_KEY)) {
            out.push(violation(
                &s.path,
                format!(
                    "{n} 行目で `{REENTRY_KEY}` を読んでいる。再入は Reentry を宣言して StopInput::permits に問う"
                ),
            ));
        }
    }
    out
}

/// hook の bin のソースか(`src/main.rs`・`src/entry.rs`・`src/bin/*.rs`)。
fn is_bin_source(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("crates/") else {
        return false;
    };
    let Some((_, within)) = rest.split_once('/') else {
        return false;
    };
    within == "src/main.rs"
        || within == "src/entry.rs"
        || (within.starts_with("src/bin/") && !within["src/bin/".len()..].contains('/'))
}

fn has_direct_stdin_read(text: &str) -> Option<usize> {
    code_lines(text)
        .find(|(_, l)| l.contains(STDIN_CALL) && !l.contains(TERMINAL_CHECK))
        .map(|(n, _)| n)
}

/// hook の bin が、標準入力を直接読んでいないことを検査する。
pub fn check_stdin_reads(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources
        .iter()
        .filter(|s| is_bin_source(&s.path) && !is_owner(&s.path))
    {
        match (
            has_direct_stdin_read(&s.text),
            exception(STDIN_EXCEPTIONS, &s.path),
        ) {
            (Some(n), false) => out.push(violation(
                &s.path,
                format!(
                    "{n} 行目で標準入力を直接読んでいる。agent_session::entry::read_stdin を呼ぶ(hook でない CLI は STDIN_EXCEPTIONS に理由つきで足す)"
                ),
            )),
            (None, true) => out.push(violation(
                &s.path,
                "STDIN_EXCEPTIONS にあるが、標準入力の直読みがもう無い。例外から消す",
            )),
            _ => {}
        }
    }
    for (path, _) in STDIN_EXCEPTIONS {
        if !sources.iter().any(|s| s.path == *path) {
            out.push(violation(
                path,
                "STDIN_EXCEPTIONS にあるが、ファイルが無い。例外から消す",
            ));
        }
    }
    out
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<Source>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{} を読めない: {e}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            collect(&p, root, out)?;
        } else if p.extension().is_some_and(|x| x == "rs") {
            let text =
                fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))?;
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(Source { path: rel, text });
        }
    }
    Ok(())
}

/// `crates/*/src/**/*.rs` を全部読む。
pub fn read_sources(root: &Path) -> Result<Vec<Source>, String> {
    let crates = root.join("crates");
    let mut dirs: Vec<_> = fs::read_dir(&crates)
        .map_err(|e| format!("{} を読めない: {e}", crates.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src").is_dir())
        .collect();
    dirs.sort();
    let mut out = Vec::new();
    for d in dirs {
        collect(&d.join("src"), root, &mut out)?;
    }
    Ok(out)
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let sources = read_sources(root)?;
    let mut out = check_envelope_literals(&sources);
    out.extend(check_stdin_reads(&sources));
    out.extend(check_stop_literals(&sources));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(path: &str, text: &str) -> Source {
        Source {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    /// 例外のファイルを、違反の形で足した一覧(例外の「もう無い」「ファイルが無い」を出さないため)。
    fn with_exceptions(mut v: Vec<Source>, list: &[(&str, &str)], text: &str) -> Vec<Source> {
        for (p, _) in list {
            v.push(src(p, text));
        }
        v
    }

    fn envelope_exceptions_present(v: Vec<Source>) -> Vec<Source> {
        with_exceptions(v, ENVELOPE_EXCEPTIONS, "let a = \"hookSpecificOutput\";\n")
    }

    fn stdin_exceptions_present(v: Vec<Source>) -> Vec<Source> {
        with_exceptions(v, STDIN_EXCEPTIONS, "let _ = std::io::stdin();\n")
    }

    // ---- 返答の封筒: 正例 ----

    #[test]
    fn agent_session_may_write_the_envelope() {
        let v = envelope_exceptions_present(vec![src(
            "crates/agent-session/src/reply.rs",
            "let k = \"hookSpecificOutput\";\n",
        )]);
        assert_eq!(check_envelope_literals(&v), vec![]);
    }

    #[test]
    fn comments_and_the_test_section_are_not_read() {
        let text = "//! `hookSpecificOutput` の形は Reply が持つ\n\
                    fn f() {}\n\
                    // let a = \"hookSpecificOutput\";\n\
                    #[cfg(test)]\n\
                    mod tests { const X: &str = \"hookSpecificOutput\"; }\n";
        let v = envelope_exceptions_present(vec![src("crates/x/src/lib.rs", text)]);
        assert_eq!(check_envelope_literals(&v), vec![]);
    }

    // ---- 返答の封筒: 負例(検査が実際に落ちること) ----

    #[test]
    fn a_handwritten_envelope_is_a_violation() {
        let v = envelope_exceptions_present(vec![src(
            "crates/x/src/main.rs",
            "fn main() {\n    println!(\"{{\\\"hookSpecificOutput\\\": {{}}}}\");\n}\n",
        )]);
        let vs = check_envelope_literals(&v);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert_eq!(vs[0].subject, "crates/x/src/main.rs");
        assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
    }

    #[test]
    fn a_stale_envelope_exception_is_a_violation() {
        let mut v = envelope_exceptions_present(vec![]);
        v[0] = src(ENVELOPE_EXCEPTIONS[0].0, "fn main() {}\n");
        let vs = check_envelope_literals(&v);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert!(vs[0].detail.contains("例外から消す"));
    }

    #[test]
    fn a_missing_exception_file_is_a_violation() {
        let vs = check_envelope_literals(&[]);
        assert_eq!(vs.len(), ENVELOPE_EXCEPTIONS.len());
    }

    // ---- 標準入力: 正例 ----

    #[test]
    fn read_stdin_and_terminal_checks_pass() {
        let text = "let raw = agent_session::entry::read_stdin();\n\
                    if !std::io::stdin().is_terminal() {}\n";
        let v = stdin_exceptions_present(vec![src("crates/x/src/main.rs", text)]);
        assert_eq!(check_stdin_reads(&v), vec![]);
    }

    #[test]
    fn library_sources_are_not_bin_sources() {
        let v = stdin_exceptions_present(vec![src(
            "crates/x/src/lib.rs",
            "let _ = std::io::stdin();\n",
        )]);
        assert_eq!(check_stdin_reads(&v), vec![]);
    }

    // ---- 標準入力: 負例 ----

    #[test]
    fn a_direct_read_in_a_bin_is_a_violation() {
        for path in [
            "crates/x/src/main.rs",
            "crates/x/src/entry.rs",
            "crates/x/src/bin/y.rs",
        ] {
            let v = stdin_exceptions_present(vec![src(
                path,
                "fn main() {\n    let _ = std::io::stdin().read_to_end(&mut b);\n}\n",
            )]);
            let vs = check_stdin_reads(&v);
            assert_eq!(vs.len(), 1, "{path}: {vs:?}");
            assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
        }
    }

    #[test]
    fn a_stale_stdin_exception_is_a_violation() {
        let mut v = stdin_exceptions_present(vec![]);
        v[0] = src(STDIN_EXCEPTIONS[0].0, "fn main() {}\n");
        let vs = check_stdin_reads(&v);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert!(vs[0].detail.contains("例外から消す"));
    }

    // ---- Stop の block と再入: 正例 ----

    #[test]
    fn stop_literals_in_agent_session_comments_and_tests_are_fine() {
        let text = "//! `{\"decision\": \"block\"}` と stop_hook_active の説明\n\
                    fn f() {}\n\
                    #[cfg(test)]\n\
                    mod tests { const X: &str = \"{\\\"decision\\\":\\\"block\\\"} stop_hook_active\"; }\n";
        let v = vec![
            src("crates/x/src/lib.rs", text),
            src(
                "crates/agent-session/src/reply.rs",
                "top.push((\"decision\", J::str(\"block\")));\nlet k = \"stop_hook_active\";\n",
            ),
        ];
        assert_eq!(check_stop_literals(&v), vec![]);
    }

    // ---- Stop の block と再入: 負例(検査が実際に落ちること) ----

    #[test]
    fn a_handwritten_stop_block_is_a_violation() {
        for line in [
            "let out = json!({\"decision\": \"block\", \"reason\": r});",
            "println!(\"{{\\\"decision\\\":\\\"block\\\"}}\");",
        ] {
            let v = vec![src(
                "crates/x/src/main.rs",
                &format!("fn main() {{\n    {line}\n}}\n"),
            )];
            let vs = check_stop_literals(&v);
            assert_eq!(vs.len(), 1, "{line}: {vs:?}");
            assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
        }
    }

    #[test]
    fn reading_stop_hook_active_outside_agent_session_is_a_violation() {
        let v = vec![src(
            "crates/x/src/lib.rs",
            "fn f(v: &Value) -> bool {\n    v.get(\"stop_hook_active\").is_some()\n}\n",
        )];
        let vs = check_stop_literals(&v);
        assert_eq!(vs.len(), 1, "{vs:?}");
        assert!(vs[0].detail.contains("2 行目"), "{}", vs[0].detail);
    }

    #[test]
    fn decision_without_block_is_not_a_stop_block() {
        let v = vec![src(
            "crates/x/src/lib.rs",
            "let d = v[\"decision\"].as_str(); // block ではない\n",
        )];
        assert_eq!(check_stop_literals(&v), vec![]);
    }

    #[test]
    fn nested_bin_directories_are_not_bin_sources() {
        assert!(is_bin_source("crates/x/src/bin/y.rs"));
        assert!(!is_bin_source("crates/x/src/bin/y/mod.rs"));
        assert!(!is_bin_source("crates/x/src/lib.rs"));
        assert!(!is_bin_source("crates/x/tests/main.rs"));
    }
}
