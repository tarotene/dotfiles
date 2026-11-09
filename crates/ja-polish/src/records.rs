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
