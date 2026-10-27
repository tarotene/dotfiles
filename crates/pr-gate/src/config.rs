//! 環境変数からの設定(bash 版の冒頭の `${PR_GATE_*:-既定値}` 群)。

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    /// `PR_GATE_DIR`(既定 `~/.claude/pr-gate`)。`skip` と `state/` の置き場所。
    pub state_root: PathBuf,
    /// `PR_GATE_ALLOWLIST`(既定 `~/.claude/pr-gate-repos`)。
    pub allowlist: PathBuf,
    /// `PR_GATE_CI_TIMEOUT`(既定 300 秒): Stop で CI の判定を待つ上限
    /// (`pr-checks-wait` は `--timeout` で別に持つ)。
    pub ci_timeout: Duration,
    /// `PR_GATE_POLL_INTERVAL`(既定 10 秒): 判定のためにチェックを取り直す間隔。
    pub poll: Duration,
    /// `PR_GATE_CHECK_APPEAR_TIMEOUT`(既定 60 秒): チェックの出現待ちの上限。
    pub check_appear_timeout: u64,
    /// `PR_GATE_QUIESCE`(既定 15 秒): quiesce モードで「揃った」とみなす静止時間。
    pub quiesce: u64,
    /// `PR_GATE_FETCH_TTL`(既定 600 秒): SessionStart で fetch を省く FETCH_HEAD の鮮度。
    pub fetch_ttl: u64,
    /// `PR_GATE_MAX_BLOCKS`(既定 6): escalate までの block 回数。
    ///
    /// 上限が 5 でなく 6 なのは G_stack を足したから: 最悪の連鎖(push → G_pr →
    /// 本文修正 → stack link → CI 待ち)が正当に 5 回 block しうるので、5 のままだと
    /// 最後の 1 回が escalate に化ける(4→5 に上げた G_pr 追加時と同じ論法)。なお
    /// G_unpushed の block メッセージは push と `gh pr create` を 1 往復に
    /// まとめて案内するため、この最悪連鎖は実際には起きにくい。
    pub max_blocks: u64,
    /// `SKIP_PR_GATE=1`。
    pub skip_env: bool,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn env_secs(name: &str, default: f64) -> f64 {
    env_nonempty(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

fn env_num(name: &str, default: u64) -> u64 {
    env_nonempty(name)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        Config {
            state_root: env_nonempty("PR_GATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/pr-gate")),
            allowlist: env_nonempty("PR_GATE_ALLOWLIST")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude/pr-gate-repos")),
            ci_timeout: Duration::from_secs(env_num("PR_GATE_CI_TIMEOUT", 300)),
            poll: Duration::from_secs_f64(env_secs("PR_GATE_POLL_INTERVAL", 10.0)),
            check_appear_timeout: env_num("PR_GATE_CHECK_APPEAR_TIMEOUT", 60),
            quiesce: env_num("PR_GATE_QUIESCE", 15),
            fetch_ttl: env_num("PR_GATE_FETCH_TTL", 600),
            max_blocks: env_num("PR_GATE_MAX_BLOCKS", 6),
            skip_env: env_nonempty("SKIP_PR_GATE").as_deref() == Some("1"),
        }
    }

    pub fn state_dir(&self) -> PathBuf {
        self.state_root.join("state")
    }

    /// `[[ -f "$STATE_ROOT/skip" ]] || [[ "${SKIP_PR_GATE:-0}" == "1" ]]`。
    pub fn skipped(&self) -> bool {
        self.state_root.join("skip").is_file() || self.skip_env
    }
}
