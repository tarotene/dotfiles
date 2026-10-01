//! クラスタ K: herdr の unix socket への `pane.report_metadata` 送信(#413)。
//!
//! 吸収元(4 つの独立実装。いずれも python3 の heredoc):
//! - `config/claude/hooks/herdr-claude-metadata.sh:178-243`
//! - `config/codex/hooks/herdr-codex-metadata.sh` / `config/copilot/hooks/herdr-copilot-metadata.sh`
//! - `config/claude/statusline/claude-statusline.sh:198-252`
//!
//! 共通の形: socket に JSON 1 行を書き、応答を最大 4096 バイト読んで捨てる。
//! connect / write / read はすべて 0.5 秒で打ち切り、失敗は呼び出し側が
//! 無音で握りつぶす(herdr 側の公式 integration と同じパターン)。

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// python 版の `client.settimeout(0.5)`。
pub const SOCKET_TIMEOUT: Duration = Duration::from_millis(500);

/// metadata の TTL(4 時間 — SessionEnd クリアの保険)。
pub const TTL_MS: u64 = 14_400_000;

/// Herdr のペイン内で動いているときだけ `(socket_path, pane_id)` を返す。
/// `HERDR_ENV=1` かつ `HERDR_SOCKET_PATH` / `HERDR_PANE_ID` が非空であること
/// (bash 版の 3 行のガード)。
pub fn pane_env() -> Option<(PathBuf, String)> {
    if std::env::var("HERDR_ENV").ok().as_deref() != Some("1") {
        return None;
    }
    let socket = std::env::var_os("HERDR_SOCKET_PATH").filter(|s| !s.is_empty())?;
    let pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    Some((PathBuf::from(socket), pane))
}

/// `printf '%s' "$HERDR_PANE_ID" | tr -c 'A-Za-z0-9_-' '_'` と同じ置換。
/// `tr` はバイト単位なので、マルチバイト文字はバイト数ぶんの `_` になる。
pub fn sanitize_pane_id(pane_id: &str) -> String {
    pane_id
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                b as char
            } else {
                '_'
            }
        })
        .collect()
}

/// ペインごとの状態ファイル `${XDG_RUNTIME_DIR:-/tmp}/<prefix>.<sanitized pane id>`。
/// `XDG_RUNTIME_DIR` は空文字でも未設定扱い(bash の `:-`)。
pub fn pane_state_file(prefix: &str, pane_id: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("{prefix}.{}", sanitize_pane_id(pane_id)))
}

fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// python 版の `time.time_ns()`(`seq` に使う単調増加の近似)。
pub fn seq_ns() -> u128 {
    now().as_nanos()
}

/// python 版の `f"{source}:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}"`。
/// 乱数部は衝突回避のためだけなので、ナノ秒と pid の混合で代用する。
pub fn request_id(source: &str) -> String {
    let t = now();
    let salt = (t.subsec_nanos() as u64 ^ (std::process::id() as u64).wrapping_mul(2_654_435_761))
        % 1_000_000;
    format!("{source}:{}:{salt:06}", t.as_millis())
}

/// socket に `line`(末尾改行は付け足す)を送り、応答を最大 4096 バイト読んで捨てる。
/// 応答の読み取り失敗は成功扱い(python 版の内側の `except: pass`)。
pub fn send_line(socket_path: &Path, line: &str) -> std::io::Result<()> {
    let mut client = connect_with_timeout(socket_path)?;
    client.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    client.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    client.write_all(&buf)?;
    let mut sink = [0u8; 4096];
    let _ = client.read(&mut sink);
    Ok(())
}

/// unix socket の connect は通常ブロックしないが、listen backlog が詰まった
/// ときに備えて python 版と同じく 0.5 秒で諦める。std には connect_timeout が
/// 無いので、別スレッドで connect して待つ。
fn connect_with_timeout(path: &Path) -> std::io::Result<UnixStream> {
    let (tx, rx) = std::sync::mpsc::channel();
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(UnixStream::connect(p));
    });
    rx.recv_timeout(SOCKET_TIMEOUT).unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "herdr socket connect timed out",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn sanitize_is_bytewise() {
        assert_eq!(sanitize_pane_id("p-1_a"), "p-1_a");
        assert_eq!(sanitize_pane_id("a/b c"), "a_b_c");
        // 'é' は 2 バイト
        assert_eq!(sanitize_pane_id("é"), "__");
    }

    #[test]
    fn request_id_shape() {
        let id = request_id("claude-hook");
        let parts: Vec<&str> = id.split(':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "claude-hook");
        assert_eq!(parts[2].len(), 6);
    }

    #[test]
    fn send_line_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("s.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 1];
            while s.read(&mut b).unwrap() == 1 {
                got.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
            s.write_all(b"{\"ok\":true}\n").unwrap();
            String::from_utf8(got).unwrap()
        });
        send_line(&sock, r#"{"x":1}"#).unwrap();
        assert_eq!(h.join().unwrap(), "{\"x\":1}\n");
    }

    #[test]
    fn send_line_fails_without_listener() {
        let d = tempfile::tempdir().unwrap();
        assert!(send_line(&d.path().join("none.sock"), "{}").is_err());
    }
}
