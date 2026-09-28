//! hook エントリポイント。詳細は lib.rs。
//!
//! deny/allow は一切返さない(記録専用の PostToolUse hook)。読めない・
//! 書けない場合は黙って exit 0(best-effort — 記録の失敗で作業を止めない、
//! `gh-edit-allow`/`hook_io::gate_event` と同じ fail-open)。

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    if input.hook_event_name != "PostToolUse" {
        return;
    }

    let repo_key = input
        .project_dir()
        .and_then(|d| hook_io::git::toplevel(&d))
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default();

    let ts_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let Some(line) = cmd_hash_log::build_record(&input, &repo_key, ts_unix) else {
        return;
    };

    let Some(path) = cmd_hash_log::resolve_log_path(
        std::env::var_os("CMD_HASH_LOG_PATH").as_deref(),
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    ) else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}
