//! desk-github-gate — 机の段 3a(GitHub 通知の消化)の関門。設計は `lib.rs` と `docs/agents/my-desk.md`。
//!
//! 使い方: `desk-github-gate`。通知ツールのコマンド名は、環境変数 `DESK_NOTIFY_TOOL` か、目印ファイル
//! `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/desk-notify-tool` の 1 行目から解く(名前は公開のソースに書かない)。
//! 成功なら終了コード 0 で、通知ツールの範囲の報告(JSON)を標準出力に出す。
//! 条件が 1 つでも崩れたら終了コード 1 で、理由を標準エラーに出す(その場合、GitHub の通知には触れない)。

use desk_github_gate::{gate, resolve_tool, Runner};
use process_control::command_exists;
use std::process::{Command, ExitCode, Stdio};

/// 実際のコマンドを呼ぶ。標準エラーは引き継ぐ。
struct Real;

impl Runner for Real {
    fn exists(&self, cmd: &str) -> bool {
        command_exists(cmd)
    }

    fn run(&self, cmd: &str, args: &[&str]) -> Option<(bool, String)> {
        let out = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .ok()?;
        Some((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ))
    }
}

/// 通知ツールのコマンド名(環境変数、無ければ目印ファイル)。
fn notify_tool() -> Option<String> {
    let env = std::env::var("DESK_NOTIFY_TOOL").ok();
    let marker = process_env::Xdg::Config
        .resolve()
        .and_then(|d| std::fs::read_to_string(d.join("dotfiles/desk-notify-tool")).ok());
    resolve_tool(env.as_deref(), marker.as_deref())
}

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|a| a == "-h" || a == "--help") {
        println!(
            "使い方: desk-github-gate\n  机の段 3a(GitHub 通知の消化)の関門。この機体の identity(dotfiles-identity verify)と、\n  通知ツールの [github] の範囲の側が一致し、キャッシュが現在の範囲で作られているときだけ、\n  終了コード 0 で `<通知ツール> scope --json` の報告を出す。崩れたら終了コード 1(通知には触れない)。\n  通知ツールの名前: 環境変数 DESK_NOTIFY_TOOL、無ければ ~/.config/dotfiles/desk-notify-tool の 1 行目"
        );
        return ExitCode::SUCCESS;
    }
    let Some(tool) = notify_tool() else {
        eprintln!(
            "desk-github-gate: 通知ツールの名前が未設定(環境変数 DESK_NOTIFY_TOOL か、${{XDG_CONFIG_HOME:-$HOME/.config}}/dotfiles/desk-notify-tool の 1 行目)"
        );
        return ExitCode::from(1);
    };
    match gate(&Real, &tool) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("desk-github-gate: {reason}");
            ExitCode::from(1)
        }
    }
}
