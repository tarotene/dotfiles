//! 書き直しの基準(文体規約)の読み込み。
//!
//! 規約の正本は private のスタイルガイドのハブ(`writing-style-hub` が指す
//! `docs/style/writing.md`)。公開のこのリポジトリには規約の中身を置かず、実行のたびに
//! 解決して読む(写しを持たない)。解決できないときは `Err` で、基準なしでは送らない。
//! 環境変数 `JA_POLISH_STYLE_FILE` があれば、そのファイルを読む(テスト用の差し替え)。

use hub_resolve::{resolve, WRITING_STYLE_HUB};
use std::path::{Path, PathBuf};

pub fn style_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("JA_POLISH_STYLE_FILE").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let hub = resolve(&WRITING_STYLE_HUB, &|k| std::env::var(k).ok()).map_err(|msg| {
        let first = msg.lines().next().unwrap_or("").to_string();
        format!("スタイルガイドのハブを解決できません: {first}")
    })?;
    Ok(Path::new(&hub).join("docs/style/writing.md"))
}

/// 辞書(prh)の場所。文体規約と同じ `docs/style/` の `prh.yml`(private のハブの側に置く。公開のこの
/// リポジトリには、辞書の中身を置かない)。ハブが解決できないときは `None`で、辞書なしで動く。
/// 環境変数 `JA_POLISH_PRH_FILE` があれば、そのファイル(テスト用の差し替え)。
pub fn prh_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("JA_POLISH_PRH_FILE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let hub = resolve(&WRITING_STYLE_HUB, &|k| std::env::var(k).ok()).ok()?;
    Some(Path::new(&hub).join("docs/style/prh.yml"))
}

pub fn load() -> Result<String, String> {
    let path = style_path()?;
    std::fs::read_to_string(&path).map_err(|e| format!("{} を読めません: {e}", path.display()))
}
