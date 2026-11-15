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

/// 外に送る文章の例文集の場所。文体規約と同じ `docs/style/` の `messages.toml`(private のハブの側に置く。
/// 公開のこのリポジトリには、例文の中身を置かない)。環境変数 `JA_POLISH_MESSAGES_FILE` があれば、そのファイル
/// (テスト用の差し替え)。
pub fn messages_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("JA_POLISH_MESSAGES_FILE").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let hub = resolve(&WRITING_STYLE_HUB, &|k| std::env::var(k).ok()).map_err(|msg| {
        let first = msg.lines().next().unwrap_or("").to_string();
        format!("スタイルガイドのハブを解決できません: {first}")
    })?;
    Ok(Path::new(&hub).join("docs/style/messages.toml"))
}

/// メール・メッセージの下書きの基準: 文体規約に、例文集と、例文を保つ指示を足したもの。例文集が読めない
/// ときは `Err`で、基準なしでは送らない(文書の基準だけで書き直すと、例文どおりの言い回しを書き換えてしまう)。
pub fn load_message() -> Result<String, String> {
    let writing = load()?;
    let path = messages_path()?;
    let messages = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    Ok(compose_message(&writing, &messages))
}

/// 文体規約と例文集を、1 つの基準の文字列にする。
fn compose_message(writing: &str, messages: &str) -> String {
    format!(
        "{writing}\n\n---\n\n# 外に送る文章(メール・メッセージ)の例文集\n\n\
次の例文集は、本人が実際に送った文から取った言い回しである。例文どおりの言い回し(依頼の「〜いただければ」、結びの\
「よろしくお願いいたします」、署名など)は、意味が通るかぎり変えない。補助動詞は仮名で書く。外に送る文では、半角と全角の\
間に空白を入れない(「10月28日」「2027年4月」)。例文に合わせるために、事実・数値・相手の事情を変えてはならない。\n\n{messages}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_basis_is_the_style_guide_then_the_phrasebank_with_the_keep_instruction() {
        let s = compose_message(
            "# 規約\nR1. 常体で書く",
            "[ja.move.closing]\nlabel = \"結び\"",
        );
        let rules = s.find("R1. 常体で書く").unwrap();
        let keep = s.find("例文どおりの言い回し").unwrap();
        let book = s.find("[ja.move.closing]").unwrap();
        assert!(rules < keep && keep < book, "{s}");
        assert!(s.contains("半角と全角の間に空白を入れない"), "{s}");
    }

    #[test]
    fn messages_file_can_be_replaced_by_an_environment_variable() {
        std::env::set_var("JA_POLISH_MESSAGES_FILE", "/x/messages.toml");
        assert_eq!(
            messages_path().unwrap(),
            std::path::PathBuf::from("/x/messages.toml")
        );
        std::env::remove_var("JA_POLISH_MESSAGES_FILE");
    }
}
