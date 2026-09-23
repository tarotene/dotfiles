//! stdin JSON の型(クラスタ D・E)と、エージェントごとの入力形式差。
//!
//! 吸収元:
//! - D: `plan-precedent-gate.sh:240-245` / `plan-scope-gate.sh:300-305`
//!   (`INPUT="$(cat)"` → `.cwd` → `[[ -d $CWD ]] || CWD="$HOME"` → `cd`)
//! - E: `worktree-fresh-base.sh:66` / `adr-number.sh:119` ほか
//!   (`${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty')}`)
//!
//! エージェント差(`config/codex/hooks/attribution-guard.sh:16-19`、
//! `config/copilot/hooks/attribution-guard.sh:16-24` の実測記録):
//! - Claude / Codex: `{"tool_name":"Bash","tool_input":{"command":..}, "cwd":..}`
//! - Copilot: `{"toolName":"bash","toolArgs":{"command":..}, "cwd":..}`

use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;

/// hook を呼ぶエージェント。入力 JSON の形と、判定出力の形が異なる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Copilot,
}

impl std::str::FromStr for Agent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Agent::Claude),
            "codex" => Ok(Agent::Codex),
            "copilot" => Ok(Agent::Copilot),
            other => Err(format!("unknown agent: {other} (claude|codex|copilot)")),
        }
    }
}

/// 3 エージェントの入力をまとめて受ける生の形。未知のフィールドは無視する。
#[derive(Debug, Default, Deserialize)]
struct RawInput {
    // Claude / Codex
    tool_name: Option<String>,
    tool_input: Option<Value>,
    tool_response: Option<Value>,
    session_id: Option<String>,
    hook_event_name: Option<String>,
    // Copilot
    #[serde(rename = "toolName")]
    tool_name_copilot: Option<String>,
    #[serde(rename = "toolArgs")]
    tool_args_copilot: Option<Value>,
    #[serde(rename = "sessionId")]
    session_id_copilot: Option<String>,
    // 共通
    cwd: Option<String>,
}

/// エージェント差を吸収した hook 入力。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HookInput {
    /// ツール名。Copilot の小文字 `bash` は `Bash` に正規化する。
    pub tool_name: String,
    /// `tool_input`(Copilot は `toolArgs`)。無ければ `Value::Null`。
    pub tool_input: Value,
    /// PostToolUse の `tool_response`。無ければ `Value::Null`。
    pub tool_response: Value,
    pub session_id: String,
    pub hook_event_name: String,
    pub cwd: Option<PathBuf>,
}

impl HookInput {
    /// JSON 文字列をパースする。不正 JSON は `None`(呼び出し側は素通しする —
    /// bash 版の「stdin 不正は黙って exit 0」と同じ縮退)。
    pub fn parse(json: &str) -> Option<Self> {
        let raw: RawInput = serde_json::from_str(json).ok()?;
        let tool_name = raw
            .tool_name
            .or_else(|| {
                raw.tool_name_copilot.map(|t| match t.as_str() {
                    "bash" => "Bash".to_string(),
                    _ => t,
                })
            })
            .unwrap_or_default();
        Some(HookInput {
            tool_name,
            tool_input: raw
                .tool_input
                .or(raw.tool_args_copilot)
                .unwrap_or(Value::Null),
            tool_response: raw.tool_response.unwrap_or(Value::Null),
            session_id: raw
                .session_id
                .or(raw.session_id_copilot)
                .unwrap_or_else(|| "unknown".to_string()),
            hook_event_name: raw.hook_event_name.unwrap_or_default(),
            cwd: raw.cwd.filter(|c| !c.is_empty()).map(PathBuf::from),
        })
    }

    /// Bash ツール呼び出しならコマンド文字列を返す。
    pub fn bash_command(&self) -> Option<&str> {
        if self.tool_name != "Bash" {
            return None;
        }
        self.tool_input
            .get("command")?
            .as_str()
            .filter(|c| !c.is_empty())
    }

    /// クラスタ E: `CLAUDE_PROJECT_DIR` を優先し、無ければ `.cwd`。
    /// 実在するディレクトリだけを返す。
    pub fn project_dir(&self) -> Option<PathBuf> {
        self.project_dir_with(std::env::var_os("CLAUDE_PROJECT_DIR").map(PathBuf::from))
    }

    /// [`Self::project_dir`] の環境変数を注入可能にした版(テスト用)。
    pub fn project_dir_with(&self, env_project: Option<PathBuf>) -> Option<PathBuf> {
        env_project
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| self.cwd.clone())
            .filter(|p| p.is_dir())
    }

    /// クラスタ D: `.cwd` が実在すればそこへ、無ければ `$HOME` へ移動する。
    /// 移動に失敗しても続行する(bash 版の `cd ... || true`)。
    pub fn enter_cwd(&self) {
        let target = self
            .cwd
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        if let Some(dir) = target {
            let _ = std::env::set_current_dir(dir);
        }
    }
}

/// クラスタ D: stdin を全部読んでパースする。読めない・不正なら `None`。
pub fn read_stdin() -> Option<HookInput> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    HookInput::parse(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_shape() {
        let i = HookInput::parse(
            r#"{"tool_name":"Bash","tool_input":{"command":"git status"},"session_id":"s1","cwd":"/tmp","hook_event_name":"PreToolUse"}"#,
        )
        .unwrap();
        assert_eq!(i.bash_command(), Some("git status"));
        assert_eq!(i.session_id, "s1");
        assert_eq!(i.hook_event_name, "PreToolUse");
        assert_eq!(i.cwd, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn copilot_shape_is_normalised() {
        let i = HookInput::parse(
            r#"{"sessionId":"c1","timestamp":1,"cwd":"/tmp","toolName":"bash","toolArgs":{"command":"ls"}}"#,
        )
        .unwrap();
        assert_eq!(i.tool_name, "Bash");
        assert_eq!(i.bash_command(), Some("ls"));
        assert_eq!(i.session_id, "c1");
    }

    #[test]
    fn missing_fields_default() {
        let i = HookInput::parse("{}").unwrap();
        assert_eq!(i.session_id, "unknown");
        assert_eq!(i.bash_command(), None);
        assert_eq!(i.tool_response, Value::Null);
        assert!(HookInput::parse("{").is_none());
    }

    #[test]
    fn non_bash_or_empty_command() {
        let i = HookInput::parse(r#"{"tool_name":"Read","tool_input":{"command":"x"}}"#).unwrap();
        assert_eq!(i.bash_command(), None);
        let i = HookInput::parse(r#"{"tool_name":"Bash","tool_input":{"command":""}}"#).unwrap();
        assert_eq!(i.bash_command(), None);
    }

    #[test]
    fn project_dir_prefers_env_and_requires_dir() {
        let d = tempfile::tempdir().unwrap();
        let i = HookInput {
            cwd: Some(d.path().to_path_buf()),
            ..Default::default()
        };
        assert_eq!(i.project_dir_with(None), Some(d.path().to_path_buf()));
        let e = tempfile::tempdir().unwrap();
        assert_eq!(
            i.project_dir_with(Some(e.path().to_path_buf())),
            Some(e.path().to_path_buf())
        );
        // 空の CLAUDE_PROJECT_DIR は未設定扱い
        assert_eq!(
            i.project_dir_with(Some(PathBuf::new())),
            Some(d.path().to_path_buf())
        );
        let gone = HookInput {
            cwd: Some(PathBuf::from("/nonexistent/xyz")),
            ..Default::default()
        };
        assert_eq!(gone.project_dir_with(None), None);
    }

    #[test]
    fn agent_from_str() {
        assert_eq!("copilot".parse::<Agent>(), Ok(Agent::Copilot));
        assert!("x".parse::<Agent>().is_err());
    }
}
