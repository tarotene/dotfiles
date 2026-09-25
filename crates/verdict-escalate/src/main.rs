//! CLI エントリポイント。`wrapup-stop-gate.sh` が Stop の stdin JSON を
//! そのまま渡して呼ぶ(hook 登録はしない — 詳細は lib.rs /
//! docs/claude/verdict-escalate.md)。
//!
//! usage: verdict-escalate --inbox <path>
//!
//! stdin から Stop hook の入力 JSON(`session_id` を読む)を受け取る。
//! `--inbox` が無い・stdin が読めない・不正な JSON の場合は何もせず
//! exit 0(fail-open)。

use std::path::PathBuf;

fn main() {
    let mut inbox: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--inbox" {
            inbox = args.next().map(PathBuf::from);
        }
    }
    let Some(inbox) = inbox else {
        return;
    };
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    verdict_escalate::run(&input.session_id, &inbox);
}
