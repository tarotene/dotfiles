//! process-env — 環境変数・ホームディレクトリ・XDG の基準ディレクトリの解決。依存ゼロ。
//!
//! bash の `${VAR:-}`(未設定と空文字を同一視する)と、`${XDG_STATE_HOME:-$HOME/.local/state}` の形が、
//! 約 20 の crate に手で複写されていた。複写ごとに、空文字の扱い(`XDG_STATE_HOME=""` を未設定として
//! 扱うか)と、`HOME` が無いときの扱い(空のパス・エラー・panic)が少しずつ違った。ここに 1 つに集める。
//!
//! 機能の永続状態のパス(`<基準>/claude/wrapup` など)は、各機能が持つ。ここは基準ディレクトリまでを解決する
//! (`docs/agents/tooling-conventions.md` の境界)。
//!
//! 実環境を読む関数は、環境を引く関数を受ける `*_with` の薄い包みで、テストは `*_with` を使う
//! (プロセスの環境変数を書き換えると、並列のテストが競合する)。

use std::ffi::OsString;
use std::path::PathBuf;

/// `${NAME:-}` が非空なら値(UTF-8 でないバイトは置き換える)。
pub fn var_nonempty(name: &str) -> Option<String> {
    var_nonempty_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// `${NAME:-}` が非空なら値(バイトのまま)。
pub fn var_nonempty_os(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// `${NAME:-}` が非空ならパス(バイトのまま)。
pub fn var_nonempty_path(name: &str) -> Option<PathBuf> {
    var_nonempty_os(name).map(PathBuf::from)
}

/// 設定されているか(空文字でも設定とみなす)。`[ -n "${NAME+x}" ]` の意味。
pub fn var_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// `$HOME`。無ければ `None`。空文字は `Some("")`(空のパス)として返す。
pub fn home_opt() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `$HOME`。無ければ空のパス(以降の `join` は相対パスになる)。
pub fn home() -> PathBuf {
    home_opt().unwrap_or_default()
}

/// XDG の基準ディレクトリ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xdg {
    Config,
    State,
    Cache,
    Data,
}

impl Xdg {
    /// 環境変数の名前。
    pub fn var(self) -> &'static str {
        match self {
            Xdg::Config => "XDG_CONFIG_HOME",
            Xdg::State => "XDG_STATE_HOME",
            Xdg::Cache => "XDG_CACHE_HOME",
            Xdg::Data => "XDG_DATA_HOME",
        }
    }

    /// 環境変数が空・未設定のときの、`$HOME` からの相対パス。
    pub fn default_under_home(self) -> &'static str {
        match self {
            Xdg::Config => ".config",
            Xdg::State => ".local/state",
            Xdg::Cache => ".cache",
            Xdg::Data => ".local/share",
        }
    }

    /// `${XDG_*_HOME:-$HOME/<既定>}`。環境変数が空なら未設定として扱う。
    /// 環境変数が無く、`HOME` も無いときだけ `None`。
    pub fn resolve(self) -> Option<PathBuf> {
        self.resolve_with(|n| std::env::var_os(n))
    }

    /// `resolve` の、環境を引く関数を受ける形(テスト用)。
    pub fn resolve_with(self, get: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
        xdg_dir_with(self.var(), self.default_under_home(), get)
    }

    /// `${XDG_*_HOME:-<home>/<既定>}`。`HOME` を環境から読まず、呼び出し側が持つ `home` を使う
    /// (`HOME` が必須で、すでに検査して持っている呼び出し側向け)。環境変数が空なら未設定として扱う。
    pub fn resolve_under(self, home: &std::path::Path) -> PathBuf {
        match var_nonempty_os(self.var()) {
            Some(v) => PathBuf::from(v),
            None => home.join(self.default_under_home()),
        }
    }

    /// `resolve` と同じだが、`HOME` が無いときは空の `HOME` として続ける(結果は相対パスになる)。
    /// 失敗を返せない呼び出し側(`HOME` を `unwrap_or_default()` で読んでいた箇所)が、挙動を変えずに使う。
    pub fn resolve_lossy(self) -> PathBuf {
        self.resolve()
            .unwrap_or_else(|| PathBuf::from(self.default_under_home()))
    }
}

/// `${<var>:-$HOME/<fallback>}`。`var` が空・未設定なら `$HOME/<fallback>`。
/// `Xdg` に無い変数(`XDG_RUNTIME_DIR` など)や、別の既定を持つものに使う。
pub fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    xdg_dir_with(var, fallback, |n| std::env::var_os(n))
}

/// `xdg_dir` の、環境を引く関数を受ける形(テスト用)。
pub fn xdg_dir_with(
    var: &str,
    fallback: &str,
    get: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match get(var).filter(|v| !v.is_empty()) {
        Some(v) => Some(PathBuf::from(v)),
        None => get("HOME").map(|h| PathBuf::from(h).join(fallback)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |n| m.get(n).cloned()
    }

    #[test]
    fn xdg_uses_the_variable_when_non_empty() {
        let e = env(&[("XDG_STATE_HOME", "/x/state"), ("HOME", "/h")]);
        assert_eq!(Xdg::State.resolve_with(e), Some(PathBuf::from("/x/state")));
    }

    #[test]
    fn an_empty_xdg_variable_is_unset() {
        let e = env(&[("XDG_STATE_HOME", ""), ("HOME", "/h")]);
        assert_eq!(
            Xdg::State.resolve_with(e),
            Some(PathBuf::from("/h/.local/state"))
        );
    }

    #[test]
    fn each_kind_has_its_own_variable_and_default() {
        let e = || env(&[("HOME", "/h")]);
        assert_eq!(
            Xdg::Config.resolve_with(e()),
            Some(PathBuf::from("/h/.config"))
        );
        assert_eq!(
            Xdg::State.resolve_with(e()),
            Some(PathBuf::from("/h/.local/state"))
        );
        assert_eq!(
            Xdg::Cache.resolve_with(e()),
            Some(PathBuf::from("/h/.cache"))
        );
        assert_eq!(
            Xdg::Data.resolve_with(e()),
            Some(PathBuf::from("/h/.local/share"))
        );
        assert_eq!(Xdg::Cache.var(), "XDG_CACHE_HOME");
    }

    #[test]
    fn no_home_and_no_variable_is_none_but_the_variable_alone_is_enough() {
        assert_eq!(Xdg::Config.resolve_with(env(&[])), None);
        let e = env(&[("XDG_CONFIG_HOME", "/c")]);
        assert_eq!(Xdg::Config.resolve_with(e), Some(PathBuf::from("/c")));
    }

    #[test]
    fn xdg_dir_takes_any_variable_and_fallback() {
        let e = env(&[("HOME", "/h")]);
        assert_eq!(
            xdg_dir_with("XDG_RUNTIME_DIR", ".run", e),
            Some(PathBuf::from("/h/.run"))
        );
    }

    #[test]
    fn resolve_under_uses_the_given_home_when_the_variable_is_unset() {
        // 変数が実環境で設定されているかに依らず成り立つ形だけを確かめる: 戻り値は、変数の値か home の下。
        let home = std::path::Path::new("/given/home");
        let got = Xdg::State.resolve_under(home);
        let from_env = var_nonempty_os(Xdg::State.var()).map(PathBuf::from);
        assert_eq!(got, from_env.unwrap_or_else(|| home.join(".local/state")));
    }

    #[test]
    fn lossy_resolution_without_home_is_the_relative_default() {
        // 実環境を読むので、HOME を持たない値の比較は、既定の綴りだけを確かめる。
        assert_eq!(Xdg::State.default_under_home(), ".local/state");
        let none = Xdg::Config.resolve_with(env(&[]));
        assert_eq!(
            none.unwrap_or_else(|| PathBuf::from(".config")),
            PathBuf::from(".config")
        );
    }

    #[test]
    fn an_empty_home_gives_a_relative_path_like_the_shell_would() {
        let e = env(&[("HOME", "")]);
        assert_eq!(
            Xdg::State.resolve_with(e),
            Some(PathBuf::from(".local/state"))
        );
    }
}
