//! codex-plan-gate — Codex CLI の Plan mode(`<proposed_plan>` ブロック)に、
//! Claude Code の ExitPlanMode 相当の機械検査を課す Stop hook
//! (ADR-0032 Amendment #531、D1/D2。bash 版 `codex-plan-gate.sh` の移植、
//! ADR-0024 / #389 Stage 4b の残件)。
//!
//! 設計と根拠: docs/claude/codex-plan-gate.md
//!
//! Codex CLI に ExitPlanMode という UI 概念(ツールコール)は無く、Plan mode
//! の提案は応答本文に埋め込まれた `<proposed_plan>...</proposed_plan>` ブロック
//! として現れる(Codex TUI バイナリの文字列リテラルで確認、2026-09-28)。この
//! hook は Stop イベントで直前の応答(`last_assistant_message`)から
//! `<proposed_plan>` を検出し、見つかったときだけ既存の
//! `plan-scope-gate --check-plan` / `plan-precedent-gate --check` をそのまま
//! 呼ぶ。新しい判定ロジックは一切持たない(D1)。両者は Rust バイナリ
//! (crates/plan-scope-gate・crates/plan-precedent-gate、#412)で、
//! `~/.claude/hooks/` に配備済みのものを子プロセスとして直接実行する
//! (bash 版と同じ。判定エンジンを複写せず単一正本に保つ)。
//!
//! 無限 block 対策(D2): pr-gate.sh と同じ設計 — `stop_hook_active` は見ず、
//! session_id ごとの独自カウンタが上限(`CODEX_PLAN_GATE_MAX_BLOCKS`、既定 4)に
//! 達したら 1 回だけ escalate して以後そのセッションは無条件で通す。
//! `stop_hook_active` 素通しだと block 直後の再呼び出しが判定に届かない
//! (docs/claude/copilot-plan-review.md の「第二次の非収束」と同型)。
//!
//! 縮退(ADR-0005 の binary-existence gating に倣う): plan-scope-gate・
//! plan-precedent-gate 不在 / 判定不能な stdin は黙って exit 0(判定不能を
//! deny に変えない)。bash 版の「jq 不在」は Rust では起こり得ないので落とした。
//!
//! エスケープハッチ: `touch ~/.codex/codex-plan-gate/skip` または
//! `SKIP_CODEX_PLAN_GATE=1`。

use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn env_path(key: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default)
}

fn is_executable(p: &Path) -> bool {
    p.is_file()
        && fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

/// 直前の応答本文から `<proposed_plan>...</proposed_plan>` の中身だけを取り出す
/// (タグの外側は破棄)。最初の 1 ブロックのみ対象 — Codex は 1 ターンに 1 つしか
/// proposed_plan を出さない前提(TUI が「complete replacement」を要求する設計)。
/// 閉じタグが無ければ開きタグ以降すべて(bash の `${msg%%</proposed_plan>*}`)。
fn extract_plan(msg: &str) -> Option<&str> {
    let after = msg.split_once(OPEN_TAG)?.1;
    Some(after.split_once(CLOSE_TAG).map_or(after, |(body, _)| body))
}

/// bash の `$(...)` と同じく末尾の改行をすべて落とす。
fn trim_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// `cmd 2>&1` 相当: stdout と stderr を同じパイプに流して 1 本の文字列にする。
fn run_merged(bin: &Path, flag: &str, file: &Path) -> Option<(i32, String)> {
    let (mut reader, writer) = std::io::pipe().ok()?;
    let mut child = Command::new(bin)
        .arg(flag)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(writer.try_clone().ok()?)
        .stderr(writer)
        .spawn()
        .ok()?;
    // Command を落として親側の書き込み端を閉じる(閉じないと read_to_end が返らない)。
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    let status = child.wait().ok()?;
    let out = String::from_utf8_lossy(&buf).into_owned();
    Some((status.code().unwrap_or(1), out))
}

fn write_plan_file(body: &str) -> Option<PathBuf> {
    let dir = env_path("TMPDIR", || PathBuf::from("/tmp"));
    let pid = std::process::id();
    for n in 0..100u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = dir.join(format!("codex-plan-gate.{pid}-{nanos}-{n}.md"));
        if let Ok(mut f) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            f.write_all(body.as_bytes()).ok()?;
            f.write_all(b"\n").ok()?;
            return Some(path);
        }
    }
    None
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn run() -> Option<String> {
    let state_root = env_path("CODEX_PLAN_GATE_DIR", || {
        home().join(".codex/codex-plan-gate")
    });
    let state_dir = state_root.join("state");
    let max_blocks: u64 = std::env::var("CODEX_PLAN_GATE_MAX_BLOCKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let claude_hooks = env_path("CODEX_PLAN_GATE_CLAUDE_HOOKS_DIR", || {
        home().join(".claude/hooks")
    });
    let scope_gate = claude_hooks.join("plan-scope-gate");
    let precedent_gate = claude_hooks.join("plan-precedent-gate");

    if !(is_executable(&scope_gate) && is_executable(&precedent_gate)) {
        return None;
    }
    if state_root.join("skip").exists()
        || std::env::var("SKIP_CODEX_PLAN_GATE").as_deref() == Ok("1")
    {
        return None;
    }

    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok()?;
    let input: Value = serde_json::from_str(&raw).ok()?;
    if input.get("hook_event_name").and_then(Value::as_str) != Some("Stop") {
        return None;
    }
    let msg = input
        .get("last_assistant_message")
        .and_then(Value::as_str)?;
    if msg.is_empty() {
        return None;
    }
    let session_id = input
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    // bash の `plan_body="$(extract_plan ...)"` は末尾改行を落とす。
    let plan_body = trim_trailing_newlines(extract_plan(msg)?);
    if plan_body.is_empty() {
        return None;
    }

    fs::create_dir_all(&state_dir).ok()?;
    let count_file = state_dir.join(format!("{session_id}.count"));
    let escalated_file = state_dir.join(format!("{session_id}.escalated"));
    if escalated_file.exists() {
        return None;
    }

    let plan_file = write_plan_file(plan_body)?;
    let _cleanup = Cleanup(plan_file.clone());

    // 起動できない(=判定不能)ときは通す。
    let (rc1, out1) = run_merged(&scope_gate, "--check-plan", &plan_file)?;
    let (rc2, out2) = run_merged(&precedent_gate, "--check", &plan_file)?;
    if rc1 == 0 && rc2 == 0 {
        return None;
    }

    let count = fs::read_to_string(&count_file)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    let _ = fs::write(&count_file, count.to_string());

    if count >= max_blocks {
        let _ = fs::write(&escalated_file, "");
        return None;
    }

    let mut reason = String::from(
        "Codex の Plan(<proposed_plan>)が要求インベントリ/先行例接地の形式検査を通過していません。修正してから再度 Plan を提案してください。\n\n",
    );
    if rc1 != 0 {
        reason.push_str(&format!(
            "--- plan-scope-gate ---\n{}\n\n",
            trim_trailing_newlines(&out1)
        ));
    }
    if rc2 != 0 {
        reason.push_str(&format!(
            "--- plan-precedent-gate ---\n{}",
            trim_trailing_newlines(&out2)
        ));
    }
    let doc = serde_json::json!({"decision": "block", "reason": reason});
    serde_json::to_string_pretty(&doc).ok()
}

fn main() -> ExitCode {
    if let Some(out) = run() {
        println!("{out}");
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_block_only() {
        assert_eq!(
            extract_plan("a<proposed_plan>X</proposed_plan>b<proposed_plan>Y</proposed_plan>"),
            Some("X")
        );
    }

    #[test]
    fn unclosed_block_takes_rest() {
        assert_eq!(extract_plan("a<proposed_plan>X\nY"), Some("X\nY"));
    }

    #[test]
    fn no_tag_is_none() {
        assert_eq!(extract_plan("no plan"), None);
    }
}
