//! 旧 `codex-plan-gate.sh --selftest` の 4 ケース + 縮退・エスケープハッチ。
//!
//! 子の plan-scope-gate / plan-precedent-gate は PATH ではなく
//! `CODEX_PLAN_GATE_CLAUDE_HOOKS_DIR` 配下のスタブにする(bash 版 selftest と同じ)。
//! `CODEX_PLAN_GATE_UNDER_TEST` に実行ファイルを渡すと bash 版に同じケースを流せる。

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn under_test() -> PathBuf {
    std::env::var_os("CODEX_PLAN_GATE_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-plan-gate")))
}

fn stub(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct T {
    dir: tempfile::TempDir,
}

impl T {
    fn new() -> Self {
        let t = T {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(t.hooks()).unwrap();
        // scope-gate: `mode` ファイルがあれば指摘(exit 1)、無ければ OK。
        let mode = t.dir.path().join("mode");
        stub(
            &t.hooks().join("plan-scope-gate"),
            &format!(
                "if [[ -e '{}' ]]; then echo 'R1: 処分が未記載です'; exit 1; fi\necho 'OK: 検査を通過しました。'",
                mode.display()
            ),
        );
        stub(
            &t.hooks().join("plan-precedent-gate"),
            "echo 'OK: 検査を通過しました。'",
        );
        t
    }
    fn hooks(&self) -> PathBuf {
        self.dir.path().join("claude-hooks")
    }
    fn state_root(&self) -> PathBuf {
        self.dir.path().join("state-root")
    }
    fn failing(&self, on: bool) {
        let mode = self.dir.path().join("mode");
        if on {
            fs::write(mode, "").unwrap();
        } else {
            let _ = fs::remove_file(mode);
        }
    }
    fn run_env(&self, input: &str, envs: &[(&str, &str)]) -> String {
        let mut cmd = Command::new(under_test());
        cmd.env("CODEX_PLAN_GATE_DIR", self.state_root())
            .env("CODEX_PLAN_GATE_CLAUDE_HOOKS_DIR", self.hooks())
            .env("TMPDIR", self.dir.path())
            .env_remove("SKIP_CODEX_PLAN_GATE")
            .env_remove("CODEX_PLAN_GATE_MAX_BLOCKS");
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // 早期終了(skip など)では stdin を読まないので EPIPE は無視する。
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook は常に exit 0");
        String::from_utf8(out.stdout).unwrap()
    }
    fn run(&self, input: &str) -> String {
        self.run_env(input, &[])
    }
}

fn with_plan() -> String {
    serde_json::json!({
        "hook_event_name": "Stop", "session_id": "sid1",
        "last_assistant_message": "before\n<proposed_plan>\n## 要求インベントリ\n- R1: x — 段1で実装\n</proposed_plan>\nafter",
    })
    .to_string()
}

fn without_plan() -> String {
    serde_json::json!({
        "hook_event_name": "Stop", "session_id": "sid2",
        "last_assistant_message": "no plan here",
    })
    .to_string()
}

fn decision(out: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(out)
        .ok()?
        .get("decision")?
        .as_str()
        .map(String::from)
}

/// selftest 1: proposed_plan 無し → 無出力
#[test]
fn no_proposed_plan_is_silent() {
    let t = T::new();
    assert_eq!(t.run(&without_plan()), "");
}

/// selftest 2: proposed_plan あり、両 gate OK → 無出力
#[test]
fn both_gates_ok_is_silent() {
    let t = T::new();
    assert_eq!(t.run(&with_plan()), "");
}

/// selftest 3: proposed_plan あり、scope-gate が指摘 → block
#[test]
fn scope_gate_finding_blocks() {
    let t = T::new();
    t.failing(true);
    let out = t.run(&with_plan());
    assert_eq!(decision(&out).as_deref(), Some("block"), "出力={out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.ends_with("--- plan-scope-gate ---\nR1: 処分が未記載です\n\n"));
    assert!(!reason.contains("plan-precedent-gate"));
}

/// selftest 4: 上限到達後は escalate して無条件で通す
#[test]
fn escalates_after_max_blocks() {
    let t = T::new();
    t.failing(true);
    assert_eq!(decision(&t.run(&with_plan())).as_deref(), Some("block"));
    let mut out = String::new();
    for _ in 0..5 {
        out = t.run(&with_plan());
    }
    assert_eq!(out, "", "escalate 後は無出力");
    assert!(t.state_root().join("state/sid1.escalated").exists());
}

#[test]
fn max_blocks_env_is_honoured() {
    let t = T::new();
    t.failing(true);
    // 上限 1 → 最初の失敗で即 escalate(block を出さない)。
    assert_eq!(
        t.run_env(&with_plan(), &[("CODEX_PLAN_GATE_MAX_BLOCKS", "1")]),
        ""
    );
    assert!(t.state_root().join("state/sid1.escalated").exists());
}

#[test]
fn skip_file_and_env_bypass() {
    let t = T::new();
    t.failing(true);
    assert_eq!(
        t.run_env(&with_plan(), &[("SKIP_CODEX_PLAN_GATE", "1")]),
        ""
    );
    fs::create_dir_all(t.state_root()).unwrap();
    fs::write(t.state_root().join("skip"), "").unwrap();
    assert_eq!(t.run(&with_plan()), "");
}

#[test]
fn degrades_silently() {
    let t = T::new();
    t.failing(true);
    // Stop 以外のイベント / 壊れた stdin / 空メッセージ / 開きタグ直後に何も無い。
    let other = serde_json::json!({"hook_event_name":"PreToolUse","last_assistant_message":"<proposed_plan>x</proposed_plan>"}).to_string();
    assert_eq!(t.run(&other), "");
    assert_eq!(t.run("{not json"), "");
    let empty =
        serde_json::json!({"hook_event_name":"Stop","last_assistant_message":""}).to_string();
    assert_eq!(t.run(&empty), "");
    let blank = serde_json::json!({"hook_event_name":"Stop","last_assistant_message":"<proposed_plan>\n\n</proposed_plan>"}).to_string();
    assert_eq!(t.run(&blank), "");
}

#[test]
fn missing_gate_binary_degrades() {
    let t = T::new();
    t.failing(true);
    fs::remove_file(t.hooks().join("plan-precedent-gate")).unwrap();
    assert_eq!(t.run(&with_plan()), "");
}

#[test]
fn both_gates_failing_reports_both_sections() {
    let t = T::new();
    t.failing(true);
    stub(
        &t.hooks().join("plan-precedent-gate"),
        "echo 'D1: 出典なし' >&2; exit 2",
    );
    let out = t.run(&with_plan());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.contains("--- plan-scope-gate ---\nR1: 処分が未記載です\n\n--- plan-precedent-gate ---\nD1: 出典なし"));
    // 末尾改行は落ちる($(...) と同じ)。
    assert!(!reason.ends_with('\n'));
}

#[test]
fn plan_file_is_cleaned_up() {
    let t = T::new();
    t.failing(true);
    t.run(&with_plan());
    let leftovers: Vec<_> = fs::read_dir(t.dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("codex-plan-gate.")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "一時ファイルが残っている: {leftovers:?}"
    );
}
