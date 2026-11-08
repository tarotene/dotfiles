//! git の外にある文書(vault のノートなど)の基点と、セッションで触れた文書の一覧。
//!
//! 書き込み前(PreToolUse の `Write` / `Edit` / `MultiEdit`)に、次の 2 つを状態ディレクトリへ残す。
//! 作業ツリーには何も書かない(ルータの規則は並列に走るため)。
//!
//! - **基点**: そのファイルの書き込み前の内容。git の外のファイルには merge-base が無いので、
//!   `ja-polish <ファイル>` と Stop の関所は、これを「追加・変更した段落」の比較元に使う。
//!   既にスナップショットがあり、新しければ(12 時間以内)上書きしない。古ければ取り直す。
//!   セッションの ID を使わないのは、`ja-polish` の CLI が ID を知らないため。基点が古い側に
//!   寄ると対象が増えるだけで、通した段落は記録で飛ばすので、書き換えは増えない。
//! - **触れたファイルの一覧**: セッションごとに追記する。Stop の関所が、git の差分に出ない
//!   (git の外の)文書も拾うため。
//!
//! 対象は文書の拡張子(`.md` `.markdown` `.txt`)のファイルだけ。

use crate::records;
use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 基点を取り直すまでの時間。
const SNAPSHOT_TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// 文書とみなす拡張子。
pub fn is_doc(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown" | "txt")
    )
}

fn base_dir() -> Option<PathBuf> {
    Some(records::state_dir()?.join("base"))
}

fn touched_dir() -> Option<PathBuf> {
    Some(records::state_dir()?.join("touched"))
}

fn key(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let mut hex = String::with_capacity(32);
    for b in digest.iter().take(16) {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// `path` の基点(スナップショット)。無ければ `None`。
pub fn load(path: &Path) -> Option<String> {
    let abs = path.canonicalize().ok()?;
    std::fs::read_to_string(base_dir()?.join(format!("{}.snap", key(&abs)))).ok()
}

/// 書き込み前の内容を残す。既に新しいスナップショットがあれば何もしない。ファイルが
/// 無ければ(新規)何もしない — 基点なしで全体が対象になる。
fn take(path: &Path, now: SystemTime) -> std::io::Result<()> {
    let Ok(abs) = path.canonicalize() else {
        return Ok(());
    };
    let Some(dir) = base_dir() else {
        return Ok(());
    };
    let snap = dir.join(format!("{}.snap", key(&abs)));
    if let Ok(meta) = std::fs::metadata(&snap) {
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age < SNAPSHOT_TTL);
        if fresh {
            return Ok(());
        }
    }
    let content = std::fs::read(&abs)?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(snap, content)
}

/// セッションで触れた文書の一覧に足す。
fn touch(session_id: &str, path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let Some(dir) = touched_dir() else {
        return Ok(());
    };
    // セッション ID はファイル名になるので、英数字・`-`・`_` 以外は落とす。
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{safe}.txt")))?;
    writeln!(f, "{}", path.display())
}

/// セッションで触れた文書(いまも存在するもの)。重複は除く。
pub fn touched(session_id: &str) -> Vec<PathBuf> {
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let Some(dir) = touched_dir() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{safe}.txt"))) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    text.lines()
        .map(PathBuf::from)
        .filter(|p| p.is_file() && seen.insert(p.clone()))
        .collect()
}

/// PreToolUse の規則(`ja-polish-snapshot`)。`Write` / `Edit` / `MultiEdit` の対象が文書なら、
/// 基点を残し、触れた一覧に足す。判定はしない(常に `None`)。設定ファイルが無い機体では何もしない。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    crate::config::load()?;
    let call = ToolCall::parse(agent, raw)?;
    if !matches!(call.tool.as_str(), "Write" | "Edit" | "MultiEdit") {
        return None;
    }
    let file = call.raw.pointer("/tool_input/file_path")?.as_str()?;
    let path = Path::new(file);
    if !is_doc(path) {
        return None;
    }
    let session_id = call.raw.get("session_id").and_then(|v| v.as_str())?;
    // 記録は best-effort。書けなくても書き込みを止めない。
    let _ = take(path, SystemTime::now());
    let _ = touch(session_id, path);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_state<T>(f: impl FnOnce(&Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
        std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
        let cfg = dir.path().join("config/dotfiles");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("ja-polish.toml"), "model = \"m\"\n").unwrap();
        f(dir.path())
    }

    // 環境変数を書き換えるので、1 つのテストにまとめる(並列実行で互いに壊さない)。
    #[test]
    fn snapshot_and_touch_lifecycle() {
        with_state(|root| {
            let doc = root.join("note.md");
            std::fs::write(&doc, "書き込み前の内容です。\n").unwrap();
            let raw = |tool: &str, file: &Path| {
                json!({
                    "tool_name": tool,
                    "session_id": "sess-1",
                    "tool_input": {"file_path": file.to_string_lossy()}
                })
                .to_string()
            };

            // 初回の書き込み前: 基点を残し、一覧に足す。
            assert!(hook(Agent::Claude, &raw("Edit", &doc)).is_none());
            assert_eq!(load(&doc).as_deref(), Some("書き込み前の内容です。\n"));
            assert_eq!(touched("sess-1"), vec![doc.clone()]);

            // 書き込んだあとの再実行: 新しい基点があるので上書きしない。一覧は重複しない。
            std::fs::write(&doc, "書き込み後の内容です。\n").unwrap();
            hook(Agent::Claude, &raw("Write", &doc));
            assert_eq!(load(&doc).as_deref(), Some("書き込み前の内容です。\n"));
            assert_eq!(touched("sess-1").len(), 1);

            // 古い基点は取り直す。
            let snap = base_dir()
                .unwrap()
                .join(format!("{}.snap", key(&doc.canonicalize().unwrap())));
            let old = SystemTime::now() - Duration::from_secs(13 * 60 * 60);
            let f = std::fs::File::options().write(true).open(&snap).unwrap();
            f.set_modified(old).unwrap();
            drop(f);
            take(&doc, SystemTime::now()).unwrap();
            assert_eq!(load(&doc).as_deref(), Some("書き込み後の内容です。\n"));

            // 文書でない拡張子、対象でないツール、新規ファイル(基点なし)。
            let code = root.join("a.rs");
            std::fs::write(&code, "x").unwrap();
            hook(Agent::Claude, &raw("Write", &code));
            assert!(load(&code).is_none());
            hook(Agent::Claude, &raw("Read", &doc));
            let fresh = root.join("new.md");
            hook(Agent::Claude, &raw("Write", &fresh));
            assert!(load(&fresh).is_none());
            assert_eq!(
                touched("sess-1").len(),
                1,
                "新規ファイルはまだ存在しないので出ない"
            );
            std::fs::write(&fresh, "x").unwrap();
            assert_eq!(touched("sess-1").len(), 2, "存在すれば出る");
        });
    }

    #[test]
    fn doc_extensions() {
        assert!(
            is_doc(Path::new("a/b.md"))
                && is_doc(Path::new("c.markdown"))
                && is_doc(Path::new("d.txt"))
        );
        assert!(!is_doc(Path::new("e.rs")) && !is_doc(Path::new("README")));
    }
}
