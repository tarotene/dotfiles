//! block 回数 / escalated フラグ(`state/<sid>.count` / `state/<sid>.escalated`)。
//!
//! 上限は独自カウンタ: `state/<sid>.count` が `PR_GATE_MAX_BLOCKS` に達したら
//! 1 回だけ escalate し、`state/<sid>.escalated` を作る。以後そのセッションは
//! 無条件で素通る(escalated のチェックは上限判定より前 —
//! docs/agents/copilot-plan-review.md の closer と同じ置き方)。
//!
//! 同じ文面の block が続いたときは、上限を待たず escalate する(`state/<sid>.last` に、直前の文面の
//! ハッシュと連続回数を持つ。`PR_GATE_SAME_BLOCK_LIMIT`、F9)。

use crate::config::Config;
use agent_session::SessionLedger;
use agent_session::StopOutcome;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
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
    /// 直前の block の文面のハッシュと、同じ文面の連続回数(`<hash> <回数>`)。
    last: SessionLedger,
    max_blocks: u64,
    same_block_limit: u64,
}

impl State {
    pub fn new(cfg: &Config) -> Self {
        State {
            count: SessionLedger::new(cfg.state_dir(), "count"),
            escalated: SessionLedger::new(cfg.state_dir(), "escalated"),
            last: SessionLedger::new(cfg.state_dir(), "last"),
            max_blocks: cfg.max_blocks,
            same_block_limit: cfg.same_block_limit,
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

    /// 直前の block と同じ文面が、連続して何回目か(今回を含む)。文面が変われば 1。数え直しの記録も更新する。
    fn repeat_count(&self, sid: &str, message: &str) -> u64 {
        let mut h = DefaultHasher::new();
        message.hash(&mut h);
        let now = format!("{:016x}", h.finish());
        let prev = fs::read_to_string(self.last.path(sid)).unwrap_or_default();
        let mut it = prev.split_whitespace();
        let repeat = match (it.next(), it.next().and_then(|n| n.parse::<u64>().ok())) {
            (Some(p), Some(n)) if p == now => n + 1,
            _ => 1,
        };
        let _ = fs::write(self.last.path(sid), format!("{now} {repeat}"));
        repeat
    }

    /// block を返す。上限に達したら escalate 文言に差し替え、escalated フラグを立てる。
    /// 上限は 2 つ: block の総回数(`max_blocks`)と、同じ文面の連続回数(`same_block_limit`。0 なら使わない)。
    /// 理由は、モデルへ渡す文(`StopOutcome::Block` の `reason`)。
    pub fn block_or_escalate(&self, sid: &str, message: &str) -> StopOutcome {
        let n = self.read_count(sid) + 1;
        let _ = fs::write(self.count_path(sid), n.to_string());
        let repeat = self.repeat_count(sid, message);
        if n >= self.max_blocks {
            let _ = fs::write(self.escalated.path(sid), "");
            StopOutcome::block(format!(
                "{max} 回ブロックしましたが解消していません。残存:\n{message}\n\nAskUserQuestion で GO/NO-GO を取ってから終了してください。\n(次回以降このゲートは素通りします)",
                max = self.max_blocks
            ))
        } else if self.same_block_limit > 0 && repeat >= self.same_block_limit {
            let _ = fs::write(self.escalated.path(sid), "");
            StopOutcome::block(format!(
                "同じ内容で {repeat} 回続けてブロックしましたが解消していません(直す手が尽きた可能性があります。外部の障害でコードでは直せない赤など)。残存:\n{message}\n\nAskUserQuestion で GO/NO-GO を取ってから終了してください。\n(次回以降このゲートは素通りします)"
            ))
        } else {
            StopOutcome::block(message)
        }
    }
}
