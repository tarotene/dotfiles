//! Bitwarden Secrets Manager の machine token(GNOME Keyring)と `bws run`。
//!
//! 秘密は Bitwarden の `github-apps` プロジェクトに置き、ローカルに残るのは
//! read-only な machine account の access token だけ(GNOME Keyring)。形は
//! 旧 `scripts/obsidian-backup` の `configure_token()` / `keyring_token()` と
//! 同型だが、obsidian-backup の machine account とは共有しない(App ごとの
//! 最小権限、2026-09-30 の user 判断、ADR-590)。obsidian-backup の Rust 版は
//! このブランチの時点で存在しない(リポジトリにも無い)ので再利用できず、
//! keyring まわりは secret-tool を呼ぶだけの数十行をここに持つ。
//! token は argv にも stdout にも出さない(`secret-tool` の stdin / `bws` の
//! 環境変数 `BWS_ACCESS_TOKEN` のみ)。

use crate::config::{Config, KEYRING_ATTRIBUTE_APP, KEYRING_ATTRIBUTE_KIND};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus, Stdio};

/// 終了コード(シグナルは bash と同じ 128+sig)。
pub fn exit_code(s: ExitStatus) -> u8 {
    s.code()
        .map(|c| c as u8)
        .or_else(|| s.signal().map(|n| (128 + n) as u8))
        .unwrap_or(1)
}

/// `secret-tool lookup`。空(失敗含む)なら案内を出して None。
pub fn keyring_token(cfg: &Config) -> Option<String> {
    let out = Command::new(&cfg.secret_tool_bin)
        .args([
            "lookup",
            "application",
            KEYRING_ATTRIBUTE_APP,
            "credential",
            KEYRING_ATTRIBUTE_KIND,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .ok();
    let token = out
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default();
    if token.is_empty() {
        eprintln!("github-app-snapshot: Bitwarden machine token is missing from the login keyring");
        eprintln!("run: github-app-snapshot configure-token");
        return None;
    }
    Some(token)
}

pub fn configure_token(cfg: &Config) -> u8 {
    let tty = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        Ok(t) => t,
        Err(e) => {
            eprintln!("github-app-snapshot: /dev/tty: {e}");
            return 1;
        }
    };
    let mut w = &tty;
    let _ = write!(w, "Bitwarden Secrets Manager access token: ");
    // `read -s` 相当: tty のエコーを切って 1 行読み、必ず戻す。
    let stty = |arg: &str| {
        if let Ok(t) = tty.try_clone() {
            let _ = Command::new("stty").arg(arg).stdin(t).status();
        }
    };
    stty("-echo");
    let mut line = String::new();
    let read = BufReader::new(&tty).read_line(&mut line);
    stty("echo");
    let _ = writeln!(w);
    let token = if read.is_ok() {
        line.trim_end_matches(['\n', '\r']).to_string()
    } else {
        String::new()
    };
    if token.is_empty() {
        eprintln!("github-app-snapshot: refusing to store an empty access token");
        return 1;
    }
    let child = Command::new(&cfg.secret_tool_bin)
        .args([
            "store",
            "--label=github-app-snapshot Bitwarden machine token",
            "application",
            KEYRING_ATTRIBUTE_APP,
            "credential",
            KEYRING_ATTRIBUTE_KIND,
        ])
        .stdin(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!(
            "github-app-snapshot: {}: command not found",
            cfg.secret_tool_bin
        );
        return 127;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(token.as_bytes());
    }
    child.wait().map(exit_code).unwrap_or(1)
}

pub fn clear_token(cfg: &Config) -> u8 {
    match Command::new(&cfg.secret_tool_bin)
        .args([
            "clear",
            "application",
            KEYRING_ATTRIBUTE_APP,
            "credential",
            KEYRING_ATTRIBUTE_KIND,
        ])
        .status()
    {
        Ok(s) => exit_code(s),
        Err(_) => {
            eprintln!(
                "github-app-snapshot: {}: command not found",
                cfg.secret_tool_bin
            );
            127
        }
    }
}

/// 引数列を POSIX sh で安全な 1 行にする(各語を単引用符で囲む、末尾に空白)。
/// `printf %q` にしないのは、複数行の語で bash が `$'...\n...'` を出し、
/// `bws run` が再パースする dash が解釈できないため。
pub fn sh_quote_words<S: AsRef<str>>(words: &[S]) -> String {
    let mut out = String::new();
    for w in words {
        out.push('\'');
        out.push_str(&w.as_ref().replace('\'', "'\\''"));
        out.push_str("' ");
    }
    out
}

/// `bws run` の内側で `github-app-snapshot __run <operation> [args...]` を動かす。
/// `bws run` は `--` 以降を空白で結合して shell に再パースさせるため、クォートが
/// 失われる(`sh -c 'a b'` が `sh -c a b` で届く)。すでにクォート済みの文字列を
/// ちょうど 1 つ渡す。
pub fn run_with_bitwarden(cfg: &Config, operation: &str, args: &[String]) -> u8 {
    let Some(token) = keyring_token(cfg) else {
        return 1;
    };
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok().or(Some(p)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "github-app-snapshot".to_string());
    let mut words = vec![
        cfg.env_bin.clone(),
        format!("HOME={}", std::env::var("HOME").unwrap_or_default()),
        format!("PATH={}", std::env::var("PATH").unwrap_or_default()),
        format!("GITHUB_APP_SNAPSHOT_OWNER={}", cfg.owner),
        format!("GITHUB_APP_SNAPSHOT_STATE_DIR={}", cfg.state_dir.display()),
        format!(
            "GITHUB_APP_SNAPSHOT_MANIFESTS_DIR={}",
            cfg.manifests_dir.display()
        ),
        format!("GITHUB_APP_SNAPSHOT_CURL_BIN={}", cfg.curl_bin),
        format!("GITHUB_APP_SNAPSHOT_OPENSSL_BIN={}", cfg.openssl_bin),
        me,
        "__run".to_string(),
        operation.to_string(),
    ];
    words.extend(args.iter().cloned());
    let cmd = sh_quote_words(&words);
    match Command::new(&cfg.bws_bin)
        .args(["run", "--no-inherit-env", "--"])
        .arg(cmd)
        .env("BWS_ACCESS_TOKEN", token)
        .status()
    {
        Ok(s) => exit_code(s),
        Err(_) => {
            eprintln!("github-app-snapshot: {}: command not found", cfg.bws_bin);
            127
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_each_word() {
        assert_eq!(sh_quote_words(&["a b", "c'd"]), "'a b' 'c'\\''d' ");
        assert_eq!(sh_quote_words(&["x\ny"]), "'x\ny' ");
    }
}
