//! esa-mcp-launcher — esa.io MCP サーバの起動ラッパー(ADR-0022。
//! bash 版 `scripts/esa-mcp-launcher` の移植、ADR-0024 / #389 Stage 4e)。
//!
//! ホストローカル(git 管理外)の GPG 暗号化トークン `~/.config/esa/token.gpg` を
//! 復号して `ESA_ACCESS_TOKEN` として環境に載せ、`exec npx -y @esaio/esa-mcp-server`
//! する。`~/.claude.json` の `mcpServers.esa` がこの launcher を指す
//! (home/modules/esa.nix が activation の冪等マージで宣言)。汎用 secret ラッパーに
//! はしない — env トークン依存の MCP サーバは esa だけと調査済みで、抽象化する
//! 利用者が他にいない。
//!
//! トークンの宛先は personal identity の master fingerprint で、GnuPG はこれを
//! 有効な `[E]` サブ鍵に解決する。#252(ADR-0003 Amendment 4)以降は on-disk の
//! `[E]` を優先して選ぶため、カードの挿入は不要 — カード上の元 `[E]` は disaster
//! recovery 用に残置されているだけで、日常の復号経路には登場しない。したがって
//! ログイン後最初の復号では on-disk `[E]` のパスフレーズの pinentry が出る(以後
//! は gpg-agent がキャッシュする)。MCP サーバの起動はセッション開始時 = 画面を
//! 見ている瞬間なので、sign-prewarm(`[S]`/`[E]` 両方のパスフレーズを前倒しする
//! 別機構、#252 で `[E]` にも対応)と同じ「安全な瞬間への前倒し」に収まる。
//!
//! 縮退はしない: トークン欠如・gpg/npx 不在・復号失敗・空トークンは stderr に
//! 診断を出して exit 1 する。hook 群の「黙って no-op」(ADR-0005)を採らないのは、
//! サイレントに Unauthorized で動き続ける状態こそがこのラッパーの直したい事故
//! だから。
//!
//! トークン本体はどこにも出力しない(診断はファイルパスだけ)。gpg の stderr は
//! 呼び出し元へ素通し(pinentry・gpg 自身の診断を隠さない)。
//!
//! bash 版にあった `--selftest` は無い(`cargo test` の統合テストに移した)。
//! 引数は全て npx へ素通しする。
//!
//! 設計と根拠: docs/claude/esa-mcp.md

use hook_io::proc::command_exists;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

/// 診断(1 行目 + 任意のヒント行)を stderr に出して exit 1。
fn die(msg: &str, hint: Option<&str>) -> ExitCode {
    eprintln!("esa-mcp-launcher: {msg}");
    if let Some(h) = hint {
        eprintln!("  {h}");
    }
    ExitCode::FAILURE
}

fn non_empty_env(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|v| !v.is_empty())
}

/// `${ESA_TOKEN_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/esa/token.gpg}`
/// (空文字は未設定扱い)。sign-prewarm も同じ規則(crates/sign-prewarm)。
fn token_file() -> PathBuf {
    if let Some(f) = non_empty_env("ESA_TOKEN_FILE") {
        return PathBuf::from(f);
    }
    let base = non_empty_env("XDG_CONFIG_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
        PathBuf::from,
    );
    base.join("esa/token.gpg")
}

fn main() -> ExitCode {
    let token_file = token_file();
    let shown = token_file.display();

    if !command_exists("gpg") {
        return die("gpg が見つからない(home-manager 未適用?)", None);
    }
    if !token_file.is_file() {
        return die(
            &format!("トークンファイルが無い: {shown}"),
            Some("esa.io で Personal access token を発行し、docs/setup.md の「esa MCP token」節の手順で配置する"),
        );
    }

    // `token="$(gpg --quiet --batch --decrypt FILE)"` 相当。stderr/stdin は素通し。
    let decrypted = Command::new("gpg")
        .args(["--quiet", "--batch", "--decrypt"])
        .arg(&token_file)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output();
    let mut token = match decrypted {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            return die(
                &format!("復号に失敗した: {shown}"),
                Some("on-disk [E] サブ鍵のパスフレーズ・gpg-agent の状態を確認する(#252 未実施のホストではカードの挿入が必要)"),
            )
        }
    };
    // `$(...)` と同じく末尾の改行をすべて落とす。
    while token.last() == Some(&b'\n') {
        token.pop();
    }
    if token.is_empty() {
        return die(&format!("復号結果が空だった: {shown}"), None);
    }
    if !command_exists("npx") {
        return die("npx が見つからない(mise の node runtime を確認)", None);
    }

    let err = Command::new("npx")
        .args(["-y", "@esaio/esa-mcp-server"])
        .args(std::env::args_os().skip(1))
        .env("ESA_ACCESS_TOKEN", OsString::from_vec(token))
        .exec();
    // exec が返るのは起動失敗のときだけ(シェルの `exec: not found` と同じ 127)。
    eprintln!("esa-mcp-launcher: npx の起動に失敗した: {err}");
    ExitCode::from(127)
}
