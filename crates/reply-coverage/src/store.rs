//! 機体ローカルの状態: 会話の全文(`threads/`)と、検査の記録(`records.jsonl`)。
//!
//! 置き場は `${XDG_STATE_HOME:-$HOME/.local/state}/claude/reply-coverage`(Git の外・vault の外。
//! vault は LiveSync でスマホに同期されるので、第三者のメッセージの全文を置かない。ADR-1256 D4)。
//! テストでは環境変数 `REPLY_COVERAGE_STATE_DIR` で差し替える。正本は媒体(Gmail など)のままで、
//! ここは複製にすぎない(有効期限 [`TTL_SECS`] で掃く)。

use crate::text;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 会話の全文の有効期限(60 日)。
pub const TTL_SECS: u64 = 60 * 24 * 3600;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 保存した会話。`format` は `normalized`(`## them|me|other` の見出しでメッセージを区切った形)か
/// `raw`(ブラウザの snapshot など、整形していない形。役割は検出の側が読み取る)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredThread {
    /// `<媒体>:<会話の id>`(例 `gmail:19abc…`・`linkedin:<会話を指す短い印>`)。
    pub id: String,
    pub fetched_ts: u64,
    pub format: String,
    /// この会話に属するメッセージの id(Gmail の返信の `replyToMessageId` から会話を引く索引)。
    #[serde(default)]
    pub message_ids: Vec<String>,
    pub text: String,
}

/// 検査の記録 1 件。`verdict` は `pass` か `fail`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub ts_unix: u64,
    pub thread: String,
    /// 検査した時点の会話の全文の sha。会話を取り直して中身が変われば、記録は合わなくなる。
    pub thread_sha: String,
    /// 検査した下書きの sha([`text::sha`])。
    pub draft_sha: String,
    pub verdict: String,
    pub model: String,
    pub asks: usize,
    /// 答えていない依頼の引用(不合格の理由を、関所の拒否文に出すため)。
    #[serde(default)]
    pub unanswered: Vec<String>,
    /// 前の返信が落としていた依頼に答えるのに、軽い詫びが無かった。
    #[serde(default)]
    pub apology_missing: bool,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// 環境変数 `REPLY_COVERAGE_STATE_DIR`、無ければ XDG の状態ディレクトリの下。`HOME` が無ければ `None`。
    pub fn from_env() -> Option<Store> {
        if let Some(d) = process_env::var_nonempty("REPLY_COVERAGE_STATE_DIR") {
            return Some(Store::at(d));
        }
        Some(Store::at(
            process_env::Xdg::State
                .resolve()?
                .join("claude")
                .join("reply-coverage"),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn threads_dir(&self) -> PathBuf {
        self.root.join("threads")
    }

    fn records_path(&self) -> PathBuf {
        self.root.join("records.jsonl")
    }

    /// 検出の `claude -p` を動かす作業ディレクトリ(空。プロジェクトの設定・CLAUDE.md を拾わせない)。
    pub fn cwd(&self) -> std::io::Result<PathBuf> {
        let d = self.root.join("cwd");
        mkdir_private(&self.root)?;
        mkdir_private(&d)?;
        Ok(d)
    }

    fn thread_path(&self, id: &str) -> PathBuf {
        let safe: String = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(60)
            .collect();
        // 記号を `_` に畳むと別の id が同じ名前になりうるので、id そのものの sha を添える。
        self.threads_dir()
            .join(format!("{safe}-{}.json", &text::sha(id)[..8]))
    }

    pub fn save_thread(&self, t: &StoredThread) -> std::io::Result<()> {
        mkdir_private(&self.root)?;
        mkdir_private(&self.threads_dir())?;
        let body = serde_json::to_string(t).map_err(std::io::Error::other)?;
        write_private(&self.thread_path(&t.id), body.as_bytes())
    }

    pub fn load_thread(&self, id: &str) -> Option<StoredThread> {
        let s = fs::read_to_string(self.thread_path(id)).ok()?;
        serde_json::from_str(&s).ok()
    }

    fn all_threads(&self) -> Vec<StoredThread> {
        let Ok(rd) = fs::read_dir(self.threads_dir()) else {
            return Vec::new();
        };
        rd.filter_map(|e| fs::read_to_string(e.ok()?.path()).ok())
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// メッセージの id から、保存済みの会話を引く(新しく取り直した会話を優先する)。
    pub fn thread_for_message(&self, message_id: &str) -> Option<StoredThread> {
        self.all_threads()
            .into_iter()
            .filter(|t| t.message_ids.iter().any(|m| m == message_id))
            .max_by_key(|t| t.fetched_ts)
    }

    pub fn append_record(&self, r: &Record) -> std::io::Result<()> {
        mkdir_private(&self.root)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(self.records_path())?;
        let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        writeln!(f, "{line}")
    }

    /// 記録を古い順に返す。読めない行は飛ばす。
    pub fn records(&self) -> Vec<Record> {
        let Ok(s) = fs::read_to_string(self.records_path()) else {
            return Vec::new();
        };
        s.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// 期限を過ぎた会話の全文と記録を消す。消した会話の数を返す。
    pub fn sweep(&self, now: u64, ttl_secs: u64) -> usize {
        let mut removed = 0;
        if let Ok(rd) = fs::read_dir(self.threads_dir()) {
            for e in rd.flatten() {
                let expired = fs::read_to_string(e.path())
                    .ok()
                    .and_then(|s| serde_json::from_str::<StoredThread>(&s).ok())
                    .is_none_or(|t| now.saturating_sub(t.fetched_ts) > ttl_secs);
                if expired && fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        let kept: Vec<String> = self
            .records()
            .into_iter()
            .filter(|r| now.saturating_sub(r.ts_unix) <= ttl_secs)
            .filter_map(|r| serde_json::to_string(&r).ok())
            .collect();
        if self.records_path().exists() {
            let mut body = kept.join("\n");
            if !body.is_empty() {
                body.push('\n');
            }
            let _ = write_private(&self.records_path(), body.as_bytes());
        }
        removed
    }
}

fn mkdir_private(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

fn write_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, ts: u64, msgs: &[&str]) -> StoredThread {
        StoredThread {
            id: id.to_string(),
            fetched_ts: ts,
            format: "normalized".to_string(),
            message_ids: msgs.iter().map(|s| s.to_string()).collect(),
            text: format!("# thread: {id}\n## them 2026-01-01\nhello"),
        }
    }

    fn record(thread: &str, ts: u64, verdict: &str) -> Record {
        Record {
            ts_unix: ts,
            thread: thread.to_string(),
            thread_sha: "t".to_string(),
            draft_sha: "d".to_string(),
            verdict: verdict.to_string(),
            model: "m".to_string(),
            asks: 1,
            unanswered: vec![],
            apology_missing: false,
        }
    }

    #[test]
    fn thread_roundtrip_and_private_modes() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path().join("rc"));
        let t = thread("gmail:abc", 100, &["m1", "m2"]);
        s.save_thread(&t).unwrap();
        assert_eq!(s.load_thread("gmail:abc"), Some(t));
        assert_eq!(s.load_thread("gmail:other"), None);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(s.root()), 0o700);
        assert_eq!(mode(&s.thread_path("gmail:abc")), 0o600);
    }

    #[test]
    fn ids_that_differ_only_in_symbols_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        s.save_thread(&thread("a:b/c", 1, &[])).unwrap();
        s.save_thread(&thread("a:b?c", 2, &[])).unwrap();
        assert_eq!(s.load_thread("a:b/c").unwrap().fetched_ts, 1);
        assert_eq!(s.load_thread("a:b?c").unwrap().fetched_ts, 2);
    }

    #[test]
    fn message_id_finds_the_newest_snapshot_of_its_thread() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        s.save_thread(&thread("gmail:t1", 10, &["m1"])).unwrap();
        s.save_thread(&thread("gmail:t2", 20, &["m2", "m3"]))
            .unwrap();
        assert_eq!(s.thread_for_message("m3").unwrap().id, "gmail:t2");
        assert!(s.thread_for_message("nope").is_none());
    }

    #[test]
    fn records_append_and_skip_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        s.append_record(&record("gmail:t", 1, "pass")).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(s.records_path())
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        s.append_record(&record("gmail:t", 2, "fail")).unwrap();
        let rs = s.records();
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[1].verdict, "fail");
    }

    #[test]
    fn sweep_removes_only_expired_threads_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        s.save_thread(&thread("gmail:old", 100, &[])).unwrap();
        s.save_thread(&thread("gmail:new", 1_000_000, &[])).unwrap();
        s.append_record(&record("gmail:old", 100, "pass")).unwrap();
        s.append_record(&record("gmail:new", 1_000_000, "pass"))
            .unwrap();
        let removed = s.sweep(1_000_100, 1000);
        assert_eq!(removed, 1);
        assert!(s.load_thread("gmail:old").is_none());
        assert!(s.load_thread("gmail:new").is_some());
        assert_eq!(s.records().len(), 1);
    }
}
