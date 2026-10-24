//! cmd-hash-log — PostToolUse(Bash) hook が実行されたコマンドの正規化
//! ハッシュだけを記録する(ADR-543「既存手段の前倒し接地と、決定論への
//! 昇格導線」段3)。逐語反復(SKILL.md のコードブロックが改変なしに繰り
//! 返し実行されている)の検出入力になる。
//!
//! コマンド本文はどこにも書かない(ADR-0011 のプライバシー規約 —
//! `agent-turn-log.sh` の「本文を1箇所以外に出さない」方針を、ここでは
//! 「どこにも出さない(ハッシュのみ)」までさらに強める)。正規化・ハッシュ
//! そのものは `hook_io::cmd_hash` が単一正本(段4 の promotion-detect が
//! SKILL.md のコードブロックを同じ関数でハッシュして照合する)。

use hook_io::HookInput;
use std::ffi::OsStr;
use std::path::PathBuf;

/// `explicit`(`CMD_HASH_LOG_PATH`)→ `xdg_state_home` → `home` の順に解決。
/// `hook_io::gate_event::default_path_with` と同じ優先順位。
pub fn resolve_log_path(
    explicit: Option<&OsStr>,
    xdg_state_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(p) = explicit {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let base = match xdg_state_home {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(home?).join(".local/state"),
    };
    Some(base.join("claude").join("cmd-hashes.jsonl"))
}

/// Bash 以外・コマンドが空なら記録しない(`None`)。
pub fn build_record(input: &HookInput, repo_key: &str, ts_unix: u64) -> Option<String> {
    let cmd = input.bash_command()?;
    let hash = hook_io::cmd_hash::hash(cmd);
    Some(
        serde_json::json!({
            "ts_unix": ts_unix,
            "session_id": input.session_id,
            "repo_key": repo_key,
            "hash": hash,
        })
        .to_string(),
    )
}

/// hook 本体(`main` が標準入力を読んだあとの処理)。コマンドのハッシュを 1 行、ログに追記する。
/// 出力は無い(`None`)。`main` と、ルータ(`crates/hook-router`)が同じ関数を呼ぶ。
pub fn hook(raw: &str) -> Option<hook_io::Outcome> {
    use std::io::Write;
    let input = HookInput::parse(raw)?;
    if input.hook_event_name != "PostToolUse" {
        return None;
    }

    let repo_key = input
        .project_dir()
        .and_then(|d| hook_io::git::toplevel(&d))
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default();

    let ts_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let line = build_record(&input, &repo_key, ts_unix)?;

    let path = resolve_log_path(
        std::env::var_os("CMD_HASH_LOG_PATH").as_deref(),
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_log_path_prefers_explicit() {
        assert_eq!(
            resolve_log_path(
                Some(OsStr::new("/explicit")),
                Some(OsStr::new("/xdg")),
                Some(OsStr::new("/home"))
            ),
            Some(PathBuf::from("/explicit"))
        );
    }

    #[test]
    fn resolve_log_path_prefers_xdg_over_home() {
        assert_eq!(
            resolve_log_path(None, Some(OsStr::new("/xdg")), Some(OsStr::new("/home"))),
            Some(PathBuf::from("/xdg/claude/cmd-hashes.jsonl"))
        );
    }

    #[test]
    fn resolve_log_path_falls_back_to_home() {
        assert_eq!(
            resolve_log_path(None, None, Some(OsStr::new("/home"))),
            Some(PathBuf::from("/home/.local/state/claude/cmd-hashes.jsonl"))
        );
    }

    #[test]
    fn resolve_log_path_none_without_home() {
        assert_eq!(resolve_log_path(None, None, None), None);
    }

    #[test]
    fn build_record_omits_body_and_includes_hash() {
        let input = HookInput::parse(
            r#"{"tool_name":"Bash","tool_input":{"command":"echo secret-token-xyz"},"session_id":"s1"}"#,
        )
        .unwrap();
        let rec = build_record(&input, "/repo", 100).unwrap();
        assert!(!rec.contains("secret-token-xyz"), "{rec}");
        assert!(rec.contains("\"session_id\":\"s1\""), "{rec}");
        assert!(rec.contains("\"repo_key\":\"/repo\""), "{rec}");
        assert!(rec.contains("\"ts_unix\":100"), "{rec}");
    }

    #[test]
    fn build_record_none_for_non_bash() {
        let input = HookInput::parse(r#"{"tool_name":"Read","tool_input":{}}"#).unwrap();
        assert!(build_record(&input, "", 0).is_none());
    }

    #[test]
    fn build_record_none_for_empty_command() {
        let input =
            HookInput::parse(r#"{"tool_name":"Bash","tool_input":{"command":""}}"#).unwrap();
        assert!(build_record(&input, "", 0).is_none());
    }
}
