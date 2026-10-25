//! dotfiles-identity — 詳細は lib.rs。
//!
//! `dotfiles-identity verify`: この機体の identity と、ログイン中の Claude アカウントが
//! 一致したときだけ、identity(`personal` / `company`)を標準出力に 1 行出して終了コード 0。
//! 欠ける・読めない・食い違うときは、理由を標準エラー出力に出して終了コード 1
//! (メールアドレスは出さない)。引数の誤りは終了コード 2。

use std::process::ExitCode;

const USAGE: &str = "usage: dotfiles-identity verify";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("verify"), None) => verify(),
        (Some("-h" | "--help"), None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn verify() -> ExitCode {
    let identity = std::env::var("DOTFILES_IDENTITY").ok();
    let expectation = std::env::var("DOTFILES_CLAUDE_ACCOUNT").ok();
    // 読めないファイルと、読めたが中身が足りないファイルは、lib の側で別の理由になる。
    let claude_json = dotfiles_identity::claude_json_path(
        std::env::var_os("CLAUDE_CONFIG_DIR"),
        std::env::var_os("HOME"),
    )
    .and_then(|path| std::fs::read_to_string(path).ok());

    match dotfiles_identity::verify(
        identity.as_deref(),
        expectation.as_deref(),
        claude_json.as_deref(),
    ) {
        Ok(identity) => {
            println!("{identity}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dotfiles-identity: {e}");
            ExitCode::from(1)
        }
    }
}
