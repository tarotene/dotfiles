//! pr-title-check — PR タイトルの commit-message 契約の検査(ADR-0031)。
//! 判定は lib.rs。サーバ側 required check(.github/actions/pr-title)の実体で、
//! 他リポジトリの CI もこのバイナリを build して呼ぶ。
//!
//! 使い方:
//!   pr-title-check "<タイトル>"        引数で渡す
//!   echo "<タイトル>" | pr-title-check  stdin(引数が空・省略で端末でないとき)
//!
//! 終了コード: 0 = 適合, 1 = 非適合, 2 = 判定不能(入力なし等)。
//! bash 版にあった `--selftest` は Rust のテスト(`cargo test -p
//! pr-title-check`)に置き換えた。

use pr_title_check::{check_title, Verdict};
use std::ffi::OsString;
use std::io::{IsTerminal, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::process::ExitCode;

const USAGE: &str = "usage: pr-title-check <title>\n";

fn main() -> ExitCode {
    let arg: Option<OsString> = std::env::args_os().nth(1);
    match arg.as_ref().and_then(|a| a.to_str()) {
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let mut title: Vec<u8> = arg.map(OsStringExt::into_vec).unwrap_or_default();
    if title.is_empty() && !std::io::stdin().is_terminal() {
        // bash の `title="$(cat)"`: NUL は落ち、末尾の改行は全部落ちる。
        let mut buf = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut buf);
        buf.retain(|b| *b != 0);
        while buf.last() == Some(&b'\n') {
            buf.pop();
        }
        title = buf;
    }

    if title.is_empty() {
        eprintln!("pr-title-check: 判定不能(タイトルが空)");
        return ExitCode::from(Verdict::Indeterminate.exit_code());
    }

    let verdict = check_title(&title);
    if verdict == Verdict::NonConforming {
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(
            "pr-title-check: 非適合 — 'type(scope)?!?: subject' 形式(type は feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)にしてください: "
                .as_bytes(),
        );
        let _ = err.write_all(&title);
        let _ = err.write_all(b"\n");
    }
    ExitCode::from(verdict.exit_code())
}
