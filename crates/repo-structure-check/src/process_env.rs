//! 環境変数・ホーム・XDG の基準ディレクトリの不変条件: 解決は `process-env` が持つ。
//!
//! - **XDG の基準ディレクトリ**: `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`XDG_CACHE_HOME`・`XDG_DATA_HOME` の名前の文字列は、
//!   `process-env`(`Xdg`)だけが持つ。ほかの crate の `src/` は、`process_env::Xdg::<種類>.resolve()` などで基準を
//!   解決する。環境変数が空のときを未設定として扱う規則(bash の `${VAR:-}`)が、約 20 の crate に手で複写され、
//!   複写ごとに空の扱いと `HOME` が無いときの扱いが違っていた。
//!   例外は [`XDG_EXCEPTIONS`](解決でなく、子プロセスへ引き渡す変数の名前の一覧)。
//! - **補助関数の複写**: `env_nonempty`・`non_empty_env`・`nonempty_env`・`xdg_dir`・`xdg_state_home`・`config_home`・
//!   `env_path`・`home` という名前の関数を、`process-env` の外に定義しない。非空の環境変数を読むのは
//!   `process_env::var_nonempty`、`HOME` は `process_env::home`。例外は [`HELPER_EXCEPTIONS`](同じ名前でも別の仕事)。
//!
//! 検査は、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を読まない。
//! 例外の一覧は、いずれも**恒久**の例外である。例外のファイルが、もう違反の形を持たないときは、
//! 一覧に残さず消す(古い例外は、それ自体が違反になる)。

use std::path::Path;

use crate::hook_io::{code_lines, read_sources, Source};
use crate::manifest::Violation;

/// 解決を `process-env` に集める XDG の変数名。
const XDG_NAMES: [&str; 4] = [
    "\"XDG_CONFIG_HOME\"",
    "\"XDG_STATE_HOME\"",
    "\"XDG_CACHE_HOME\"",
    "\"XDG_DATA_HOME\"",
];

/// 定義してはいけない関数(の見出し)。
const HELPER_FNS: [&str; 8] = [
    "fn env_nonempty(",
    "fn non_empty_env(",
    "fn nonempty_env(",
    "fn xdg_dir(",
    "fn xdg_state_home(",
    "fn config_home(",
    "fn env_path(",
    "fn home()",
];

/// 定義してはいけないクロージャ(の束縛)。
const HELPER_LETS: [&str; 3] = [
    "let env_nonempty =",
    "let non_empty_env =",
    "let nonempty_env =",
];

/// XDG の変数名の文字列を持ってよいソースと、その理由。解決ではない。
pub const XDG_EXCEPTIONS: &[(&str, &str)] = &[(
    "crates/local-state-backup/src/main.rs",
    "子プロセス(systemd の unit の行と rclone など)へ引き渡す環境変数の名前の一覧で、基準ディレクトリの解決ではない",
)];

/// 同じ名前の関数を持ってよいソースと、その理由。
pub const HELPER_EXCEPTIONS: &[(&str, &str)] = &[(
    "crates/backup-runtime/src/lib.rs",
    "`nonempty_env` は値を返さず、空なら失敗にする検証(Bitwarden の必須項目の確認)で、非空の値を読む仕事ではない",
)];

fn violation(path: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: path.to_string(),
        detail: detail.into(),
    }
}

/// 持ち主(`process-env`)と、この検査自身(検査の対象の文字列を定数に持つ)は、検査しない。
fn is_owner(path: &str) -> bool {
    path.starts_with("crates/process-env/") || path.starts_with("crates/repo-structure-check/")
}

fn exception(list: &[(&str, &str)], path: &str) -> bool {
    list.iter().any(|(p, _)| *p == path)
}

fn xdg_hit(text: &str) -> Option<usize> {
    code_lines(text)
        .find(|(_, l)| XDG_NAMES.iter().any(|n| l.contains(n)))
        .map(|(n, _)| n)
}

fn helper_hit(text: &str) -> Option<(usize, &'static str)> {
    code_lines(text).find_map(|(n, l)| {
        HELPER_FNS
            .iter()
            .chain(HELPER_LETS.iter())
            .find(|h| l.contains(**h))
            .map(|h| (n, *h))
    })
}

/// XDG の変数名の文字列が、`process-env` と例外のほかに無いことを検査する。
pub fn check_xdg_literals(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !is_owner(&s.path)) {
        match (xdg_hit(&s.text), exception(XDG_EXCEPTIONS, &s.path)) {
            (Some(n), false) => out.push(violation(
                &s.path,
                format!(
                    "{n} 行目に XDG の変数名の文字列がある。基準ディレクトリは process_env::Xdg::<種類>.resolve()(または resolve_lossy・resolve_under)で解決する"
                ),
            )),
            (None, true) => out.push(violation(
                &s.path,
                "XDG_EXCEPTIONS にあるが、XDG の変数名の文字列がもう無い。例外から消す",
            )),
            _ => {}
        }
    }
    for (path, _) in XDG_EXCEPTIONS {
        if !sources.iter().any(|s| s.path == *path) {
            out.push(violation(
                path,
                "XDG_EXCEPTIONS にあるが、ファイルが無い。例外から消す",
            ));
        }
    }
    out
}

/// 非空の環境変数・`HOME`・XDG の補助関数が、`process-env` のほかに定義されていないことを検査する。
pub fn check_helper_definitions(sources: &[Source]) -> Vec<Violation> {
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !is_owner(&s.path)) {
        match (helper_hit(&s.text), exception(HELPER_EXCEPTIONS, &s.path)) {
            (Some((n, what)), false) => out.push(violation(
                &s.path,
                format!(
                    "{n} 行目で `{what}` を定義している。非空の環境変数は process_env::var_nonempty、HOME は process_env::home を呼ぶ"
                ),
            )),
            (None, true) => out.push(violation(
                &s.path,
                "HELPER_EXCEPTIONS にあるが、その名前の定義がもう無い。例外から消す",
            )),
            _ => {}
        }
    }
    for (path, _) in HELPER_EXCEPTIONS {
        if !sources.iter().any(|s| s.path == *path) {
            out.push(violation(
                path,
                "HELPER_EXCEPTIONS にあるが、ファイルが無い。例外から消す",
            ));
        }
    }
    out
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let sources = read_sources(root)?;
    let mut out = check_xdg_literals(&sources);
    out.extend(check_helper_definitions(&sources));
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

    fn xdg_exceptions_present(v: Vec<Source>) -> Vec<Source> {
        with_exceptions(v, XDG_EXCEPTIONS, "let k = \"XDG_STATE_HOME\";\n")
    }

    fn helper_exceptions_present(v: Vec<Source>) -> Vec<Source> {
        with_exceptions(v, HELPER_EXCEPTIONS, "pub fn nonempty_env(n: &str) {}\n")
    }

    // ---- XDG の変数名: 正例 ----

    #[test]
    fn process_env_may_name_the_variables() {
        let v = xdg_exceptions_present(vec![src(
            "crates/process-env/src/lib.rs",
            "Xdg::State => \"XDG_STATE_HOME\",\n",
        )]);
        assert_eq!(check_xdg_literals(&v), vec![]);
    }

    #[test]
    fn comments_and_the_test_section_are_not_read() {
        let text = "//! `\"XDG_STATE_HOME\"` を解決する\n\
                    fn f() {}\n\
                    // let a = \"XDG_CONFIG_HOME\";\n\
                    #[cfg(test)]\n\
                    mod tests { fn t() { set(\"XDG_STATE_HOME\"); } }\n";
        let v = xdg_exceptions_present(vec![src("crates/x/src/lib.rs", text)]);
        assert_eq!(check_xdg_literals(&v), vec![]);
    }

    // ---- XDG の変数名: 負例(検査が実際に落ちること) ----

    #[test]
    fn a_handwritten_xdg_resolution_is_a_violation() {
        for name in ["CONFIG", "STATE", "CACHE", "DATA"] {
            let v = xdg_exceptions_present(vec![src(
                "crates/x/src/main.rs",
                &format!("let b = std::env::var_os(\"XDG_{name}_HOME\");\n"),
            )]);
            let got = check_xdg_literals(&v);
            assert_eq!(got.len(), 1, "{name}: {got:?}");
            assert_eq!(got[0].subject, "crates/x/src/main.rs");
            assert!(got[0].detail.contains("1 行目"), "{got:?}");
        }
    }

    #[test]
    fn a_stale_xdg_exception_is_a_violation() {
        let v = vec![src(XDG_EXCEPTIONS[0].0, "fn no_longer_names_it() {}\n")];
        let got = check_xdg_literals(&v);
        assert!(got.iter().any(|x| x.detail.contains("もう無い")), "{got:?}");
        assert!(check_xdg_literals(&[])
            .iter()
            .any(|x| x.detail.contains("ファイルが無い")));
    }

    // ---- 補助関数: 正例 / 負例 ----

    #[test]
    fn the_owner_and_the_exception_may_define_the_names() {
        let v = helper_exceptions_present(vec![src(
            "crates/process-env/src/lib.rs",
            "pub fn home() -> PathBuf { PathBuf::new() }\n",
        )]);
        assert_eq!(check_helper_definitions(&v), vec![]);
    }

    #[test]
    fn a_copied_helper_is_a_violation_in_every_form() {
        for text in [
            "fn env_nonempty(name: &str) -> Option<String> { None }\n",
            "pub fn non_empty_env(k: &str) -> Option<String> { None }\n",
            "fn xdg_dir(v: &str, f: &str) -> Option<PathBuf> { None }\n",
            "fn home() -> PathBuf { PathBuf::new() }\n",
            "    let env_nonempty = |n: &str| None::<String>;\n",
        ] {
            let v = helper_exceptions_present(vec![src("crates/x/src/lib.rs", text)]);
            let got = check_helper_definitions(&v);
            assert_eq!(got.len(), 1, "{text}: {got:?}");
        }
    }

    #[test]
    fn a_method_named_home_with_a_receiver_is_not_a_helper_copy() {
        let v = helper_exceptions_present(vec![src(
            "crates/x/src/lib.rs",
            "impl Env { fn home(&self) -> PathBuf { PathBuf::new() } }\n",
        )]);
        assert_eq!(check_helper_definitions(&v), vec![]);
    }

    #[test]
    fn a_stale_helper_exception_is_a_violation() {
        let v = vec![src(HELPER_EXCEPTIONS[0].0, "pub fn other() {}\n")];
        let got = check_helper_definitions(&v);
        assert!(got.iter().any(|x| x.detail.contains("もう無い")), "{got:?}");
    }
}
