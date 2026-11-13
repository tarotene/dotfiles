//! private ハブリポジトリの絶対パス解決(writing-style-hub / performance-hub、
//! bash 版 `scripts/writing-style-hub`・`scripts/performance-hub` の移植、
//! ADR-0024 / #389 Stage 4e)と、机の正本の解決(desk-hub、`resolve_desk`)。
//!
//! 設計と根拠: docs/agents/writing-style.md(#115)、docs/agents/music-performance-planning.md、
//! docs/agents/my-desk.md(desk-hub)
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
//! なら明示して止まる」原則。docs/agents/wrapup-inbox.md も同じ縮退方針の先例)。

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

/// 机の正本の、ハブの中での場所。個人と会社の person-state リポジトリで同じ(対称な対)。
pub const DESK_CANON: &str = "state/reviews/daily/desk.md";

/// 机(`my-desk`)の正本を持つハブ。機体ごとに、どの person-state リポジトリを机の正本の置き場に
/// するかを指す(会社の機体は会社側の person-state リポジトリ)。個人データ用の `performance-hub` を
/// 会社の機体に置かずに、その機体の机の入口を解決するための、別のマーカー。レイアウトの検査は、
/// 机の正本のファイルそのものに対して行う。
pub const DESK_HUB: Hub = Hub {
    name: "desk-hub",
    env_var: "DESK_HUB",
    marker_name: "desk-hub",
    readme_rel: DESK_CANON,
    unset_msg: "机の正本のハブが未設定です。次のいずれかで設定してください:",
    repo_placeholder: "/path/to/person-state-repo",
};

/// Obsidian vault(worklog と vault-note が共有する単一の正本)。マーカーの実値は
/// `home/hosts/vega/obsidian.nix` が 1 か所だけで置く(ADR-0034)。存在確認は `.obsidian/app.json`
/// で、日次ノートの設定(`daily-notes.json`)を要るのは worklog だけが追加で確かめる。
pub const OBSIDIAN_VAULT: Hub = Hub {
    name: "obsidian-vault",
    env_var: "OBSIDIAN_VAULT",
    marker_name: "obsidian-vault",
    readme_rel: ".obsidian/app.json",
    unset_msg: "vault の場所が未設定です。次のいずれかで設定してください:",
    repo_placeholder: "/path/to/obsidian-vault",
};

/// 環境変数の取得を差し替え可能にした入口(テスト用)。
pub type Getenv<'a> = &'a dyn Fn(&str) -> Option<String>;

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

/// マーカーファイルのパス(`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/<marker>`)。
pub fn marker_path(hub: &Hub, getenv: Getenv) -> PathBuf {
    let base = process_env::Xdg::Config
        .resolve_with(|n| getenv(n).map(std::ffi::OsString::from))
        .unwrap_or_else(|| PathBuf::from(process_env::Xdg::Config.default_under_home()));
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

/// 環境変数、無ければマーカーの 1 行目。どちらも無い・空なら空文字列(検査の前の値)。
fn raw(hub: &Hub, getenv: Getenv) -> String {
    if let Some(v) = non_empty(getenv(hub.env_var)) {
        v
    } else {
        let marker = marker_path(hub, getenv);
        if File::open(&marker).is_ok() {
            first_line(&marker)
        } else {
            String::new()
        }
    }
}

/// 成功時は解決したハブの絶対パス、失敗時は stderr にそのまま出すメッセージ(改行込み)。
pub fn resolve(hub: &Hub, getenv: Getenv) -> Result<String, String> {
    let marker = marker_path(hub, getenv);
    let resolved = raw(hub, getenv);

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
    if !hub.readme_rel.is_empty() && File::open(hub_dir.join(hub.readme_rel)).is_err() {
        return Err(format!(
            "{n}: {resolved}/{rel} が読めません(ハブのレイアウトが想定と違う可能性)。\n",
            n = hub.name,
            rel = hub.readme_rel,
        ));
    }
    Ok(resolved)
}

/// `resolve_desk` の失敗。どちらも stderr にそのまま出すメッセージ(改行込み)を持つ。
#[derive(Debug, PartialEq, Eq)]
pub enum DeskError {
    /// どのハブも設定されていない。「この機体には机が無い」で、起動時の hook は黙って何もしない。
    Unset(String),
    /// ハブは設定されているが解決できない(パスが無い・机の正本が読めない)。
    Broken(String),
}

/// この機体の机のハブ(person-state リポジトリ)の絶対パスを解決する。机の正本は、ハブの
/// `DESK_CANON` にある(個人と会社で同じ場所)。
///
/// 1. 目印 `desk-hub`(環境変数 `DESK_HUB`、無ければマーカーの 1 行目)があれば、それが机のハブ。
/// 2. 目印 `desk-hub` が無く、`performance-hub` があれば、従来どおりそのハブが机を持つ。
/// 3. どちらも無ければ `DeskError::Unset`。
///
/// 会社の機体は、個人データ用の `performance-hub` を置かず、`desk-hub` だけで会社側の
/// person-state リポジトリを指す。
pub fn resolve_desk(getenv: Getenv) -> Result<String, DeskError> {
    if !raw(&DESK_HUB, getenv).is_empty() {
        // `readme_rel` が `DESK_CANON` なので、机の正本が無ければここで失敗する。
        return resolve(&DESK_HUB, getenv).map_err(DeskError::Broken);
    }
    if !raw(&PERFORMANCE_HUB, getenv).is_empty() {
        let hub = resolve(&PERFORMANCE_HUB, getenv).map_err(DeskError::Broken)?;
        let canon = Path::new(&hub).join(DESK_CANON);
        if !canon.is_file() {
            return Err(DeskError::Broken(format!(
                "{n}: 机の正本が読めません: {path}\n",
                n = DESK_HUB.name,
                path = canon.display(),
            )));
        }
        return Ok(hub);
    }
    Err(DeskError::Unset(format!(
        "{n}: {msg}\n  echo {ph} > '{marker}'\n  export {env}={ph}\n  \
         (従来の `performance-hub` が設定されている機体は、そのハブが机を持ちます)\n",
        n = DESK_HUB.name,
        msg = DESK_HUB.unset_msg,
        ph = DESK_HUB.repo_placeholder,
        marker = marker_path(&DESK_HUB, getenv).display(),
        env = DESK_HUB.env_var,
    )))
}

/// `desk-hub` の main。引数なしで、机のハブの絶対パスを末尾改行なしで stdout へ。
pub fn run_desk(args: &[String]) -> std::process::ExitCode {
    use std::io::Write;
    if !args.is_empty() {
        eprintln!("desk-hub: 使い方: desk-hub(引数なし)");
        return std::process::ExitCode::from(2);
    }
    let getenv = |k: &str| std::env::var(k).ok();
    match resolve_desk(&getenv) {
        Ok(hub) => {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(hub.as_bytes());
            let _ = stdout.flush();
            std::process::ExitCode::SUCCESS
        }
        Err(DeskError::Unset(msg) | DeskError::Broken(msg)) => {
            eprint!("{msg}");
            std::process::ExitCode::FAILURE
        }
    }
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
