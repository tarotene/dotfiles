//! adr-number のエントリポイント。判定は lib.rs。
//!
//! Claude Code / Codex CLI から同じ 1 バイナリを呼ぶ(#391、attribution-guard
//! と同じ型)。bash 時代の Codex adapter(config/codex/hooks/adr-number.sh)の
//! 代わりに `--agent <claude|codex>` で「プロジェクト dir を示す環境変数」を
//! 切り替える。省略時は `claude`。`--agent` の解析は `agent_session::entry` が持つ。
//! 標準入力は、プロジェクト dir が環境変数で分かるときは読まない(最安の早期 exit)ので、
//! `run` が遅延して読む。
//!
//! 縮退: 何があっても exit 0(deny は一切しない段 3 の利便性層)。
//! ただし `--agent` の誤りは、標準エラーに理由を出して exit 1(exit 2 は「止める」の意味)。

use agent_session::entry::{enter, Spec};
use agent_session::{Agent, HookEvent};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "adr-number",
    agents: &[Agent::Claude, Agent::Codex],
    event: Some(HookEvent::PostToolUse),
};

fn main() -> ExitCode {
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };
    if let Some(out) = adr_number::run(call.agent, &mut std::io::stdin()) {
        print!("{out}");
    }
    ExitCode::SUCCESS
}
