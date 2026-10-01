//! hook 入出力(bash 版 guard の `main()` と `emit_deny()`)。
//!
//! `hook_io::HookInput` を使わず生の JSON を見るのは、bash 版がエージェント
//! ごとに別のキーだけを読んでいたため(Claude/Codex は `.tool_name` /
//! `.tool_input`、Copilot adapter は `.toolName` / `.toolArgs`)。`HookInput`
//! は両方を混ぜて正規化する(Copilot の `bash` → `Bash` 等)ので、たとえば
//! Copilot 形の入力を Claude として受けたときの挙動が bash と変わる。
//!
//! 出力は `jq -n --arg reason … '{…}'` と同じバイト列(2 空白インデント、
//! 末尾改行)— `hook_io::jqfmt` を使う。

use hook_io::jqfmt::{self, J};
use hook_io::Agent;
use serde_json::Value;

use crate::gh::{command_substitution, jq_raw};

/// エージェント差を吸収した PreToolUse 入力(生の JSON を保持する)。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub agent: Agent,
    /// `jq -r '.tool_name // empty'`(Copilot は `.toolName`)。無ければ空文字。
    pub tool: String,
    /// stdin の JSON 全体(MCP の本文などを見るため)。
    pub raw: Value,
}

impl ToolCall {
    /// stdin の JSON を読む。不正 JSON・`.tool_name` が読めない(jq が
    /// エラーになる形)なら `None` — bash の `|| exit 0` と同じく黙って通す。
    pub fn parse(agent: Agent, stdin: &str) -> Option<Self> {
        let raw: Value = serde_json::from_str(stdin).ok()?;
        let key = match agent {
            Agent::Copilot => "toolName",
            Agent::Claude | Agent::Codex => "tool_name",
        };
        let tool = jq_r_path(&raw, &[key]).ok()?.unwrap_or_default();
        Some(ToolCall { agent, tool, raw })
    }

    /// この呼び出しがシェル実行 tool か(Claude/Codex: `Bash`、Copilot:
    /// 小文字の `bash` — Copilot adapter の実測記録)。
    pub fn is_bash(&self) -> bool {
        match self.agent {
            Agent::Copilot => self.tool == "bash",
            Agent::Claude | Agent::Codex => self.tool == "Bash",
        }
    }

    /// シェル実行 tool のコマンド文字列(`jq -r '.tool_input.command // empty'`、
    /// Copilot は `.toolArgs.command`)。シェル実行でない・空・読めないなら
    /// `None`。
    pub fn bash_command(&self) -> Option<String> {
        if !self.is_bash() {
            return None;
        }
        let key = match self.agent {
            Agent::Copilot => "toolArgs",
            Agent::Claude | Agent::Codex => "tool_input",
        };
        jq_r_path(&self.raw, &[key, "command"])
            .ok()
            .flatten()
            .filter(|c| !c.is_empty())
    }

    /// `jq -r '.tool_input.<a> // .tool_input.<b> // … // empty'`(MCP の本文
    /// など)。jq がエラーになる形・全部空なら `None`。
    pub fn tool_input_alt(&self, keys: &[&str]) -> Option<String> {
        let key = match self.agent {
            Agent::Copilot => "toolArgs",
            Agent::Claude | Agent::Codex => "tool_input",
        };
        for k in keys {
            match jq_r_path(&self.raw, &[key, k]) {
                Err(JqError) => return None,
                Ok(Some(s)) => return Some(s),
                Ok(None) => {}
            }
        }
        None
    }
}

/// jq が添字で失敗した(null でも object でもない値を `.key` で引いた)。
/// bash 版ではこの終了ステータスで `|| exit 0` / `|| return 1` に倒れていた。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JqError;

/// `$(jq -r '.a.b // empty' <<< "$json")` の結果。途中で null でも
/// object でもない値を添字で引くと jq はエラー(`Err`)、`null`/`false`/
/// 欠落は `Ok(None)`。コマンド置換なので末尾の改行は落とす。
pub fn jq_r_path(v: &Value, path: &[&str]) -> Result<Option<String>, JqError> {
    let mut cur = v;
    for k in path {
        cur = match cur {
            Value::Null => &Value::Null,
            Value::Object(m) => m.get(*k).unwrap_or(&Value::Null),
            _ => return Err(JqError),
        };
    }
    Ok(jq_raw(cur).map(|s| command_substitution(&s)))
}

/// deny の hook 出力(bash の `emit_deny` / Copilot adapter の
/// `emit_deny_copilot`)。Claude/Codex は `hookSpecificOutput` でラップし、
/// Copilot はラップしない直下の JSON(Copilot adapter の実測記録)。
pub fn deny_output(agent: Agent, reason: &str) -> String {
    match agent {
        Agent::Claude | Agent::Codex => jqfmt::deny_for_event("PreToolUse", reason),
        Agent::Copilot => format!(
            "{}\n",
            J::obj(vec![
                ("permissionDecision", J::str("deny")),
                ("permissionDecisionReason", J::str(reason)),
            ])
            .pretty()
        ),
    }
}

/// `--agent <claude|codex|copilot>` / `--agent=<…>` を引数から読む。無い・
/// 不正なら `Agent::Claude`(pkexec-guard と同じ既定)。
pub fn agent_from_args(args: &[String]) -> Agent {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--agent" {
            if let Some(v) = it.next() {
                if let Ok(agent) = v.parse() {
                    return agent;
                }
            }
        } else if let Some(v) = a.strip_prefix("--agent=") {
            if let Ok(agent) = v.parse() {
                return agent;
            }
        }
    }
    Agent::Claude
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_and_copilot_shapes() {
        let c = ToolCall::parse(
            Agent::Claude,
            r#"{"tool_name":"Bash","tool_input":{"command":"ls\n\n"}}"#,
        )
        .unwrap();
        assert_eq!(c.bash_command().as_deref(), Some("ls"));
        // Copilot 形を Claude として受けても拾わない(bash は .tool_name だけを見る)
        let c = ToolCall::parse(
            Agent::Claude,
            r#"{"toolName":"bash","toolArgs":{"command":"ls"}}"#,
        )
        .unwrap();
        assert_eq!(c.tool, "");
        assert_eq!(c.bash_command(), None);
        let c = ToolCall::parse(
            Agent::Copilot,
            r#"{"toolName":"bash","toolArgs":{"command":"ls"}}"#,
        )
        .unwrap();
        assert_eq!(c.bash_command().as_deref(), Some("ls"));
        let c = ToolCall::parse(
            Agent::Copilot,
            r#"{"toolName":"Bash","toolArgs":{"command":"ls"}}"#,
        )
        .unwrap();
        assert_eq!(c.bash_command(), None);
    }

    #[test]
    fn jq_errors_and_alternatives() {
        assert!(ToolCall::parse(Agent::Claude, "{x").is_none());
        assert!(ToolCall::parse(Agent::Claude, "\"str\"").is_none());
        let c = ToolCall::parse(Agent::Claude, r#"{"tool_name":"m","tool_input":"s"}"#).unwrap();
        assert_eq!(c.tool_input_alt(&["body"]), None);
        let c = ToolCall::parse(
            Agent::Claude,
            r#"{"tool_name":"m","tool_input":{"body":false,"comment":"c"}}"#,
        )
        .unwrap();
        assert_eq!(c.tool_input_alt(&["body", "comment"]).as_deref(), Some("c"));
        let c = ToolCall::parse(
            Agent::Claude,
            r#"{"tool_name":"m","tool_input":{"body":""}}"#,
        )
        .unwrap();
        assert_eq!(c.tool_input_alt(&["body", "comment"]).as_deref(), Some(""));
    }

    #[test]
    fn deny_output_shapes() {
        assert_eq!(
            deny_output(Agent::Copilot, "r"),
            "{\n  \"permissionDecision\": \"deny\",\n  \"permissionDecisionReason\": \"r\"\n}\n"
        );
        assert!(deny_output(Agent::Codex, "r").starts_with("{\n  \"hookSpecificOutput\": {\n"));
    }

    #[test]
    fn agent_args() {
        let a = |v: &[&str]| agent_from_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&["--agent", "codex"]), Agent::Codex);
        assert_eq!(a(&["--agent=copilot"]), Agent::Copilot);
        assert_eq!(a(&["--agent", "x"]), Agent::Claude);
        assert_eq!(a(&[]), Agent::Claude);
    }
}
