//! external-send-guard — 外部宛メール・Slack への直接送信を deny し、下書き作成
//! (Gmail の create_draft / Slack の draft 系 tool)へ誘導する PreToolUse hook
//! (#413 で `config/claude/hooks/external-send-guard.sh` から移植)。
//!
//! 設計と根拠: docs/claude/external-send-guard.md。
//!
//! 判定は 2 つ:
//! - Gmail: 外部宛(自分のアドレス以外を含む、または reply で宛先が暗黙)の
//!   send_message / reply / forward → deny。create_draft・自分宛のみは通す。
//! - Slack: send_message / reply / schedule_message / post_message 系の tool →
//!   宛先を問わず無条件 deny。名前に draft を含む tool は対象外。
//!
//! 自分のアドレス: `${XDG_CONFIG_HOME:-$HOME/.config}/external-send-guard/self.txt`
//! (1 行 1 アドレス、`#` コメント・空行は無視)。無ければ自分宛 0 件として
//! 全送信を外部宛とみなす(fail-closed)。
//!
//! bash 版は `LC_ALL=C` で動いていたので、大文字小文字の畳み込み・空白の判定は
//! ASCII だけを対象にする(バイト列として比べる)。

use serde_json::Value;

/// `^mcp__.*Gmail.*__(send_message|reply|forward)$`
const GMAIL_SUFFIXES: &[&str] = &["__send_message", "__reply", "__forward"];

/// `^mcp__.*[Ss]lack.*__(slack_)?(send_message|reply|schedule_message|post_message)$`
const SLACK_SUFFIXES: &[&str] = &[
    "__send_message",
    "__reply",
    "__schedule_message",
    "__post_message",
    "__slack_send_message",
    "__slack_reply",
    "__slack_schedule_message",
    "__slack_post_message",
];

/// `^mcp__.*<needle>.*<suffix>$` を 1 行に対して判定する(grep -E 相当)。
fn line_matches(line: &[u8], needles: &[&[u8]], suffixes: &[&str]) -> bool {
    let Some(rest) = line.strip_prefix(b"mcp__") else {
        return false;
    };
    suffixes.iter().any(|s| {
        rest.strip_suffix(s.as_bytes())
            .is_some_and(|mid| needles.iter().any(|n| contains(mid, n)))
    })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// bash 版の `grep -qE "$RE" <<< "$tool"`: tool 名のどれか 1 行がマッチすれば真。
fn any_line_matches(tool: &str, needles: &[&[u8]], suffixes: &[&str]) -> bool {
    tool.as_bytes()
        .split(|&b| b == b'\n')
        .any(|l| line_matches(l, needles, suffixes))
}

/// C ロケールの `[[:space:]]`(空白・\t・\n・\v・\f・\r)。
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if !is_space(*first) {
            break;
        }
        s = rest;
    }
    while let [rest @ .., last] = s {
        if !is_space(*last) {
            break;
        }
        s = rest;
    }
    s
}

/// self.txt の中身 → 自分のアドレス集合(小文字化・前後空白除去済み)。
pub fn parse_self_list(content: &[u8]) -> Vec<Vec<u8>> {
    content
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let t = trim(line);
            if t.is_empty() || t[0] == b'#' {
                return None; // grep -Ev '^[[:space:]]*(#|$)'
            }
            Some(t.to_ascii_lowercase())
        })
        .collect()
}

/// jq -r が 1 値を出力するときの文字列。
fn jq_raw(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string_pretty(v).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// `jq -r '(.tool_input.to // [])[], (.tool_input.cc // [])[], (.tool_input.bcc // [])[]'`
/// を `read -r` で 1 行ずつ読み、空行を捨てた結果。jq がエラーで止まった
/// (配列でも object でもない値を `[]` で回そうとした)時点までの出力を使う。
fn recipients(input: &Value) -> Vec<String> {
    let mut out = String::new();
    'keys: for key in ["to", "cc", "bcc"] {
        let ti = &input["tool_input"];
        if !(ti.is_object() || ti.is_null()) {
            break; // `.tool_input.to` がエラー
        }
        match &ti[key] {
            Value::Null | Value::Bool(false) => {}
            Value::Array(items) => {
                for it in items {
                    out.push_str(&jq_raw(it));
                    out.push('\n');
                }
            }
            Value::Object(m) => {
                for it in m.values() {
                    out.push_str(&jq_raw(it));
                    out.push('\n');
                }
            }
            _ => break 'keys, // Cannot iterate over string/number/true
        }
    }
    out.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn has_key(input: &Value, key: &str) -> bool {
    input["tool_input"]
        .as_object()
        .is_some_and(|m| m.contains_key(key))
}

pub fn gmail_deny_reason(recipients: &str) -> String {
    format!(
        "外部宛のメール送信は直接実行せず、Gmail の下書き作成(create_draft。返信は replyToMessageId 付き)を使ってください(deny)。宛先: {recipients}。下書きを作成したら、ユーザーが Gmail 上で内容を確認・編集して送信します。あわせて、宛先アドレスが公式の一般問い合わせ窓口として文脈まで確認済みか(求人・採用等の別目的窓口の転用ではないか)を確認し、出典 URL・取得日を送信記録に残してください。"
    )
}

pub const SLACK_DENY_REASON: &str = "Slack への直接送信は行わず、下書き系 tool(例: slack_draft_message / send_message_draft。接続先の MCP サーバーに存在しない場合は本文をチャットに出力し、ユーザー自身が Slack へ貼り付けてください)を使ってください(deny)。下書きを作成/提示したら、ユーザーが内容を確認・編集して送信します。";

/// deny なら理由文、通すなら `None`。`self_set` は [`parse_self_list`] の結果。
pub fn decide(tool: &str, input: &Value, self_set: &[Vec<u8>]) -> Option<String> {
    // draft 系 tool は両パターンの対象名を含みうるので先に除外する
    if tool.contains("draft") || tool.contains("Draft") {
        return None;
    }
    if any_line_matches(tool, &[b"Slack", b"slack"], SLACK_SUFFIXES) {
        return Some(SLACK_DENY_REASON.to_string());
    }
    if !any_line_matches(tool, &[b"Gmail"], GMAIL_SUFFIXES) {
        return None;
    }
    let rcpts = recipients(input);
    if rcpts.is_empty() {
        // reply で宛先が全く指定されていない → スレッド由来で判定不能 → deny
        if tool.contains("reply")
            && !has_key(input, "to")
            && !has_key(input, "cc")
            && !has_key(input, "bcc")
        {
            return Some(gmail_deny_reason(
                "(宛先未指定・スレッド由来のため判定不能)",
            ));
        }
        return None;
    }
    let external = rcpts.iter().any(|a| {
        let needle = a.as_bytes().to_ascii_lowercase();
        !self_set.contains(&needle)
    });
    external.then(|| gmail_deny_reason(&rcpts.join(" ")))
}

/// jq と同じ規則で JSON 文字列リテラルを書く(制御文字と DEL は `\u00xx`)。
pub fn jq_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                o.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// bash 版の `jq -n --arg reason ... '{hookSpecificOutput: {...}}'` の整形出力
/// (末尾改行込み)。
pub fn deny_json(reason: &str) -> String {
    format!(
        "{{\n  \"hookSpecificOutput\": {{\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": {}\n  }}\n}}\n",
        jq_string(reason)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn jq_escaping() {
        // jq 1.8.2 の出力: {"a":"x\u007fy\u0001\"\\\n"}
        assert_eq!(
            jq_string("x\u{7f}y\u{1}\"\\\n\u{8}\u{c}\r\t日"),
            r#""x\u007fy\u0001\"\\\n\b\f\r\t日""#
        );
    }

    #[test]
    fn deny_json_parses_back() {
        let v: Value = serde_json::from_str(&deny_json("a\"b")).unwrap();
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecisionReason"],
            json!("a\"b")
        );
    }

    #[test]
    fn regex_equivalents() {
        let g = |t: &str| any_line_matches(t, &[b"Gmail"], GMAIL_SUFFIXES);
        assert!(g("mcp__claude_ai_Gmail__send_message"));
        assert!(g("mcp__Gmail__reply"));
        assert!(!g("mcp__claude_ai_Gmail__send_message_x"));
        assert!(!g("xmcp__Gmail__reply"));
        assert!(!g("mcp__gmail__reply"));
        // 複数行の tool 名はどれか 1 行がマッチすればよい(grep の行単位)
        assert!(g("junk\nmcp__Gmail__forward"));
        let s = |t: &str| any_line_matches(t, &[b"Slack", b"slack"], SLACK_SUFFIXES);
        assert!(s("mcp__slack__slack_send_message"));
        assert!(s("mcp__claude_ai_Slack__post_message"));
        assert!(!s("mcp__x__slack_send_message"));
        assert!(!s("mcp__claude_ai_Slack__search_messages"));
    }

    #[test]
    fn self_list_parsing() {
        let set = parse_self_list(b"# c\n  Me@X.test \r\n\n \t# y\nalt@x.test");
        assert_eq!(set, vec![b"me@x.test".to_vec(), b"alt@x.test".to_vec()]);
    }

    #[test]
    fn recipients_stop_at_non_iterable() {
        // to が文字列だと jq はそこでエラーになり cc 以降も出さない
        let v = json!({"tool_input":{"to":"a@x","cc":["b@x"]}});
        assert!(recipients(&v).is_empty());
        let v = json!({"tool_input":{"to":["a@x"],"cc":"b@x","bcc":["c@x"]}});
        assert_eq!(recipients(&v), vec!["a@x"]);
        let v = json!({"tool_input":{"to":["a\nb", "", 1]}});
        assert_eq!(recipients(&v), vec!["a", "b", "1"]);
    }
}
