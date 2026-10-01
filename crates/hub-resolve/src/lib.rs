//! private ハブリポジトリの絶対パス解決(writing-style-hub / performance-hub、
//! bash 版 `scripts/writing-style-hub`・`scripts/performance-hub` の移植、
//! ADR-0024 / #389 Stage 4e)。
//!
//! 設計と根拠: docs/claude/writing-style.md(#115)、docs/claude/performance-planning.md
//!
//! ハブの絶対パスをソースにハードコードしない(private リポジトリの存在自体を
//! 公開ソースに書かないため)— ADR-0019 のホスト・マーカー方式
//! (`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/host`)と同じ間接参照の型で、
//! 環境変数 / マーカーファイル 1 個のどちらかからパスを解決する。
//!
//! 解決順序:
//!   1. 環境変数(セッション限定の上書き用)
//!   2. `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/<marker>` の 1 行目(絶対パス)
//!
//! 縮退: マーカーが無い / 指すパスが存在しない / README が無い、いずれも
//! 黙って何もしない(silent failure)のではなく、明示的なメッセージを stderr に
//! 出して非 0 で終わる(ADR-0005 の binary-existence gating と同じ「存在しない
//! なら明示して止まる」原則。docs/claude/wrapup-inbox.md も同じ縮退方針の先例)。

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// 1 本のハブ解決コマンドの仕様。
pub struct Hub {
    /// コマンド名(メッセージ接頭辞)。
    pub name: &'static str,
    /// 上書き用の環境変数名。
    pub env_var: &'static str,
    /// `dotfiles/` 配下のマーカーファイル名。
    pub marker_name: &'static str,
    /// ハブ直下から見た、存在確認する README の相対パス(レイアウト検査)。
    pub readme_rel: &'static str,
    /// 未設定時メッセージの 1 行目(`<name>: ` の後)。
    pub unset_msg: &'static str,
    /// 未設定時の設定例に出すプレースホルダ(実値は書かない、ADR-0034)。
    pub repo_placeholder: &'static str,
}

pub const WRITING_STYLE_HUB: Hub = Hub {
    name: "writing-style-hub",
    env_var: "WRITING_STYLE_HUB",
    marker_name: "style-hub",
    readme_rel: "docs/style/README.md",
    unset_msg: "スタイルガイドのハブが未設定です。次のいずれかで設定してください:",
    repo_placeholder: "/path/to/style-hub-repo",
};

pub const PERFORMANCE_HUB: Hub = Hub {
    name: "performance-hub",
    env_var: "PERFORMANCE_HUB",
    marker_name: "performance-hub",
    readme_rel: "state/performances/README.md",
    unset_msg: "演奏本番データのハブが未設定です。次のいずれかで設定してください:",
    repo_placeholder: "/path/to/person-state-repo",
};

/// 環境変数の取得を差し替え可能にした入口(テスト用)。
pub type Getenv<'a> = &'a dyn Fn(&str) -> Option<String>;

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

/// マーカーファイルのパス(`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/<marker>`)。
pub fn marker_path(hub: &Hub, getenv: Getenv) -> PathBuf {
    let base = non_empty(getenv("XDG_CONFIG_HOME")).map_or_else(
        || PathBuf::from(getenv("HOME").unwrap_or_default()).join(".config"),
        PathBuf::from,
    );
    base.join("dotfiles").join(hub.marker_name)
}

/// bash の `head -n1 -- "$MARKER" || true`。読めなければ空。
fn first_line(path: &Path) -> String {
    let Ok(f) = File::open(path) else {
        return String::new();
    };
    let mut line = String::new();
    let _ = BufReader::new(f).read_line(&mut line);
    // `$(...)` が末尾改行を落とすのと同じ。
    line.trim_end_matches('\n').to_string()
}

/// 成功時は解決したハブの絶対パス、失敗時は stderr にそのまま出すメッセージ(改行込み)。
pub fn resolve(hub: &Hub, getenv: Getenv) -> Result<String, String> {
    let marker = marker_path(hub, getenv);
    let resolved = if let Some(v) = non_empty(getenv(hub.env_var)) {
        v
    } else if File::open(&marker).is_ok() {
        first_line(&marker)
    } else {
        String::new()
    };

    if resolved.is_empty() {
        return Err(format!(
            "{n}: {msg}\n  echo {ph} > '{marker}'\n  export {env}={ph}\n",
            n = hub.name,
            msg = hub.unset_msg,
            ph = hub.repo_placeholder,
            marker = marker.display(),
            env = hub.env_var,
        ));
    }
    let hub_dir = Path::new(&resolved);
    if !hub_dir.is_dir() {
        return Err(format!(
            "{}: ハブのパスが存在しません: {resolved}\n",
            hub.name
        ));
    }
    if File::open(hub_dir.join(hub.readme_rel)).is_err() {
        return Err(format!(
            "{n}: {resolved}/{rel} が読めません(ハブのレイアウトが想定と違う可能性)。\n",
            n = hub.name,
            rel = hub.readme_rel,
        ));
    }
    Ok(resolved)
}

/// bin 共通の main。成功時は末尾改行なしでパスを stdout へ。
pub fn run(hub: &Hub) -> std::process::ExitCode {
    use std::io::Write;
    let getenv = |k: &str| std::env::var(k).ok();
    match resolve(hub, &getenv) {
        Ok(path) => {
            let mut out = std::io::stdout();
            let _ = out.write_all(path.as_bytes());
            let _ = out.flush();
            std::process::ExitCode::SUCCESS
        }
        Err(msg) => {
            eprint!("{msg}");
            std::process::ExitCode::FAILURE
        }
    }
}
