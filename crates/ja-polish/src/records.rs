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
    /// 素通しの原因が、混雑・時間切れのような一時的なもの(再依頼や予備モデルでも通せなかった)か。
    /// キーの未設定のような恒久的な原因や、照合の外れでは立てない。古い記録には無いので `false`。
    #[serde(default, skip_serializing_if = "is_false")]
    pub transient: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Entry {
    pub fn polished(sha: String, model: &str) -> Self {
        Entry {
            ts_unix: now(),
            sha,
            state: "polished".to_string(),
            model: model.to_string(),
            reason: None,
            transient: false,
        }
    }

    pub fn skipped(sha: String, model: &str, reason: &str) -> Self {
        Entry {
            ts_unix: now(),
            sha,
            state: "skipped".to_string(),
            model: model.to_string(),
            reason: Some(reason.to_string()),
            transient: false,
        }
    }

    /// 素通しの原因が一時的なもの([`Entry::transient`])だと印を付ける。
    pub fn with_transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
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

/// 記録済みの `sha` の集合(書き直した段落も、素通しの段落も含む)。ファイルが無い・読めない行は
/// 空・読み飛ばし(関所の判定を記録の不調で止めない)。**関所(投稿・Stop)と `--check` が使う**:
/// 素通しの段落で止めると、動けない間に何も出せなくなるので、素通しも「通した」とみなす。
pub fn load(path: &Path) -> HashSet<String> {
    load_where(path, |_| true)
}

/// 書き直した(`state` が `polished`)段落の `sha` の集合。**`ja-polish` コマンドの書き直しが使う**:
/// 素通しの段落は、動けない間(キーが無い・混雑・時間切れ)に元の文のまま通しただけなので、次に
/// 実行したときに書き直しの対象へ戻す。素通しも「通した」とみなす [`load`] のままだと、直ったあとも
/// 文面を変えない限り二度と書き直されない(2026-11-12 に、時間切れで素通しにした 10 段落で実測した)。
pub fn load_polished(path: &Path) -> HashSet<String> {
    load_where(path, |e| e.state == "polished")
}

fn load_where(path: &Path, keep: impl Fn(&Entry) -> bool) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .filter(|e| keep(e))
        .map(|e| e.sha)
        .collect()
}

/// 混雑・時間切れによる素通しの率を見る期間(秒)。7 日。
pub const TRANSIENT_WINDOW: u64 = 7 * 24 * 60 * 60;

/// 率を判定に使う、期間内の最少の段落数。数件の記録で 1〜2 件が素通しになっただけで率が跳ね上がり、
/// 課金の検討を知らせすぎないための下限。
pub const TRANSIENT_MIN_SAMPLES: usize = 20;

/// 期間内の記録の数と、そのうち一時的な原因(混雑・時間切れ)で通せなかった素通しの数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransientRate {
    pub transient: usize,
    pub total: usize,
}

impl TransientRate {
    /// 一時的な原因の素通しの割合(%)。記録が無ければ 0。
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.transient as f64 * 100.0 / self.total as f64
        }
    }
}

/// `now_unix` から `window_secs` 秒さかのぼった期間の記録を数える。キーの未設定や照合の外れによる
/// 素通しは、`transient` の印が無いので、分子に入らない(分母には入る)。`transient` の印を持たない古い記録も
/// 同じ。ファイルが無い・読めない行は数えない。
pub fn transient_rate(path: &Path, now_unix: u64, window_secs: u64) -> TransientRate {
    let Ok(text) = std::fs::read_to_string(path) else {
        return TransientRate::default();
    };
    let since = now_unix.saturating_sub(window_secs);
    let mut rate = TransientRate::default();
    for e in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .filter(|e| e.ts_unix >= since)
    {
        rate.total += 1;
        if e.transient {
            rate.transient += 1;
        }
    }
    rate
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
            transient: false,
        }
    }

    #[test]
    fn transient_flag_is_written_only_when_set_and_old_lines_read_as_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        append(
            &path,
            &[
                Entry::skipped("a".into(), "m", "混雑").with_transient(true),
                Entry::skipped("b".into(), "m", "キー未設定"),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("\"transient\":true"), "{}", lines[0]);
        assert!(!lines[1].contains("transient"), "{}", lines[1]);
        let old: Entry = serde_json::from_str(
            "{\"ts_unix\":1,\"sha\":\"x\",\"state\":\"skipped\",\"model\":\"m\"}",
        )
        .unwrap();
        assert!(!old.transient);
    }

    #[test]
    fn transient_rate_counts_only_the_window_and_only_marked_skips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        let now = 1_000_000u64;
        let at = |ts: u64, e: Entry| Entry { ts_unix: ts, ..e };
        append(
            &path,
            &[
                // 期間の外(8 日前): 数えない。
                at(
                    now - 8 * 86_400,
                    Entry::skipped("old".into(), "m", "混雑").with_transient(true),
                ),
                // 期間の中: 書き直し 2、混雑の素通し 1、キー未設定の素通し 1(分母だけに入る)。
                at(now - 86_400, Entry::polished("a".into(), "m")),
                at(now - 3_600, Entry::polished("b".into(), "m")),
                at(
                    now - 600,
                    Entry::skipped("c".into(), "m", "混雑").with_transient(true),
                ),
                at(now - 60, Entry::skipped("d".into(), "m", "No key found")),
            ],
        )
        .unwrap();
        let r = transient_rate(&path, now, TRANSIENT_WINDOW);
        assert_eq!(
            r,
            TransientRate {
                transient: 1,
                total: 4
            }
        );
        assert!((r.percent() - 25.0).abs() < 1e-9);
        assert_eq!(
            transient_rate(&dir.path().join("none.jsonl"), now, TRANSIENT_WINDOW),
            TransientRate::default()
        );
        assert_eq!(TransientRate::default().percent(), 0.0);
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
    fn load_polished_leaves_out_skipped_paragraphs_but_load_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        append(
            &path,
            &[
                Entry::polished("done".into(), "m"),
                Entry::skipped("passed-through".into(), "m", "時間切れ").with_transient(true),
            ],
        )
        .unwrap();
        // 関所の見方: 素通しも通した。
        let all = load(&path);
        assert!(all.contains("done") && all.contains("passed-through"));
        // 書き直しの見方: 素通しはもう一度対象にする。
        let polished = load_polished(&path);
        assert!(polished.contains("done") && !polished.contains("passed-through"));
        assert!(load_polished(&dir.path().join("none.jsonl")).is_empty());
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
