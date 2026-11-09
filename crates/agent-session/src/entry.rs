//! hook の bin の入口。標準入力の読み、`--agent` の解析、返答の出力を、ここ 1 か所が持つ。
//!
//! 典型の `main`:
//!
//! ```ignore
//! const SPEC: Spec = Spec {
//!     name: "attribution-guard",
//!     agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
//!     event: Some(HookEvent::PreToolUse),
//! };
//! fn main() -> ExitCode {
//!     run(&SPEC, attribution_guard::hook)
//! }
//! ```
//!
//! `--check` のような手動モードや、標準入力を読まない bin は、[`enter`] で引数だけを解析し、
//! 必要になったときに [`read_stdin`] と [`emit`] を呼ぶ。
//!
//! `--agent` は厳密に解析する。値が無い・未知・[`Spec::agents`] に無い・2 回以上、はどれも
//! 標準エラーに理由を出して終了コード 1 で終わる(終了コード 2 は Claude Code で「止める」の意味なので使わない)。
//! 従来は、未知の値が黙って Claude の形に倒れていた。

use std::io::{Read, Write};
use std::process::ExitCode;

use crate::event::HookEvent;
use crate::input::Agent;
use crate::outcome::Outcome;
use crate::reply::Reply;

/// bin の宣言。
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// 標準エラーの接頭辞に使う bin の名前。
    pub name: &'static str,
    /// この bin が受ける `--agent`。先頭が、`--agent` が無いときの既定。
    pub agents: &'static [Agent],
    /// 出力のイベント。`None` は、標準入力の `hook_event_name` で決める(`PermissionRequest` 以外は PreToolUse)。
    pub event: Option<HookEvent>,
}

/// 引数の解析結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub agent: Agent,
    /// `--agent` とその値を除いた引数。順序は保つ。
    pub rest: Vec<String>,
}

fn agent_list(spec: &Spec) -> String {
    spec.agents
        .iter()
        .map(|a| match a {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Copilot => "copilot",
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// 引数(`argv[1..]`)を解析する。純粋関数。
pub fn parse_call(spec: &Spec, argv: &[String]) -> Result<Call, String> {
    let mut agent: Option<Agent> = None;
    let mut rest = Vec::new();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let value = if arg == "--agent" {
            let v = it
                .next()
                .ok_or_else(|| format!("--agent に値が無い({})", agent_list(spec)))?;
            Some(v.as_str())
        } else {
            arg.strip_prefix("--agent=")
        };
        let Some(value) = value else {
            rest.push(arg.clone());
            continue;
        };
        if agent.is_some() {
            return Err("--agent は 1 回だけ".to_string());
        }
        let parsed: Agent = value
            .parse()
            .ok()
            .filter(|a| spec.agents.contains(a))
            .ok_or_else(|| format!("--agent は {} のみ: {value}", agent_list(spec)))?;
        agent = Some(parsed);
    }
    Ok(Call {
        agent: agent.unwrap_or(spec.agents[0]),
        rest,
    })
}

/// 標準入力を読む。UTF-8 でないバイトは置き換え、末尾の改行は落とさない。
pub fn read_stdin() -> String {
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// プロセスの引数を解析する。誤りは標準エラーに出し、終了コード 1 を返す。
pub fn enter(spec: &Spec) -> Result<Call, ExitCode> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    parse_call(spec, &argv).map_err(|message| {
        eprintln!("{}: {message}", spec.name);
        ExitCode::from(1)
    })
}

fn event_of(spec: &Spec, raw: &str) -> HookEvent {
    spec.event.unwrap_or_else(|| {
        let name = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|v| v.get("hook_event_name")?.as_str().map(str::to_string))
            .unwrap_or_default();
        HookEvent::from_input_name(&name)
    })
}

/// 返答を標準出力に書く。書き込みの失敗は無視する。終了コードは 0。
pub fn emit(spec: &Spec, call: &Call, raw: &str, reply: &Reply) -> ExitCode {
    let out = reply.render(call.agent, event_of(spec, raw));
    if !out.is_empty() {
        let _ = std::io::stdout().write_all(out.as_bytes());
    }
    ExitCode::SUCCESS
}

/// 入口の全部: 引数の解析、標準入力の読み、検査、出力。余った引数は無視する。
pub fn run(spec: &Spec, hook: fn(Agent, &str) -> Option<Outcome>) -> ExitCode {
    let call = match enter(spec) {
        Ok(call) => call,
        Err(code) => return code,
    };
    let raw = read_stdin();
    let reply = Reply::from_outcome(hook(call.agent, &raw));
    emit(spec, &call, &raw, &reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Spec = Spec {
        name: "t",
        agents: &[Agent::Claude, Agent::Codex, Agent::Copilot],
        event: Some(HookEvent::PreToolUse),
    };
    const CLAUDE_ONLY: Spec = Spec {
        name: "t",
        agents: &[Agent::Claude],
        event: Some(HookEvent::PreToolUse),
    };

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_flag_uses_the_first_agent() {
        let c = parse_call(&ALL, &argv(&[])).unwrap();
        assert_eq!(c.agent, Agent::Claude);
        assert!(c.rest.is_empty());
        let c = parse_call(
            &Spec {
                agents: &[Agent::Codex, Agent::Claude],
                ..ALL
            },
            &argv(&[]),
        )
        .unwrap();
        assert_eq!(c.agent, Agent::Codex);
    }

    #[test]
    fn both_spellings_are_read_and_removed_from_rest() {
        let c = parse_call(&ALL, &argv(&["--agent", "codex"])).unwrap();
        assert_eq!((c.agent, c.rest.len()), (Agent::Codex, 0));
        let c = parse_call(&ALL, &argv(&["--agent=copilot"])).unwrap();
        assert_eq!((c.agent, c.rest.len()), (Agent::Copilot, 0));
    }

    #[test]
    fn rest_keeps_the_other_arguments_in_order_wherever_the_flag_is() {
        for args in [
            &["--agent", "codex", "pre", "--x"][..],
            &["pre", "--agent=codex", "--x"][..],
            &["pre", "--x", "--agent", "codex"][..],
        ] {
            let c = parse_call(&ALL, &argv(args)).unwrap();
            assert_eq!(c.agent, Agent::Codex, "{args:?}");
            assert_eq!(c.rest, argv(&["pre", "--x"]), "{args:?}");
        }
    }

    #[test]
    fn an_unknown_missing_or_repeated_value_is_an_error() {
        for args in [
            &["--agent", "x"][..],
            &["--agent="][..],
            &["--agent"][..],
            &["--agent", "codex", "--agent", "claude"][..],
            &["--agent=codex", "--agent=codex"][..],
        ] {
            assert!(parse_call(&ALL, &argv(args)).is_err(), "{args:?}");
        }
    }

    #[test]
    fn an_agent_outside_the_spec_is_an_error() {
        assert!(parse_call(&CLAUDE_ONLY, &argv(&["--agent", "claude"])).is_ok());
        let e = parse_call(&CLAUDE_ONLY, &argv(&["--agent", "codex"])).unwrap_err();
        assert!(e.contains("claude") && e.contains("codex"), "{e}");
    }

    #[test]
    fn the_event_comes_from_the_spec_or_from_stdin() {
        let fixed = Spec {
            event: Some(HookEvent::SessionStart),
            ..ALL
        };
        assert_eq!(
            event_of(&fixed, r#"{"hook_event_name":"PermissionRequest"}"#),
            HookEvent::SessionStart
        );
        let from_stdin = Spec { event: None, ..ALL };
        assert_eq!(
            event_of(&from_stdin, r#"{"hook_event_name":"PermissionRequest"}"#),
            HookEvent::PermissionRequest
        );
        assert_eq!(
            event_of(&from_stdin, r#"{"hook_event_name":"PreToolUse"}"#),
            HookEvent::PreToolUse
        );
        assert_eq!(event_of(&from_stdin, "not json"), HookEvent::PreToolUse);
        assert_eq!(event_of(&from_stdin, "{}"), HookEvent::PreToolUse);
    }
}
