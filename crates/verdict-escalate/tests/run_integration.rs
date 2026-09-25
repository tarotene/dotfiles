//! `verdict_escalate::run()` の統合テスト: レッジャー dir → inbox 追記 →
//! stamp 台帳までの一連の副作用。`wrapup-stop-gate.sh --add` は
//! `WRAPUP_STOP_GATE_BIN` で差し替えた最小スタブ(受け取った行のタイムスタンプ
//! を固定文字列に正規化してから追記する)で代替する — 実体は
//! `config/claude/hooks/wrapup-stop-gate.sh --add`(flock + jq -ce での
//! compact 化)そのものを別途 shellcheck/selftest で検証している。
//!
//! 各テストは環境変数(プロセスグローバル)を触るため、`serial_test` を
//! 使わずシングルスレッド実行を強制する(`--test-threads=1`)代わりに、
//! 各テスト内で毎回全変数を明示的に上書き・復元する(既存の gh-edit-allow
//! テストはプロセス env に依存しないためこの問題が無い — ここが唯一の
//! 例外)。

use std::fs;
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn write_stub(dir: &Path) -> std::path::PathBuf {
    let stub = dir.join("wrapup-stop-gate-stub.sh");
    fs::write(
        &stub,
        "#!/usr/bin/env bash\n\
set -euo pipefail\n\
if [[ \"$1\" == \"--add\" ]]; then\n\
  line=\"$3\"\n\
  line=\"$(printf '%s' \"$line\" | sed -E 's/\"ts\":\"[^\"]*\"/\"ts\":\"FIXED\"/')\"\n\
  printf '%s\\n' \"$line\" >> \"$2\"\n\
fi\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    stub
}

fn record_line(session_id: &str, term_hash: &str) -> String {
    serde_json::json!({
        "v": 1,
        "ts": "2026-09-25T00:00:00Z",
        "tool": "bleep",
        "tool_version": "0.2.0",
        "repo": "tarotene/bleep",
        "host": "claude",
        "session_id": session_id,
        "verdict": "deny",
        "reason_id": "repo-ref",
        "match_class": "plain",
        "term_hash": term_hash,
        "tool_name": "Bash",
    })
    .to_string()
}

struct Env {
    _guard: std::sync::MutexGuard<'static, ()>,
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let verdicts = tmp.path().join("verdicts");
        let state = tmp.path().join("state");
        fs::create_dir_all(&verdicts).unwrap();
        fs::create_dir_all(&state).unwrap();
        let stub = write_stub(tmp.path());
        std::env::set_var("AGENT_VERDICTS_DIR", &verdicts);
        std::env::set_var("VERDICT_ESCALATE_STATE_DIR", &state);
        std::env::set_var("WRAPUP_STOP_GATE_BIN", &stub);
        Self { _guard: guard, tmp }
    }

    fn write_ledger(&self, tool: &str, lines: &[String]) {
        fs::write(
            self.tmp
                .path()
                .join("verdicts")
                .join(format!("{tool}.jsonl")),
            lines.join("\n") + "\n",
        )
        .unwrap();
    }

    fn inbox_path(&self) -> std::path::PathBuf {
        self.tmp.path().join("inbox.jsonl")
    }

    fn inbox_contents(&self) -> String {
        fs::read_to_string(self.inbox_path()).unwrap_or_default()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var("AGENT_VERDICTS_DIR");
        std::env::remove_var("VERDICT_ESCALATE_STATE_DIR");
        std::env::remove_var("WRAPUP_STOP_GATE_BIN");
    }
}

#[test]
fn escalates_at_threshold_and_dedups_on_rerun() {
    let env = Env::new();
    env.write_ledger(
        "bleep",
        &[
            record_line("sess-1", "hashA"),
            record_line("sess-1", "hashA"),
            record_line("sess-1", "hashA"),
        ],
    );
    let inbox = env.inbox_path();

    let added = verdict_escalate::run("sess-1", &inbox);
    assert_eq!(added, 1);
    let contents = env.inbox_contents();
    assert!(contents.contains("\"repo\":\"tarotene/bleep\""));
    assert!(contents.contains("\"go\":\"ask\""));
    assert!(contents.contains("\"ts\":\"FIXED\""));
    assert_eq!(contents.lines().count(), 1);

    // 再実行しても stamp 済みなので追記しない。
    let added_again = verdict_escalate::run("sess-1", &inbox);
    assert_eq!(added_again, 0);
    assert_eq!(env.inbox_contents().lines().count(), 1);
}

#[test]
fn below_threshold_does_not_escalate() {
    let env = Env::new();
    env.write_ledger(
        "bleep",
        &[
            record_line("sess-1", "hashA"),
            record_line("sess-1", "hashA"),
        ],
    );
    let inbox = env.inbox_path();
    assert_eq!(verdict_escalate::run("sess-1", &inbox), 0);
    assert!(!inbox.exists());
}

#[test]
fn other_sessions_are_ignored() {
    let env = Env::new();
    env.write_ledger(
        "bleep",
        &[
            record_line("sess-1", "hashA"),
            record_line("sess-1", "hashA"),
            record_line("sess-1", "hashA"),
        ],
    );
    let inbox = env.inbox_path();
    assert_eq!(verdict_escalate::run("sess-2", &inbox), 0);
    assert!(!inbox.exists());
}
