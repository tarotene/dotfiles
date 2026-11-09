//! main-checkout-guard — `pre`(PreToolUse)と `stop`(Stop)の 2 つのサブコマンド。
//! `--agent claude|codex`(既定 claude)。判定なし・縮退はすべて無出力 exit 0。

use agent_session::entry::{emit, enter, finish_stop, read_stdin, Call, Spec};
use agent_session::{
    Agent, HookEvent, HookInput, Reentry, Reply, StopInput, StopOutcome, StopResult,
};
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "main-checkout-guard",
    agents: &[Agent::Claude, Agent::Codex],
    event: Some(HookEvent::PreToolUse),
};

fn main() -> ExitCode {
    // exit 2 は Claude/Codex で「ブロック」扱いになるので、引数の誤りは exit 1(entry が出す)。
    let call = match enter(&SPEC) {
        Ok(call) => call,
        Err(code) => return code,
    };
    let raw = read_stdin();
    let Some(input) = HookInput::parse(&raw) else {
        return ExitCode::SUCCESS;
    };
    match call.rest.first().map(String::as_str) {
        Some("pre") => pre(&call, &raw, &input),
        Some("stop") => finish_stop(call.agent, stop(&input, &raw)),
        _ => {
            eprintln!("usage: main-checkout-guard [--agent claude|codex] pre|stop");
            ExitCode::from(1)
        }
    }
}

fn pre(call: &Call, raw: &str, input: &HookInput) -> ExitCode {
    let reply = Reply::from_outcome(main_checkout_guard::pre_hook(call.agent, input));
    emit(&SPEC, call, raw, &reply)
}

/// block の直後の再呼び出しでは黙る(台帳の更新が使えない場合の無限 block 対策)。入口が適用する宣言。
const STOP_REENTRY: Reentry = Reentry::Quiet;

fn stop(input: &HookInput, raw: &str) -> StopResult {
    if !StopInput::new(raw).permits(STOP_REENTRY) {
        return Ok(None);
    }
    let Some(ledger) = main_checkout_guard::ledger() else {
        return Ok(None);
    };
    let drift = main_checkout_guard::check_drift(&ledger, &input.session_id);
    // 読み取りだけで触れた checkout の移動は block せず、ユーザーへの注意書き
    // (`systemMessage`)だけにする(#898)。
    let notice =
        (!drift.notices.is_empty()).then(|| main_checkout_guard::notice_text(&drift.notices));
    Ok(if !drift.blocking.is_empty() {
        let reason = main_checkout_guard::drift_reason(&drift.blocking);
        Some(match notice {
            Some(n) => StopOutcome::block_with_notice(reason, n),
            None => StopOutcome::block(reason),
        })
    } else {
        notice.map(StopOutcome::notice)
    })
}
