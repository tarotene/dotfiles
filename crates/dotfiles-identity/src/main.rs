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
    match dotfiles_identity::verify_from_env() {
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
