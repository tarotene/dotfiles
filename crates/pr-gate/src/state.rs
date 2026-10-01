//! block 回数 / escalated フラグ(`state/<sid>.count` / `state/<sid>.escalated`)。
//!
//! 上限は独自カウンタ: `state/<sid>.count` が `PR_GATE_MAX_BLOCKS` に達したら
//! 1 回だけ escalate し、`state/<sid>.escalated` を作る。以後そのセッションは
//! 無条件で素通る(escalated のチェックは上限判定より前 —
//! docs/claude/copilot-plan-review.md の closer と同じ置き方)。

use crate::config::Config;
use hook_io::SessionLedger;
use std::fs;
use std::path::{Path, PathBuf};

/// state ディレクトリを 0700 で作り、`skip` 以外のファイルを 0600 に絞る
/// (bash の `ensure_dirs`: `find "$STATE_ROOT" -maxdepth 2 -type f ! -name skip
/// -perm /077 -exec chmod 600`)。失敗は無視する。
pub fn ensure_dirs(cfg: &Config) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::create_dir_all(cfg.state_dir());
    for d in [&cfg.state_root, &cfg.state_dir()] {
        let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o700));
    }
    let tighten = |p: &Path| {
        if let Ok(m) = fs::symlink_metadata(p) {
            if m.is_file()
                && p.file_name().is_some_and(|n| n != "skip")
                && m.permissions().mode() & 0o077 != 0
            {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
            }
        }
    };
    let Ok(top) = fs::read_dir(&cfg.state_root) else {
        return;
    };
    for e in top.flatten() {
        let p = e.path();
        tighten(&p);
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            if let Ok(sub) = fs::read_dir(&p) {
                for s in sub.flatten() {
                    tighten(&s.path());
                }
            }
        }
    }
}

pub struct State {
    count: SessionLedger,
    escalated: SessionLedger,
    max_blocks: u64,
}

impl State {
    pub fn new(cfg: &Config) -> Self {
        State {
            count: SessionLedger::new(cfg.state_dir(), "count"),
            escalated: SessionLedger::new(cfg.state_dir(), "escalated"),
            max_blocks: cfg.max_blocks,
        }
    }

    fn count_path(&self, sid: &str) -> PathBuf {
        self.count.path(sid)
    }

    pub fn is_escalated(&self, sid: &str) -> bool {
        self.escalated.path(sid).is_file()
    }

    /// 数値として読めなければ 0。
    pub fn read_count(&self, sid: &str) -> u64 {
        fs::read_to_string(self.count_path(sid))
            .ok()
            .map(|s| s.trim_end_matches('\n').to_string())
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    pub fn at_limit(&self, sid: &str) -> bool {
        self.read_count(sid) >= self.max_blocks
    }

    /// block メッセージを stderr に出し、exit code 2 を返す。上限に達したら
    /// escalate 文言に差し替え、escalated フラグを立てる。
    pub fn block_or_escalate(&self, sid: &str, message: &str) -> i32 {
        let n = self.read_count(sid) + 1;
        let _ = fs::write(self.count_path(sid), n.to_string());
        if n >= self.max_blocks {
            let _ = fs::write(self.escalated.path(sid), "");
            eprint!(
                "{max} 回ブロックしましたが解消していません。残存:\n{message}\n\nAskUserQuestion で GO/NO-GO を取ってから終了してください。\n(次回以降このゲートは素通りします)\n",
                max = self.max_blocks
            );
        } else {
            eprintln!("{message}");
        }
        2
    }
}
