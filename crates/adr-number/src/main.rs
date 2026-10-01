//! adr-number のエントリポイント。判定は lib.rs。
//!
//! Claude Code / Codex CLI から同じ 1 バイナリを呼ぶ(#391、attribution-guard
//! と同じ型)。bash 時代の Codex adapter(config/codex/hooks/adr-number.sh)の
//! 代わりに `--agent <claude|codex>` で「プロジェクト dir を示す環境変数」を
//! 切り替える。省略時は `claude`。
//!
//! 縮退: 何があっても exit 0(deny は一切しない段 3 の利便性層)。

use guard_core::hook::agent_from_args;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = agent_from_args(&args);
    if let Some(out) = adr_number::run(agent, &mut std::io::stdin()) {
        print!("{out}");
    }
    ExitCode::SUCCESS
}
