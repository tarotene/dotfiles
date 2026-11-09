//! Stop hook の入力。`stop_hook_active`(直前の block で続けさせられたターンの再呼び出しか)を読む
//! のは、ここだけである。
//!
//! 検査は、`stop_hook_active` を自分で読まず、[`Reentry`] を宣言して [`StopInput::permits`] に
//! 問う。`Quiet` の検査は、立っているとき黙る。`Recheck` の検査は、見ずに検査する。
//! `repo-structure-check` の `hook_io` 検査(モジュール名)が、`stop_hook_active` の文字列を `agent-session` の外に
//! 書いた crate を落とす。

use serde_json::Value;

use crate::outcome::Reentry;

/// Stop hook の標準入力。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopInput {
    raw: String,
    active: bool,
}

impl StopInput {
    /// 読めない JSON でも失敗しない(`stop_hook_active` を偽として、`raw` をそのまま検査へ渡す)。
    /// 入力が読めないことの扱いは、検査が決める。真偽は厳密な bool だけを真とする。
    pub fn new(raw: impl Into<String>) -> StopInput {
        let raw = raw.into();
        let active = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|v| v.get("stop_hook_active").and_then(Value::as_bool))
            .unwrap_or(false);
        StopInput { raw, active }
    }

    /// 標準入力の全文。
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// この検査は、今回の呼び出しで検査してよいか。
    pub fn permits(&self, policy: Reentry) -> bool {
        match policy {
            Reentry::Recheck => true,
            Reentry::Quiet => !self.active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_is_silenced_only_by_a_true_bool() {
        for (raw, quiet_ok) in [
            (r#"{"stop_hook_active":true}"#, false),
            (r#"{"stop_hook_active":false}"#, true),
            (r#"{}"#, true),
            (r#"{"stop_hook_active":"true"}"#, true),
            (r#"{"stop_hook_active":1}"#, true),
            ("not json", true),
            ("", true),
        ] {
            let i = StopInput::new(raw);
            assert_eq!(i.permits(Reentry::Quiet), quiet_ok, "{raw}");
            assert!(i.permits(Reentry::Recheck), "{raw}");
        }
    }

    #[test]
    fn raw_is_kept_verbatim() {
        let raw = "{\"a\":1}\n";
        assert_eq!(StopInput::new(raw).raw(), raw);
    }
}
