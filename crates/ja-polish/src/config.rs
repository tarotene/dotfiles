//! 設定ファイル `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/ja-polish.toml`。
//!
//! home-manager の `dotfiles.jaPolish.enable` が置く(`home/modules/ja-polish.nix`)。
//! **このファイルの有無が、その機体で ja-polish が有効かどうかの唯一の印**: 関所は、無ければ
//! 何も検査しない。置いていない機体(今は会社の機体)では、関所も LLM への送信も起きない。

use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// `home/modules/ja-polish.nix` の `model` の既定値と同じにする(暫定の理由と戻す条件はそちらのコメント)。
pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";
/// `home/modules/ja-polish.nix` の `fallbackModel` の既定値と同じにする。
pub const DEFAULT_FALLBACK_MODEL: &str = "gemini-flash-lite-latest";
/// 1 回の依頼の待ち(秒)。応答が返らないモデル(無料枠の混雑)を待ち続けないための値で、
/// 120 秒だった以前の値から縮めた(docs/adr/1387-ja-polish.md の D9)。
const DEFAULT_TIMEOUT_SECS: u64 = 45;
/// 1 回の依頼に載せる段落の数。多い文書は、これで分けて並べて送る(D10)。
const DEFAULT_CHUNK_PARAS: usize = 8;
/// 同時に送る依頼の数。
const DEFAULT_PARALLEL: usize = 3;
/// 混雑(429・503 など)で同じモデルに再依頼する回数。時間切れは再依頼せず、予備モデルへ移る。
const DEFAULT_RETRIES: u32 = 2;
/// 再依頼の待ちの最初の長さ(ミリ秒)。回ごとに 3 倍になり、±25% のゆらぎが付く。
const DEFAULT_RETRY_BASE_MS: u64 = 2000;
/// llm-gemini の `-o thinking_level` が受ける値(llm-gemini 0.35 の `MODEL_THINKING_LEVELS`)。
const THINKING_LEVELS: [&str; 4] = ["minimal", "low", "medium", "high"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub model: String,
    /// 主のモデルが混雑・時間切れで通らないときに 1 回だけ試すモデル。`None` なら使わない。
    /// 思考量の指定(`thinking_level`)は、主のモデルにだけ渡す(予備が同じ選択肢を持つとは限らない)。
    pub fallback_model: Option<String>,
    /// 主のモデルの思考量(`minimal` `low` `medium` `high`)。`None` ならモデルの既定に任せる。
    pub thinking_level: Option<String>,
    pub timeout: Duration,
    pub chunk_paras: usize,
    pub parallel: usize,
    pub retries: u32,
    pub retry_base: Duration,
    /// Gemini の前後に textlint の自動修正を挟むか(D13)。`textlint` コマンドが無いときは、
    /// この設定に関わらず挟まない(止めない)。
    pub textlint: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: DEFAULT_MODEL.to_string(),
            fallback_model: Some(DEFAULT_FALLBACK_MODEL.to_string()),
            thinking_level: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            chunk_paras: DEFAULT_CHUNK_PARAS,
            parallel: DEFAULT_PARALLEL,
            retries: DEFAULT_RETRIES,
            retry_base: Duration::from_millis(DEFAULT_RETRY_BASE_MS),
            textlint: true,
        }
    }
}

#[derive(Deserialize, Default)]
struct File {
    model: Option<String>,
    fallback_model: Option<String>,
    thinking_level: Option<String>,
    timeout_secs: Option<u64>,
    chunk_paras: Option<usize>,
    parallel: Option<usize>,
    retries: Option<u32>,
    retry_base_ms: Option<u64>,
    textlint: Option<bool>,
}

pub fn config_path() -> Option<PathBuf> {
    Some(
        process_env::Xdg::Config
            .resolve()?
            .join("dotfiles")
            .join("ja-polish.toml"),
    )
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
    // 空の文字列は「予備を使わない」。キーが無いときだけ既定の予備を使う。
    if let Some(m) = file.fallback_model {
        c.fallback_model = Some(m.trim().to_string()).filter(|m| !m.is_empty());
    }
    c.thinking_level = file
        .thinking_level
        .map(|l| l.trim().to_string())
        .filter(|l| THINKING_LEVELS.contains(&l.as_str()));
    if let Some(t) = file.timeout_secs.filter(|t| *t > 0) {
        c.timeout = Duration::from_secs(t);
    }
    if let Some(n) = file.chunk_paras.filter(|n| *n > 0) {
        c.chunk_paras = n;
    }
    if let Some(n) = file.parallel.filter(|n| *n > 0) {
        c.parallel = n;
    }
    if let Some(n) = file.retries {
        c.retries = n;
    }
    if let Some(ms) = file.retry_base_ms {
        c.retry_base = Duration::from_millis(ms);
    }
    if let Some(b) = file.textlint {
        c.textlint = b;
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
    fn resilience_defaults() {
        let c = parse("");
        assert_eq!(c.timeout, Duration::from_secs(45));
        assert_eq!(c.fallback_model.as_deref(), Some(DEFAULT_FALLBACK_MODEL));
        assert_eq!(c.thinking_level, None);
        assert_eq!((c.chunk_paras, c.parallel, c.retries), (8, 3, 2));
        assert_eq!(c.retry_base, Duration::from_millis(2000));
    }

    #[test]
    fn resilience_overrides() {
        let c = parse(
            "fallback_model = \"m2\"\nthinking_level = \"low\"\nchunk_paras = 4\nparallel = 1\nretries = 0\nretry_base_ms = 0\n",
        );
        assert_eq!(c.fallback_model.as_deref(), Some("m2"));
        assert_eq!(c.thinking_level.as_deref(), Some("low"));
        assert_eq!((c.chunk_paras, c.parallel, c.retries), (4, 1, 0));
        assert_eq!(c.retry_base, Duration::ZERO);
    }

    #[test]
    fn textlint_is_on_by_default_and_can_be_turned_off() {
        assert!(parse("").textlint);
        assert!(!parse("textlint = false\n").textlint);
    }

    #[test]
    fn empty_fallback_disables_it_and_unknown_thinking_level_is_dropped() {
        assert_eq!(parse("fallback_model = \"\"\n").fallback_model, None);
        assert_eq!(parse("thinking_level = \"extreme\"\n").thinking_level, None);
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        assert_eq!(parse("model = [").model, DEFAULT_MODEL);
        assert_eq!(parse("model = \"\"\n").model, DEFAULT_MODEL);
        assert_eq!(parse("timeout_secs = 0\n").timeout, Duration::from_secs(45));
        assert_eq!(parse("chunk_paras = 0\nparallel = 0\n").chunk_paras, 8);
        assert_eq!(parse("parallel = 0\n").parallel, 3);
    }
}
