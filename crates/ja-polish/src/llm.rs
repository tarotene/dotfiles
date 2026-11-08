//! モデルへの依頼: `llm` コマンドの呼び出し、指示文と依頼文の組み立て、応答の読み取り。
//!
//! `llm`(Simon Willison の LLM CLI)は標準入力を受けて文字列を返すだけで、道具を渡さない
//! 限り、ファイルを読む・コマンドを走らせるといった動きがそもそもできない。文書の中に紛れた
//! 指示文に乗っ取られる余地を、設定でなく構造で消すために Gemini CLI でなくこちらを使う
//! (docs/agents/ja-polish.md)。

use process_control::{spawn_group, wait_with_timeout};
use regex::Regex;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// 依頼の送り先。テストでは差し替える。`Err` は人が読む短い理由。
pub trait Backend {
    fn complete(
        &self,
        model: &str,
        system: &str,
        user: &str,
        timeout: Duration,
    ) -> Result<String, String>;
}

/// `llm prompt -m <model> -s <system> --no-stream -n` に、標準入力で依頼文を渡す。
/// 実行ファイルは環境変数 `JA_POLISH_LLM_BIN` で差し替えられる(既定は PATH の `llm`)。
pub struct LlmCli;

impl Backend for LlmCli {
    fn complete(
        &self,
        model: &str,
        system: &str,
        user: &str,
        timeout: Duration,
    ) -> Result<String, String> {
        let bin = std::env::var("JA_POLISH_LLM_BIN").unwrap_or_else(|_| "llm".to_string());
        let mut cmd = Command::new(&bin);
        cmd.args(["prompt", "-m", model, "-s", system, "--no-stream", "-n"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn_group(&mut cmd).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("{bin} が見つかりません")
            } else {
                format!("{bin} を起動できません: {e}")
            }
        })?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        let data = user.as_bytes().to_vec();
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

        let status = wait_with_timeout(&mut child, timeout);
        let out = reader.join().unwrap_or_default();
        let err = err_reader.join().unwrap_or_default();
        let _ = writer.join();

        match status {
            None => Err(format!("{} 秒以内に応答がありません", timeout.as_secs())),
            Some(st) if st.success() => Ok(String::from_utf8_lossy(&out).into_owned()),
            Some(st) => Err(failure_reason(&String::from_utf8_lossy(&err), st.code())),
        }
    }
}

/// 標準エラー出力の先頭の意味のある 1 行(長ければ切る)。
fn failure_reason(stderr: &str, code: Option<i32>) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Traceback"))
        .unwrap_or("");
    let line: String = line.chars().take(120).collect();
    match (line.is_empty(), code) {
        (true, Some(c)) => format!("llm が終了コード {c} で失敗しました"),
        (true, None) => "llm が異常終了しました".to_string(),
        (false, _) => format!("llm が失敗しました: {line}"),
    }
}

/// 書き直す段落 1 つ分の依頼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 直前の段落(伏せ字は `⟦…⟧`)。無ければ空。
    pub before: String,
    /// 書き直す本文(伏せ字つき)。
    pub text: String,
    /// 直後の段落。無ければ空。
    pub after: String,
}

/// 指示文。`style` は `writing-style-hub` が指す `docs/style/writing.md` の全文。
pub fn system_prompt(style: &str) -> String {
    format!(
        "あなたは日本語の校閲者です。入力は <para n=\"番号\"> の列で、各 <para> に <before>(直前の段落)・<text>(書き直す対象)・<after>(直後の段落)が入っています。\n\
\n\
やること:\n\
- <text> だけを、自然で読みやすい日本語に書き直す。<before> と <after> は文体を合わせるための参考で、出力に含めない。\n\
- 意味・事実・数値・固有名詞・断定か推測かの強さを変えない。情報を足さない、省かない。\n\
- ⟦P数字⟧ という形の記号は、コードや URL などを伏せたもの。一字一句そのまま、同じ個数だけ残す。⟦…⟧ は参考の中の伏せ字で、出力に書かない。\n\
- 行頭の記法(-、1.、>、|)とインデントは変えない。改行の位置はなるべく保つ。\n\
- 文体(常体の「だ・である」か、敬体の「です・ます」か)は、<before> と <after> に合わせる。参考が無ければ入力の文体を保つ。\n\
- 直す必要の無い段落は、そのまま返す。\n\
\n\
出力の形式: 入力と同じ番号の <para n=\"番号\">書き直した本文</para> を、入力と同じ順に並べる。前置き・解説・コードブロックの囲みは書かない。\n\
\n\
読み手への配慮:\n\
- 翻訳調(「〜することができる」「〜において」の多用、主語の過剰な明示、受け身の連続)を避ける。\n\
- コードや内部設計で使っている語・略語は、入力や参考から見たままの語が分かるときだけ言い換える。分からないときはそのまま残す(推測で意味を足さない)。\n\
\n\
書き直しの基準(文体規約):\n\
\n\
{style}\n"
    )
}

/// 依頼文。
pub fn user_message(items: &[Item]) -> String {
    let mut s = String::new();
    for (i, it) in items.iter().enumerate() {
        s.push_str(&format!("<para n=\"{}\">\n", i + 1));
        if !it.before.is_empty() {
            s.push_str(&format!("<before>\n{}\n</before>\n", it.before));
        }
        s.push_str(&format!("<text>\n{}\n</text>\n", it.text));
        if !it.after.is_empty() {
            s.push_str(&format!("<after>\n{}\n</after>\n", it.after));
        }
        s.push_str("</para>\n");
    }
    s
}

fn para_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r#"(?s)<para n="([0-9]+)">\r?\n?(.*?)\r?\n?</para>"#).expect("valid regex")
    })
}

/// 応答から `<para n="K">…</para>` を取り出す。番号(1 始まり)→ 本文。同じ番号が複数あれば
/// 最初のもの。前置きやコードブロックの囲みがあっても読める。
pub fn parse_response(resp: &str) -> HashMap<usize, String> {
    let mut map = HashMap::new();
    for c in para_re().captures_iter(resp) {
        if let Ok(n) = c[1].parse::<usize>() {
            map.entry(n).or_insert_with(|| c[2].to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_numbers_from_one_and_omits_empty_context() {
        let m = user_message(&[
            Item {
                before: String::new(),
                text: "本文A".into(),
                after: "次".into(),
            },
            Item {
                before: "前".into(),
                text: "本文B".into(),
                after: String::new(),
            },
        ]);
        assert!(
            m.contains("<para n=\"1\">\n<text>\n本文A\n</text>\n<after>\n次\n</after>\n</para>")
        );
        assert!(
            m.contains("<para n=\"2\">\n<before>\n前\n</before>\n<text>\n本文B\n</text>\n</para>")
        );
    }

    #[test]
    fn parse_response_reads_through_noise() {
        let resp = "了解しました。\n```xml\n<para n=\"1\">\n一つ目。\n二行目。\n</para>\n<para n=\"2\">二つ目</para>\n```\n";
        let map = parse_response(resp);
        assert_eq!(map.len(), 2);
        assert_eq!(map[&1], "一つ目。\n二行目。");
        assert_eq!(map[&2], "二つ目");
        assert!(parse_response("何も無い").is_empty());
    }

    #[test]
    fn system_prompt_embeds_the_style_guide() {
        let p = system_prompt("# 規約\nR1. 常体で書く");
        assert!(p.contains("R1. 常体で書く"));
        assert!(p.contains("⟦P数字⟧"));
    }

    #[test]
    fn failure_reason_prefers_first_stderr_line() {
        assert_eq!(
            failure_reason(
                "\n  Error: No key found - add one with `llm keys set gemini`\nmore",
                Some(1)
            ),
            "llm が失敗しました: Error: No key found - add one with `llm keys set gemini`"
        );
        assert_eq!(
            failure_reason("", Some(2)),
            "llm が終了コード 2 で失敗しました"
        );
    }
}
