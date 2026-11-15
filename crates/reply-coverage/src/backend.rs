//! 検出の送り先: 文脈を切った新しい Claude(`claude -p`)。
//!
//! 起動の形は 2026-11-15 に実測して決めた(docs/agents/reply-coverage.md「`claude -p` の起動」):
//! - `--bare` は購読(OAuth)の認証を読まないので `Not logged in` で失敗する。使えない。
//! - 設定・CLAUDE.md・skill・MCP を切らずに呼ぶと、自明な 1 回でも `total_cost_usd` が約 0.106 になった。
//!   `--setting-sources project`・`--disable-slash-commands`・`--strict-mcp-config` と、空の作業ディレクトリ、
//!   `--system-prompt` で約 0.00024 になった(同じモデル、同じ依頼)。
//! - `--tools ""` で道具を渡さない。会話の中に紛れた指示に乗っ取られても、読み書き・実行ができない。
//!
//! 実行ファイルは環境変数 `REPLY_COVERAGE_CLAUDE_BIN` で差し替えられる(テスト用)。

use process_control::{spawn_group, wait_with_timeout};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct Request<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub schema: &'a str,
    /// 空の作業ディレクトリ(プロジェクトの設定・CLAUDE.md を拾わせない)。
    pub cwd: &'a Path,
    pub timeout: Duration,
}

pub trait Backend {
    /// 検出の JSON(スキーマに従う文字列)を返す。
    fn complete(&self, req: &Request) -> Result<String, BackendError>;
}

pub struct ClaudeCli;

impl Backend for ClaudeCli {
    fn complete(&self, req: &Request) -> Result<String, BackendError> {
        let bin = process_env::var_nonempty("REPLY_COVERAGE_CLAUDE_BIN")
            .unwrap_or_else(|| "claude".to_string());
        let mut cmd = Command::new(&bin);
        cmd.args([
            "-p",
            "--no-session-persistence",
            "--tools",
            "",
            "--output-format",
            "json",
            "--setting-sources",
            "project",
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--model",
            req.model,
            "--system-prompt",
            req.system,
            "--json-schema",
            req.schema,
        ])
        .current_dir(req.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = spawn_group(&mut cmd).map_err(|e| {
            BackendError(if e.kind() == std::io::ErrorKind::NotFound {
                format!("{bin} が見つかりません")
            } else {
                format!("{bin} を起動できません: {e}")
            })
        })?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        let data = req.user.as_bytes().to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
        let mut out_pipe = child.stdout.take().expect("piped stdout");
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out_pipe.read_to_end(&mut buf);
            buf
        });
        let mut err_pipe = child.stderr.take().expect("piped stderr");
        let err_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err_pipe.read_to_end(&mut buf);
            buf
        });

        let status = wait_with_timeout(&mut child, req.timeout);
        let out = reader.join().unwrap_or_default();
        let err = err_reader.join().unwrap_or_default();
        let _ = writer.join();

        match status {
            None => Err(BackendError(format!(
                "{} 秒以内に応答がありません",
                req.timeout.as_secs()
            ))),
            Some(st) if st.success() => extract_output(&String::from_utf8_lossy(&out)),
            Some(st) => {
                let line = String::from_utf8_lossy(&err)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect::<String>();
                Err(BackendError(format!(
                    "claude が終了コード {} で失敗しました: {line}",
                    st.code().map_or("-".to_string(), |c| c.to_string())
                )))
            }
        }
    }
}

/// `--output-format json` の封筒から、検出の JSON を取り出す。`structured_output` を優先し、
/// 無ければ `result`(文字列)を使う。封筒が `is_error` なら、その `result` を理由にして失敗にする。
pub fn extract_output(stdout: &str) -> Result<String, BackendError> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| BackendError(format!("claude の出力が JSON ではありません: {e}")))?;
    if v["is_error"].as_bool().unwrap_or(false) {
        let reason: String = v["result"]
            .as_str()
            .unwrap_or("(理由なし)")
            .chars()
            .take(120)
            .collect();
        return Err(BackendError(format!(
            "claude がエラーを返しました: {reason}"
        )));
    }
    if let Some(so) = v.get("structured_output").filter(|s| s.is_object()) {
        return Ok(so.to_string());
    }
    match v["result"].as_str() {
        Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
        _ => Err(BackendError(
            "claude の出力に structured_output も result もありません".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_output_is_preferred() {
        let out = r#"{"is_error":false,"result":"ignored","structured_output":{"asks":[]}}"#;
        assert_eq!(extract_output(out).unwrap(), r#"{"asks":[]}"#);
    }

    #[test]
    fn result_string_is_the_fallback() {
        let out = r#"{"is_error":false,"result":"{\"asks\":[]}"}"#;
        assert_eq!(extract_output(out).unwrap(), r#"{"asks":[]}"#);
    }

    #[test]
    fn an_error_envelope_is_a_failure_with_its_reason() {
        let out = r#"{"is_error":true,"result":"Not logged in · Please run /login"}"#;
        let e = extract_output(out).unwrap_err();
        assert!(e.0.contains("Not logged in"), "{e}");
    }

    #[test]
    fn garbage_and_empty_envelopes_are_failures() {
        assert!(extract_output("not json").is_err());
        assert!(extract_output(r#"{"is_error":false,"result":"  "}"#).is_err());
    }
}
