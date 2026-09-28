//! 新規 Rust gate の deny/skip イベントを JSONL に記録する(ADR-543 段3、
//! 降格候補検出の入力)。既存 bash gate には計装しない —
//! bash→Rust 移行時に hook-io 側で拾う方が二度手間にならない(ADR-543 の
//! `## 先行例との対比` D6 参照)。
//!
//! フォーマットは1行1レコードの JSON(`{ts_unix, gate, decision}`)。
//! `decision` は `"deny"` か `"skip"` の2値。

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
struct Record<'a> {
    ts_unix: u64,
    gate: &'a str,
    decision: &'a str,
}

/// `${XDG_STATE_HOME:-$HOME/.local/state}/claude/gate-events.jsonl`。
pub fn default_path() -> Option<PathBuf> {
    default_path_with(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

/// [`default_path`] の環境変数を注入可能にした版(テスト用)。
pub fn default_path_with(
    xdg_state_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = match xdg_state_home {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(home?).join(".local/state"),
    };
    Some(base.join("claude").join("gate-events.jsonl"))
}

/// `decision` は `"deny"` か `"skip"`。呼び出し側の都合で失敗は無視してよい
/// 設計(記録は best-effort — gate 自体の判定を記録の成否に依存させない)。
pub fn record(path: &Path, gate: &str, decision: &str) -> std::io::Result<()> {
    let ts_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rec = Record {
        ts_unix,
        gate,
        decision,
    };
    let line = serde_json::to_string(&rec)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn default_path_prefers_xdg_state_home() {
        assert_eq!(
            default_path_with(
                Some(OsString::from("/xdg")),
                Some(OsString::from("/home/u"))
            ),
            Some(PathBuf::from("/xdg/claude/gate-events.jsonl"))
        );
    }

    #[test]
    fn default_path_falls_back_to_home() {
        assert_eq!(
            default_path_with(None, Some(OsString::from("/home/u"))),
            Some(PathBuf::from(
                "/home/u/.local/state/claude/gate-events.jsonl"
            ))
        );
    }

    #[test]
    fn default_path_none_without_home() {
        assert_eq!(default_path_with(None, None), None);
    }

    #[test]
    fn record_appends_jsonl_line() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state").join("gate-events.jsonl");
        record(&path, "new-tool-guard", "deny").unwrap();
        record(&path, "new-tool-guard", "skip").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let v0: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v0["gate"], "new-tool-guard");
        assert_eq!(v0["decision"], "deny");
        assert!(v0["ts_unix"].is_u64());
    }
}
