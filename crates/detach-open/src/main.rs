//! detach-open — ファイル/URL を開く切り離しランチャ。`open` / `xdg-open` /
//! `$BROWSER` として配備する(scripts/detach-open.sh の移植、ADR-0024 / #389)。
//!
//! system の xdg-open(xdg-utils 1.1.3)は COSMIC を認識せず generic モードに
//! 落ち、MIME ハンドラ(ブラウザ・eog 等)を呼び出し側の foreground プロセス
//! グループで exec する — 呼び出し側はブロックし、Ctrl+C でビューアごと死ぬ。
//! 代わりに新しいセッションへ切り離す。`gio open` は同じ xdg 既定を解決して
//! 即座に返る。
//!
//! フォールバックは絶対パスの `/usr/bin/xdg-open` でなければならない: この
//! ランチャ自身が PATH 上の `xdg-open` を shadow するので、素の名前だと自分自身に
//! 再帰する。
//!
//! 切り離し自体は `setsid -f`(util-linux)に任せる(bash 版と同じ。fork 後に
//! 親が即 exit 0 する)。stdout/stderr は /dev/null へ、stdin は引き継ぐ。
//! exec で置き換えるので、終了コードは setsid のものがそのまま返る。

use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode, Stdio};

/// setsid に渡す argv(先頭が `-f`)。テストしやすいよう純粋関数に切り出す。
fn setsid_args(have_gio: bool, args: &[String]) -> Vec<String> {
    let mut v = vec!["-f".to_string()];
    if have_gio {
        v.push("gio".into());
        v.push("open".into());
    } else {
        v.push("/usr/bin/xdg-open".into());
    }
    v.extend(args.iter().cloned());
    v
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv = setsid_args(hook_io::proc::command_exists("gio"), &args);
    let err = Command::new("setsid")
        .args(argv)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .exec();
    // exec が返るのは起動失敗のときだけ(setsid 不在など)。シェルの
    // 「command not found」と同じ 127 にそろえる。
    let _ = err;
    ExitCode::from(127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gio_branch() {
        let a = vec!["https://example.com".to_string(), "x y".to_string()];
        assert_eq!(
            setsid_args(true, &a),
            ["-f", "gio", "open", "https://example.com", "x y"]
        );
    }

    #[test]
    fn fallback_is_absolute_xdg_open() {
        assert_eq!(setsid_args(false, &[]), ["-f", "/usr/bin/xdg-open"]);
    }
}
