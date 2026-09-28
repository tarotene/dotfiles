//! hook エントリポイント。詳細は lib.rs / docs/claude/pkexec-guard.md。
//!
//! PreToolUse 専用。3 つの CLI(Claude Code / Codex CLI / Copilot CLI)から
//! 同じ 1 バイナリを呼ぶ——`attribution-guard.sh` の bash `source` に相当
//! する仕組みを Rust は持たないため、代わりに `--agent <claude|codex|
//! copilot>` フラグで出力形式(hook-io::Agent)を切り替える。フラグ省略時
//! は `claude`(home/modules/claude.nix の登録では常に明示するが、手動
//! 実行時のデフォルトとして残す)。
//!
//! 判定できない入力(stdin が不正 JSON、対象イベントでない)では何も
//! 出力せず exit 0 する(判定できないことを deny に変えない — hook 入力
//! そのものが読めない場合だけの fail-open。pkexec を含むコマンドを解析
//! できない場合は lib.rs 側で deny に倒す)。

use hook_io::Agent;

fn parse_agent() -> Agent {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--agent" {
            if let Some(v) = args.next() {
                if let Ok(a) = v.parse() {
                    return a;
                }
            }
        } else if let Some(v) = arg.strip_prefix("--agent=") {
            if let Ok(a) = v.parse() {
                return a;
            }
        }
    }
    Agent::Claude
}

fn main() {
    let agent = parse_agent();
    let Some(input) = hook_io::input::read_stdin() else {
        return;
    };
    if input.hook_event_name != "PreToolUse" {
        return;
    }
    if let Some(decision) = pkexec_guard::check(&input, agent) {
        decision.emit(agent);
    }
}
