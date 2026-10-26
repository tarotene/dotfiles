//! 新規 Rust gate の deny/skip イベントを JSONL に記録する(ADR-543 段3、
//! 降格候補検出の入力)。既存 bash gate には計装しない —
//! bash→Rust 移行時に hook-io 側で拾う方が二度手間にならない(ADR-543 の
//! `## 先行例との対比` D6 参照)。
//!
//! フォーマットは1行1レコードの JSON(`{ts_unix, gate, decision}`)。
//! `decision` は `"deny"`・`"skip"`・`"fail-open"`・`"pass"` の 4 値。
//! `fail-open` は、gate が依存先の不調(外部 CLI・モデル・認証)で何も判定できずに
//! 素通りしたこと、`pass` は、判定が成立して通ったこと(#822)。同じ gate の最後の
//! `pass` より後の `fail-open` を数えれば「壊れたまま素通りし続けている」ことが
//! 状態ファイル無しで分かる([`trailing_fail_open`])。`promotion-detect` は
//! 知らない `decision` を読み飛ばすので、語彙を足しても降格候補の検出は変わらない。

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

/// `decision` は `"deny"`・`"skip"`・`"fail-open"`・`"pass"`。呼び出し側の都合で失敗は無視してよい
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

/// 同じ gate の、最後の `pass` より後の `fail-open` の数(#822)。
///
/// 末尾から読み、同じ gate の `pass` に当たったら止まる。別の gate の記録と、同じ gate の
/// `deny` / `skip` は、加算もリセットもしない。壊れた行・`gate` や `decision` の無い行は
/// 読み飛ばす。ファイルが無い・読めないときは 0(状態ファイルは持たず、ログの末尾から導く)。
pub fn trailing_fail_open(path: &Path, gate: &str) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mut n = 0;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("gate").and_then(|g| g.as_str()) != Some(gate) {
            continue;
        }
        match v.get("decision").and_then(|d| d.as_str()) {
            Some("pass") => break,
            Some("fail-open") => n += 1,
            _ => {}
        }
    }
    n
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

    fn write_events(path: &Path, events: &[(&str, &str)]) {
        for (gate, decision) in events {
            record(path, gate, decision).unwrap();
        }
    }

    #[test]
    fn trailing_fail_open_counts_since_the_last_pass() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("e.jsonl");
        write_events(
            &p,
            &[
                ("g", "fail-open"),
                ("g", "pass"),
                ("g", "fail-open"),
                ("g", "fail-open"),
            ],
        );
        assert_eq!(
            trailing_fail_open(&p, "g"),
            2,
            "最後の pass より後だけを数える"
        );
    }

    #[test]
    fn trailing_fail_open_is_reset_by_a_pass() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("e.jsonl");
        write_events(&p, &[("g", "fail-open"), ("g", "fail-open"), ("g", "pass")]);
        assert_eq!(trailing_fail_open(&p, "g"), 0);
    }

    #[test]
    fn trailing_fail_open_ignores_other_gates_and_other_decisions() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("e.jsonl");
        write_events(
            &p,
            &[
                ("g", "fail-open"),
                ("other", "pass"),
                ("g", "deny"),
                ("other", "fail-open"),
                ("g", "fail-open"),
                ("g", "skip"),
            ],
        );
        assert_eq!(
            trailing_fail_open(&p, "g"),
            2,
            "別の gate の pass と、同じ gate の deny / skip はリセットも加算もしない"
        );
    }

    #[test]
    fn trailing_fail_open_tolerates_missing_file_and_garbage_lines() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(trailing_fail_open(&d.path().join("none.jsonl"), "g"), 0);
        let p = d.path().join("e.jsonl");
        write_events(&p, &[("g", "fail-open")]);
        std::fs::write(
            &p,
            format!(
                "{}\nnot json\n\n{{\"gate\":\"g\"}}\n",
                std::fs::read_to_string(&p).unwrap().trim_end()
            ),
        )
        .unwrap();
        assert_eq!(
            trailing_fail_open(&p, "g"),
            1,
            "壊れた行と decision の無い行は読み飛ばす"
        );
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
