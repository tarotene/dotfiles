//! 通した段落の記録(`records.jsonl`)。書き直しのコマンドが書き、関所(投稿の PreToolUse、
//! 文書ファイルの Stop)が読む。記録の形式の正本はここだけ。
//!
//! 1 行 1 段落: `{"ts_unix":…, "sha":"…", "state":"polished|skipped", "model":"…", "reason":"…"}`。
//! `sha` は段落の最終の本文の [`crate::blocks::hash`]。書き直した段落は書き直した後の本文、
//! 素通しにした段落は元の本文。関所は state を区別せず、`sha` があれば「通した」とみなす
//! (素通しは fail-open で、止めない設計。後から知らせる経路は gate イベント)。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub ts_unix: u64,
    pub sha: String,
    /// `"polished"`(書き直した、または書き直す必要が無いと確かめた)か `"skipped"`(素通し)。
    pub state: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Entry {
    pub fn polished(sha: String, model: &str) -> Self {
        Entry {
            ts_unix: now(),
            sha,
            state: "polished".to_string(),
            model: model.to_string(),
            reason: None,
        }
    }

    pub fn skipped(sha: String, model: &str, reason: &str) -> Self {
        Entry {
            ts_unix: now(),
            sha,
            state: "skipped".to_string(),
            model: model.to_string(),
            reason: Some(reason.to_string()),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 状態のディレクトリ: `${XDG_STATE_HOME:-$HOME/.local/state}/claude/ja-polish`
/// (gate-events.jsonl と同じ親)。
pub fn state_dir() -> Option<PathBuf> {
    Some(
        process_env::Xdg::State
            .resolve()?
            .join("claude")
            .join("ja-polish"),
    )
}

pub fn records_path() -> Option<PathBuf> {
    Some(state_dir()?.join("records.jsonl"))
}

/// 記録済みの `sha` の集合。ファイルが無い・読めない行は空・読み飛ばし(関所の判定を
/// 記録の不調で止めない)。
pub fn load(path: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .map(|e| e.sha)
        .collect()
}

/// `ja-polish` が動けない、と見る直近の件数と、最後の 1 件からの猶予(秒)。
pub const UNAVAILABLE_RUN: usize = 5;
pub const UNAVAILABLE_WINDOW: u64 = 60 * 60;

/// `ja-polish` が動けない状態か。直近の `UNAVAILABLE_RUN` 件の記録がすべて素通し(`skipped`)で、最後の 1 件が
/// `UNAVAILABLE_WINDOW` 秒以内なら、動けないとみなす(API キーが無い、LLM に届かない、など)。
///
/// 関所は、この間、通していない段落を拒否しない。本文を書いて `ja-polish` を通して出し直す往復を求めても、結果は
/// 全部素通しで、書き直しは 1 件も起きない。猶予が過ぎると関所は元に戻り、次の 1 回の往復が、直ったかの確かめになる
/// (直っていれば書き直しの記録が入り、素通しの並びが切れる)。
pub fn unavailable(path: &Path, now_unix: u64) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let entries: Vec<Entry> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .collect();
    let recent: Vec<&Entry> = entries.iter().rev().take(UNAVAILABLE_RUN).collect();
    recent.len() == UNAVAILABLE_RUN
        && recent.iter().all(|e| e.state == "skipped")
        && recent
            .first()
            .is_some_and(|e| now_unix.saturating_sub(e.ts_unix) <= UNAVAILABLE_WINDOW)
}

/// 今、`ja-polish` が動けない状態か([`unavailable`])。記録の場所が決まらなければ `false`。
pub fn unavailable_now() -> bool {
    records_path().is_some_and(|p| unavailable(&p, now()))
}

/// 追記する。書けなかったら `Err`(呼び出し側が警告にする)。
pub fn append(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    for e in entries {
        let line = serde_json::to_string(e)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/records.jsonl");
        append(
            &path,
            &[
                Entry::polished("aaa".into(), "m"),
                Entry::skipped("bbb".into(), "m", "タイムアウト"),
            ],
        )
        .unwrap();
        append(&path, &[Entry::polished("ccc".into(), "m")]).unwrap();
        let set = load(&path);
        assert_eq!(set.len(), 3);
        assert!(set.contains("aaa") && set.contains("bbb") && set.contains("ccc"));
    }

    fn entry_at(ts: u64, state: &str) -> Entry {
        Entry {
            ts_unix: ts,
            sha: format!("sha{ts}"),
            state: state.to_string(),
            model: "m".to_string(),
            reason: None,
        }
    }

    #[test]
    fn unavailable_needs_a_recent_run_of_skips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        let now = 100_000u64;
        // 記録が無い・足りない: 動ける(拒否する側に倒す)。
        assert!(!unavailable(&path, now));
        let skips: Vec<Entry> = (0..UNAVAILABLE_RUN as u64 - 1)
            .map(|i| entry_at(now - 60 + i, "skipped"))
            .collect();
        append(&path, &skips).unwrap();
        assert!(!unavailable(&path, now), "件数が足りない");
        // 直近 5 件がすべて素通しで、最後が新しい: 動けない。
        append(&path, &[entry_at(now - 10, "skipped")]).unwrap();
        assert!(unavailable(&path, now));
        // 最後の 1 件が猶予より古い: 元に戻る(次の往復が確かめになる)。
        assert!(!unavailable(&path, now + UNAVAILABLE_WINDOW + 1));
        // 書き直しの記録が 1 件入れば、並びが切れる(直った)。
        append(&path, &[entry_at(now - 5, "polished")]).unwrap();
        assert!(!unavailable(&path, now));
        // 古い素通しの並びは数えない: 最後の 5 件に書き直しが混ざらなくなれば、また動けない。
        let more: Vec<Entry> = (0..UNAVAILABLE_RUN as u64)
            .map(|i| entry_at(now - 4 + i, "skipped"))
            .collect();
        append(&path, &more).unwrap();
        assert!(unavailable(&path, now + 5));
    }

    #[test]
    fn load_survives_missing_file_and_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("none.jsonl")).is_empty());
        let path = dir.path().join("r.jsonl");
        std::fs::write(
            &path,
            "not json\n{\"ts_unix\":1,\"sha\":\"ok\",\"state\":\"polished\",\"model\":\"m\"}\n{}\n",
        )
        .unwrap();
        let set = load(&path);
        assert_eq!(set.len(), 1);
        assert!(set.contains("ok"));
    }

    #[test]
    fn empty_append_does_not_create_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        append(&path, &[]).unwrap();
        assert!(!path.exists());
    }
}
