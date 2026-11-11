//! gpg-prewarm — git commit の署名パスフレーズと esa MCP token.gpg の復号
//! パスフレーズを、ログイン後最初に Claude を開いた瞬間に前倒しして温める
//! SessionStart hook(`config/claude/hooks/sign-prewarm.sh` の移植、#413)。
//!
//! 設計と根拠は docs/agents/gpg-prewarm.md。判定の要点:
//!
//! - `[S]` は cwd に依存しない: scope なしの `git config --get` をリポジトリ外の
//!   一時ディレクトリから読む(`--global` は使わない)。
//! - `[S]` の対象はオンディスクの鍵だけ(`gpg --list-secret-keys --with-colons`
//!   の field 15 が `+`)。card-backed(token S/N)・simple stub(`#`)は温めない。
//! - 冷えているかは `--pinentry-mode error` の試し操作で判定する(プロンプトを
//!   出さない)。温めるのは `--pinentry-mode ask` を 90 秒で刈る 1 回だけ。
//! - `[S]` / `[E]` は独立。一方の対象外・失敗が他方を妨げない。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// bash 版の `timeout 90`。Claude Code 側の timeout(120)より先に刈る。
pub const WARMUP_TIMEOUT: Duration = Duration::from_secs(90);

/// `gpg --list-secret-keys --with-colons --with-fingerprint <key>` の出力から、
/// `key` に一致する指紋を持つ sec/ssb 行の field 15 に `+` があるか(bash の
/// `key_is_on_disk` の awk + `grep -qx '+'`)。
///
/// 一致は awk 版と同じく「完全一致、または一方が他方の前方部分」。空の指紋は
/// どの key とも前方一致する(awk の `index(want, "") == 1`)。
pub fn key_is_on_disk(listing: &str, key: &str) -> bool {
    let mut cur: Option<String> = None;
    for line in listing.lines() {
        let f: Vec<&str> = line.split(':').collect();
        let field = |n: usize| f.get(n - 1).copied().unwrap_or("");
        match field(1) {
            "sec" | "ssb" => cur = Some(field(15).to_string()),
            "fpr" => {
                if let Some(f15) = cur.take() {
                    let fpr = field(10);
                    if (fpr == key || key.starts_with(fpr) || fpr.starts_with(key)) && f15 == "+" {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// esa MCP の token.gpg のパス(`ESA_TOKEN_FILE` → `XDG_CONFIG_HOME` →
/// `$HOME/.config`、いずれも空文字は未設定扱い)。crates/esa-mcp-launcher と同じ規則。
pub fn esa_token_file() -> PathBuf {
    process_env::var_nonempty_path("ESA_TOKEN_FILE").unwrap_or_else(|| {
        process_env::Xdg::Config
            .resolve_lossy()
            .join("esa/token.gpg")
    })
}

/// `command -v <prog>`(`process_control::command_exists`)と、`timeout <limit> cmd`(新しいプロセス
/// グループで動かし、刈るときはグループ全体に TERM。正常終了なら true、起動失敗・非 0・タイムアウトは false。
/// `process_control::status_with_group_timeout`)。以前はこの 2 つを私的に複写していた。
pub use process_control::command_exists;
pub use process_control::status_with_group_timeout as run_with_timeout;

/// 外部コマンドの組み立て(gpg のバイナリ名と、git config を読む一時ディレクトリ)。
pub struct Prewarm {
    pub gpg: String,
    pub warmup_timeout: Duration,
}

impl Prewarm {
    fn gpg(&self) -> Command {
        Command::new(&self.gpg)
    }

    /// scope なしの `git -C <probe> config --get <key>`(失敗は空文字)。
    fn global_git_config(probe: &Path, key: &str) -> String {
        git_repository::out(probe, &["config", "--get", key]).unwrap_or_default()
    }

    fn listing(&self, key: &str) -> String {
        self.gpg()
            .args([
                "--list-secret-keys",
                "--with-colons",
                "--with-fingerprint",
                key,
            ])
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }

    /// `printf '' | gpg --batch --no-tty --pinentry-mode error --local-user K --detach-sign -o /dev/null`
    fn is_warm(&self, key: &str) -> bool {
        self.gpg()
            .args([
                "--batch",
                "--no-tty",
                "--pinentry-mode",
                "error",
                "--local-user",
                key,
                "--detach-sign",
                "-o",
                "/dev/null",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// `timeout 90 gpg --pinentry-mode ask --no-tty --local-user K --detach-sign -o /dev/null </dev/null`
    fn warm_up(&self, key: &str) -> bool {
        let mut c = self.gpg();
        c.args([
            "--pinentry-mode",
            "ask",
            "--no-tty",
            "--local-user",
            key,
            "--detach-sign",
            "-o",
            "/dev/null",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
        run_with_timeout(&mut c, self.warmup_timeout)
    }

    fn is_warm_decrypt(&self, token: &Path) -> bool {
        self.gpg()
            .args([
                "--quiet",
                "--batch",
                "--no-tty",
                "--pinentry-mode",
                "error",
                "-o",
                "/dev/null",
                "--decrypt",
            ])
            .arg(token)
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn warm_up_decrypt(&self, token: &Path) -> bool {
        let mut c = self.gpg();
        c.args([
            "--quiet",
            "--batch",
            "--pinentry-mode",
            "ask",
            "--no-tty",
            "-o",
            "/dev/null",
            "--decrypt",
        ])
        .arg(token)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
        run_with_timeout(&mut c, self.warmup_timeout)
    }

    /// `[S]` の温め(bash の `warm_sign_if_configured`)。
    pub fn warm_sign_if_configured(&self) {
        let Some(probe) = ProbeDir::new() else {
            return;
        };
        let get = |k: &str| Self::global_git_config(&probe.0, k);
        let fmt = get("gpg.format");
        if !(fmt.is_empty() || fmt == "openpgp") {
            return;
        }
        if get("commit.gpgsign") != "true" {
            return;
        }
        let key = get("user.signingkey");
        if key.is_empty() {
            return;
        }
        if !key_is_on_disk(&self.listing(&key), &key) {
            return;
        }
        if self.is_warm(&key) {
            return;
        }
        if !self.warm_up(&key) {
            eprintln!("[gpg-prewarm] 署名鍵 {key} を温められませんでした(キャンセルまたはタイムアウト)。最初の git commit で pinentry が出ます。");
        }
    }

    /// `[E]` の温め(bash の `warm_decrypt_if_present`)。
    pub fn warm_decrypt_if_present(&self) {
        let token = esa_token_file();
        if !token.is_file() {
            return;
        }
        if self.is_warm_decrypt(&token) {
            return;
        }
        if !self.warm_up_decrypt(&token) {
            eprintln!(
                "[gpg-prewarm] esa MCP の token.gpg ({}) を温められませんでした(キャンセルまたはタイムアウト)。MCP 起動時に pinentry が出ます。",
                token.display()
            );
        }
    }

    /// hook 本体(bash の `run_prewarm`)。
    pub fn run(&self) {
        if !command_exists(&self.gpg) || !command_exists("git") {
            return;
        }
        self.warm_sign_if_configured();
        self.warm_decrypt_if_present();
    }
}

/// bash の `PROBE_DIR="$(mktemp -d)"` + `trap 'rm -rf' RETURN`。
struct ProbeDir(PathBuf);

impl ProbeDir {
    fn new() -> Option<Self> {
        let base = std::env::var_os("TMPDIR")
            .filter(|t| !t.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        for i in 0..16u32 {
            let p = base.join(format!("gpg-prewarm.{}.{nanos:x}{i}", std::process::id()));
            if std::fs::create_dir(&p).is_ok() {
                return Some(ProbeDir(p));
            }
        }
        None
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const ONDISK: &str = "\
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:22:8608A3F925E329CC:1783930808:1815466808:::::s:::+::ed25519::
fpr:::::::::57B25182FB450B06570860488608A3F925E329CC:
";

    #[test]
    fn on_disk_matches_fingerprint_and_prefixes() {
        assert!(key_is_on_disk(
            ONDISK,
            "57B25182FB450B06570860488608A3F925E329CC"
        ));
        assert!(key_is_on_disk(ONDISK, "57B25182FB450B06"));
        // primary は card-backed なので一致しても + ではない
        assert!(!key_is_on_disk(
            ONDISK,
            "92E7B05978F0FE4E5500F6F76CFC837175BE257E"
        ));
        // 末尾の key id は前方一致にならない
        assert!(!key_is_on_disk(ONDISK, "8608A3F925E329CC"));
        assert!(!key_is_on_disk("", "X"));
    }

    #[test]
    fn empty_fingerprint_prefix_matches_any_key() {
        let l = "ssb:u:::::::::::::+:\nfpr::::::::::\n";
        assert!(key_is_on_disk(l, "ANY"));
    }

    #[test]
    fn timeout_kills_slow_command() {
        let start = Instant::now();
        let ok = run_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(200));
        assert!(!ok);
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(run_with_timeout(
            &mut Command::new("true"),
            Duration::from_secs(5)
        ));
        assert!(!run_with_timeout(
            &mut Command::new("false"),
            Duration::from_secs(5)
        ));
    }
}
