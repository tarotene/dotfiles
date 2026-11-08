//! 設定ファイル `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/ja-polish.toml`。
//!
//! home-manager の `dotfiles.jaPolish.enable` が置く(`home/modules/ja-polish.nix`)。
//! **このファイルの有無が、その機体で ja-polish が有効かどうかの唯一の印**: 関所は、無ければ
//! 何も検査しない。会社の機体には置かれないので、関所も LLM への送信も起きない。

use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "gemini-3.8-flash";
const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub model: String,
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: DEFAULT_MODEL.to_string(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

#[derive(Deserialize, Default)]
struct File {
    model: Option<String>,
    timeout_secs: Option<u64>,
}

pub fn config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("dotfiles").join("ja-polish.toml"))
}

/// 設定ファイルがあれば読む(有効な機体)。無ければ `None`(無効な機体)。壊れていても
/// 有効とみなし、壊れた項目は既定に戻す。
pub fn load() -> Option<Config> {
    let path = config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    Some(parse(&text))
}

pub fn parse(text: &str) -> Config {
    let file: File = toml::from_str(text).unwrap_or_default();
    let mut c = Config::default();
    if let Some(m) = file.model.filter(|m| !m.trim().is_empty()) {
        c.model = m;
    }
    if let Some(t) = file.timeout_secs.filter(|t| *t > 0) {
        c.timeout = Duration::from_secs(t);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() {
        assert_eq!(parse("").model, DEFAULT_MODEL);
        let c = parse("model = \"gemini-x\"\ntimeout_secs = 30\n");
        assert_eq!(c.model, "gemini-x");
        assert_eq!(c.timeout, Duration::from_secs(30));
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        assert_eq!(parse("model = [").model, DEFAULT_MODEL);
        assert_eq!(parse("model = \"\"\n").model, DEFAULT_MODEL);
        assert_eq!(
            parse("timeout_secs = 0\n").timeout,
            Duration::from_secs(120)
        );
    }
}
