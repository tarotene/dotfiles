//! Preserve legacy jq input semantics separately from normalized HookInput.
use jq_json_compat::{
    jqfmt::J,
    jqparse::{parse_stream, ParseError},
    query as json,
};
use std::io::Read;

/// hook の stdin(bash の `input="$(cat)"`: 末尾の改行を落とす)。
pub fn read_hook_stdin() -> String {
    let mut b = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut b);
    let s = String::from_utf8_lossy(&b).into_owned();
    s.trim_end_matches('\n').to_string()
}

/// 解析済みの hook 入力。不正 JSON・object でない入力は jq と同じくエラー扱い。
pub struct HookJson {
    values: Result<Vec<J>, ParseError>,
}

impl HookJson {
    pub fn parse(input: &str) -> Self {
        let values = parse_stream(input);
        if values.is_err() {
            eprintln!("wrapup-stop-gate: hook input is not valid JSON");
        }
        HookJson { values }
    }

    pub fn get(&self, key: &str, alt: Option<&str>) -> Result<String, ParseError> {
        let v = self.values.as_ref().map_err(|e| *e)?;
        json::jq_r_alt(v, key, alt)
    }

    /// `jq -r '.stop_hook_active // false'` が `true`。
    pub fn stop_hook_active(&self) -> bool {
        self.get("stop_hook_active", Some("false")).as_deref() == Ok("true")
    }

    /// `${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty')}`。jq が失敗すると
    /// bash は `set -e` で jq の終了コード 5 のまま抜けていた。
    pub fn project(&self) -> Result<String, i32> {
        if let Some(p) = std::env::var_os("CLAUDE_PROJECT_DIR")
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string_lossy().into_owned())
        {
            return Ok(p);
        }
        self.get("cwd", None).map_err(|_| 5)
    }

    /// `jq -r '.session_id // "unknown"' 2>/dev/null || session_id="unknown"`
    pub fn session_id(&self) -> String {
        self.get("session_id", Some("unknown"))
            .unwrap_or_else(|_| "unknown".into())
    }
}
