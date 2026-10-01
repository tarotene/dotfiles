//! 環境変数からの設定(bash 版冒頭の `${GITHUB_APP_SNAPSHOT_*:-...}`)。
//!
//! `*_BIN` は PATH 上のコマンドを差し替えるテスト用の口で、bash 版から
//! そのまま引き継ぐ(`JQ_BIN` だけは jq を使わなくなったので無い)。

use std::path::PathBuf;

pub const KEYRING_ATTRIBUTE_APP: &str = "github-app-snapshot";
pub const KEYRING_ATTRIBUTE_KIND: &str = "bws-access-token";
pub const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone)]
pub struct Config {
    pub owner: String,
    pub state_dir: PathBuf,
    pub manifests_dir: PathBuf,
    pub bws_bin: String,
    pub secret_tool_bin: String,
    pub curl_bin: String,
    pub openssl_bin: String,
    pub env_bin: String,
}

fn env_or(name: &str, default: impl FnOnce() -> String) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default(),
    }
}

fn xdg(var: &str, home_rel: &str) -> PathBuf {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(home_rel),
    }
}

impl Config {
    pub fn from_env() -> Config {
        let config_dir = PathBuf::from(env_or("GITHUB_APP_SNAPSHOT_CONFIG_DIR", || {
            xdg("XDG_CONFIG_HOME", ".config")
                .join("github-app-snapshot")
                .to_string_lossy()
                .into_owned()
        }));
        // github-audit と同じ state ディレクトリ: github-audit が遅延して読む
        // ファイルそのもので、別の名前空間ではない(docs/github-audit.md)。
        let state_dir = PathBuf::from(env_or("GITHUB_APP_SNAPSHOT_STATE_DIR", || {
            xdg("XDG_STATE_HOME", ".local/state")
                .join("github-audit")
                .to_string_lossy()
                .into_owned()
        }));
        let manifests_dir = match std::env::var("GITHUB_APP_SNAPSHOT_MANIFESTS_DIR") {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => config_dir.join("manifests"),
        };
        Config {
            owner: env_or("GITHUB_APP_SNAPSHOT_OWNER", || "tarotene".to_string()),
            state_dir,
            manifests_dir,
            bws_bin: env_or("GITHUB_APP_SNAPSHOT_BWS_BIN", || "bws".to_string()),
            secret_tool_bin: env_or("GITHUB_APP_SNAPSHOT_SECRET_TOOL_BIN", || {
                "secret-tool".to_string()
            }),
            curl_bin: env_or("GITHUB_APP_SNAPSHOT_CURL_BIN", || "curl".to_string()),
            openssl_bin: env_or("GITHUB_APP_SNAPSHOT_OPENSSL_BIN", || "openssl".to_string()),
            env_bin: env_or("GITHUB_APP_SNAPSHOT_ENV_BIN", || "env".to_string()),
        }
    }
}
