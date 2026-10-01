//! claude-usage — Claude の rate limit(5h セッション窓 / 週間モデル別上限)を
//! herdr のタブバー右端に常時表示する(#413、移植元
//! `config/claude/statusline/claude-usage.sh`)。
//!
//! Claude Code hook ではない。herdr の `ui.tab_bar_right` の command エントリ
//! (config/herdr/config.toml)が interval 実行し、標準出力の最終行を描画する。
//! 設計と根拠: docs/claude/claude-usage.md。
//!
//! データ源は `/usage` が内部で使う非公開 API `GET https://api.anthropic.com/api/oauth/usage`。
//! fetch は bash 版と同じく `curl` に任せる(TLS をこのバイナリに持ち込まない)。
//! トークンは curl の argv に載せず、`--config -` で stdin から渡す。
//!
//! 使い方:
//!   herdr から: 引数なし
//!   内部専用:   claude-usage __render <usage_json_file> <state_file> <now_epoch>

use claude_usage::{emit_stale, guard, read_token, render_from_files, state_last_fetch};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn print(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

/// `command -v curl`
fn in_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|d| {
        let p = if d.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            d.join(name)
        };
        std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

fn env_or(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

/// `mktemp "${TMPDIR:-/tmp}/claude-usage-fetch.XXXXXX"`
fn mktemp_fetch() -> Option<PathBuf> {
    let dir = env_or("TMPDIR", "/tmp");
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    for i in 0..16u32 {
        let p = dir.join(format!(
            "claude-usage-fetch.{}{:06}",
            std::process::id(),
            (seed as u64 + i as u64 * 7919) % 1_000_000
        ));
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)
            .is_ok()
        {
            return Some(p);
        }
    }
    None
}

fn fetch(token: &str, out: &Path) -> bool {
    let child = Command::new("curl")
        .args(["-s", "--fail", "--max-time", "5", "--config", "-", "-o"])
        .arg(out)
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let cfg = format!(
        "url = \"https://api.anthropic.com/api/oauth/usage\"\nheader = \"Authorization: Bearer {token}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\n"
    );
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cfg.as_bytes());
    }
    child.wait().is_ok_and(|s| s.success())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("__render") {
        let a = |i: usize| args.get(i).cloned().unwrap_or_default();
        print(&render_from_files(
            Path::new(&a(1)),
            Path::new(&a(2)),
            &a(3),
        ));
        return;
    }

    // jq / date はもう使わない。fetch に要る curl だけを確かめる。
    if !in_path("curl") {
        return;
    }
    let state_dir = env_or("XDG_RUNTIME_DIR", "/tmp");
    if std::fs::create_dir_all(&state_dir).is_err() {
        return;
    }
    let state = state_dir.join("claude-usage-tabbar.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 再取得ガード: reload-config 直後の即時実行ストーム等で 30 秒未満の再フェッチを
    // 起こさない。
    if let Some(out) = guard(&state, now) {
        print(&out);
        return;
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let cred = PathBuf::from(format!("{}/.claude/.credentials.json", home.display()));
    if std::fs::File::open(&cred).is_err() {
        print(&emit_stale(&state, now));
        return;
    }
    let token = read_token(&cred);
    if token.is_empty() {
        print(&emit_stale(&state, now));
        return;
    }
    let Some(usage_file) = mktemp_fetch() else {
        print(&emit_stale(&state, now));
        return;
    };
    let ok = fetch(&token, &usage_file);
    drop(token);
    if !ok {
        let _ = std::fs::remove_file(&usage_file);
        print(&emit_stale(&state, now));
        return;
    }
    // render_from_files は表示可能な結果があれば 1 行出力し、成功時は state file を
    // 更新する。空出力でも state が今回の now で更新されていれば fetch は成功して
    // いる(authoritative empty)。更新されていなければ不正 JSON 等の失敗であり、
    // stale-if-error の対象。
    let output = render_from_files(&usage_file, &state, &now.to_string());
    let _ = std::fs::remove_file(&usage_file);
    let output = output.trim_end_matches('\n');
    if !output.is_empty() {
        print(&format!("{output}\n"));
    } else if state_last_fetch(&state) != now.to_string() {
        print(&emit_stale(&state, now));
    }
}
