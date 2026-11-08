//! 投稿の関所(PreToolUse、hook-router の規則 `ja-polish-gate`)。
//!
//! 投稿・下書き・コミットメッセージの本文に、`ja-polish` を通していない日本語の段落が 1 つでも
//! あれば、呼び出しを実行前に拒否する。LLM は呼ばず、記録(`records.jsonl`)を引くだけなので、
//! ルータの持ち時間に収まる。
//!
//! 見る対象:
//! - `gh pr|issue create|edit|comment`、`gh pr review` の `--body` / `--body-file`
//! - `gh api` の `-f body=…` / `--input`(本文の取り出しは `github-command`)
//! - `git commit` の `-m` / `--message` / `-F` / `--file`
//! - MCP: Gmail の下書き・返信・送信(`body`、無ければ `htmlBody`)、Slack の下書き・送信
//!   (`message` `text` `body` `content` `markdown`)、Claude Docs の作成・更新(文字列すべて)
//!
//! 通すもの:
//! - 設定ファイルが無い機体(ja-polish を有効にしていない機体)
//! - 本文に `No-Attribution:` の行がある(本人の文の代理投稿。書き直しの対象外)
//! - 環境変数 `JA_POLISH_GATE_ALLOW=1`(ja-polish が壊れたときの逃げ道)
//! - 本文が取れない(コマンド置換の中身が不明、`--body-file` が読めない、知らない形の MCP 入力)。
//!   判定不能を拒否に変えない
//!
//! `ja-polish` が失敗して素通しにした段落は、記録に `skipped` で残るので、この関所は通す
//! (止めない設計。後から知らせる経路は gate イベント)。

use crate::{config, polish, records};
use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use command_inspection::command::{first_deny, gh_command_at};
use github_command::gh::{has_heredoc, read_body_file, ApiBody, BodyFlags};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    GhBody,
    GhApi,
    GitCommit,
}

fn is_git_commit_at(t: &[String], i: usize) -> bool {
    if t[i].rsplit('/').next() != Some("git") {
        return false;
    }
    let mut j = i + 1;
    while j < t.len() {
        let a = t[j].as_str();
        if a == "-C" || a == "-c" {
            j += 2;
        } else if a.starts_with('-') {
            j += 1;
        } else {
            return a == "commit";
        }
    }
    false
}

fn is_target_at(tokens: &[String], i: usize) -> Option<Kind> {
    const BODY: &[&[&str]] = &[
        &["pr", "create"],
        &["pr", "edit"],
        &["pr", "comment"],
        &["pr", "review"],
        &["issue", "create"],
        &["issue", "edit"],
        &["issue", "comment"],
    ];
    if BODY.iter().any(|w| gh_command_at(tokens, i, w)) {
        return Some(Kind::GhBody);
    }
    if gh_command_at(tokens, i, &["api"]) {
        return Some(Kind::GhApi);
    }
    is_git_commit_at(tokens, i).then_some(Kind::GitCommit)
}

/// `git commit` の範囲から、メッセージ(`-m` / `--message` / `-F` / `--file`)を出現順に取る。
/// `-F -`(標準入力)と読めないファイルは取れない。
fn commit_messages(tokens: &[String]) -> Vec<String> {
    let n = tokens.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let t = tokens[i].as_str();
        match t {
            "-m" | "--message" if i + 1 < n => {
                out.push(tokens[i + 1].clone());
                i += 2;
                continue;
            }
            "-F" | "--file" if i + 1 < n => {
                if let Some(s) = read_body_file(&tokens[i + 1]) {
                    out.push(s);
                }
                i += 2;
                continue;
            }
            _ => {
                if let Some(v) = t.strip_prefix("--message=") {
                    out.push(v.to_string());
                } else if let Some(p) = t.strip_prefix("--file=") {
                    if let Some(s) = read_body_file(p) {
                        out.push(s);
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// 取れた本文の断片と、ヒアドキュメントの本体を、空行で区切って 1 つの文書にする。
///
/// 共有の `assemble_body_text`(`github-command`)を使わない理由は 2 つ:
/// - 断片どうしを空行で区切る。共有の関数は改行 1 つで繋ぐので、前後の断片と繋がって別の
///   段落になり、記録のハッシュと合わなくなる。
/// - インラインの `--body` が `$(` やバッククォートを含んでも判定する。共有の関数はそれを
///   「中身が不明」として `None` にする(署名の有無を見る検査には十分)が、ここは本文の中身を
///   判定する検査で、コマンド置換の文字列に日本語は含まれないので、誤って拒否することもない。
///
/// (`--body-file` の中身を「中身が不明」にしてしまう穴は、共有の関数の側で直した。)
fn join_parts(texts: &[String], has_hd: bool, heredoc_bodies: &str) -> Option<String> {
    let mut parts: Vec<String> = texts
        .iter()
        .map(|t| t.trim_end_matches('\n').to_string())
        .filter(|t| !t.trim().is_empty())
        .collect();
    if has_hd && !heredoc_bodies.trim().is_empty() {
        parts.push(heredoc_bodies.trim_end_matches('\n').to_string());
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// 本人の文の代理投稿の印(共有の規範の `No-Attribution:`)。
fn is_proxy_post(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with("No-Attribution:"))
}

/// 本文に、通していない日本語の段落があれば、拒否の理由。
fn judge(what: &str, text: &str, known: &HashSet<String>) -> Option<String> {
    if is_proxy_post(text) {
        return None;
    }
    let pending = polish::pending(text, None, known);
    if pending.is_empty() {
        return None;
    }
    let head: String = pending[0].text.chars().take(40).collect();
    Some(format!(
        "[ja-polish] {what}に、書き直しを通していない日本語の段落が {n} 個あります(最初の段落: 「{head}」)。\
本文を scratchpad のファイルに書き、`ja-polish <そのファイル>` で書き直してから、直したファイルの内容を使ってください。\
`ja-polish` は単体で呼びます(`<`・`>` のリダイレクトや `$(…)` を足すと sandbox の中に残ってキーに届きません)。\
出力の差分を読み、意味がずれていたら、直したファイルを手で直さず、下書きを直して通し直します(記録は段落の中身で照合します)。\
`ja-polish` が通せなかった段落は素通しとして記録されるので、この検査は通ります。\
本人の文をそのまま代理投稿するときは、本文に `No-Attribution: <理由>` を書いてください。\
設計: docs/agents/ja-polish.md",
        n = pending.len(),
    ))
}

/// Bash のコマンド文字列を判定する。
pub fn decide_bash(cmd: &str, known: &HashSet<String>) -> Option<String> {
    first_deny(cmd, is_target_at, |r| {
        let (what, text) = match r.kind {
            Kind::GhBody => (
                "GitHub への投稿の本文",
                join_parts(
                    &BodyFlags::scan(r.tokens).texts,
                    has_heredoc(r.tokens),
                    r.heredoc_bodies,
                )?,
            ),
            Kind::GhApi => (
                "GitHub への投稿の本文",
                join_parts(
                    &ApiBody::scan(r.tokens).texts,
                    has_heredoc(r.tokens),
                    r.heredoc_bodies,
                )?,
            ),
            Kind::GitCommit => (
                "コミットメッセージ",
                join_parts(
                    &commit_messages(r.tokens),
                    has_heredoc(r.tokens),
                    r.heredoc_bodies,
                )?,
            ),
        };
        judge(what, &text, known)
    })
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

fn str_field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

/// MCP ツールの入力から、人が読む本文を取り出す。対象のツールでない・取れないなら `None`。
fn mcp_text(tool: &str, input: &Value) -> Option<(&'static str, String)> {
    let suffix_is = |names: &[&str]| names.iter().any(|n| tool.ends_with(&format!("__{n}")));
    if tool.contains("Gmail")
        && suffix_is(&[
            "create_draft",
            "update_draft",
            "reply",
            "send_message",
            "forward",
        ])
    {
        let body = str_field(input, "body").or_else(|| str_field(input, "htmlBody"))?;
        return Some(("メールの本文", body.to_string()));
    }
    if (tool.contains("Slack") || tool.contains("slack"))
        && (tool.contains("draft")
            || suffix_is(&[
                "send_message",
                "reply",
                "schedule_message",
                "post_message",
                "slack_send_message",
                "slack_reply",
                "slack_schedule_message",
                "slack_post_message",
            ]))
    {
        let parts: Vec<&str> = ["message", "text", "body", "content", "markdown"]
            .iter()
            .filter_map(|k| str_field(input, k))
            .collect();
        return (!parts.is_empty()).then(|| ("Slack の本文", parts.join("\n\n")));
    }
    if tool.contains("Claude_Docs") && suffix_is(&["create", "update", "batch"]) {
        let mut all = Vec::new();
        collect_strings(input, &mut all);
        return (!all.is_empty()).then(|| ("Claude Docs の本文", all.join("\n\n")));
    }
    None
}

/// MCP ツールの呼び出しを判定する。
pub fn decide_mcp(tool: &str, input: &Value, known: &HashSet<String>) -> Option<String> {
    let (what, text) = mcp_text(tool, input)?;
    judge(what, &text, known)
}

/// hook 本体(ルータの規則 `ja-polish-gate` が呼ぶ)。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    if std::env::var("JA_POLISH_GATE_ALLOW").is_ok_and(|v| v == "1") {
        return None;
    }
    // 設定ファイルの無い機体は、ja-polish を有効にしていない。何も検査しない。
    config::load()?;
    let call = ToolCall::parse(agent, raw)?;
    let known = records::records_path()
        .map(|p| records::load(&p))
        .unwrap_or_default();

    let reason = if let Some(cmd) = call.bash_command() {
        decide_bash(&cmd, &known)
    } else if call.tool.starts_with("mcp__") {
        let key = match agent {
            Agent::Copilot => "toolArgs",
            Agent::Claude | Agent::Codex => "tool_input",
        };
        decide_mcp(&call.tool, call.raw.get(key)?, &known)
    } else {
        None
    }?;
    Outcome::deny(reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks;
    use serde_json::json;

    const JP: &str = "翻訳調の文章を直すことができる。";

    fn known(texts: &[&str]) -> HashSet<String> {
        texts.iter().map(|t| blocks::hash(t)).collect()
    }

    fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn gh_body_with_unpolished_japanese_is_denied_and_polished_passes() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(&dir, "b.md", &format!("## 課題\n\n{JP}\n"));
        let cmd = format!("gh pr create -R o/r --title t --body-file {f}");
        let reason = decide_bash(&cmd, &HashSet::new()).expect("deny");
        assert!(
            reason.contains("GitHub への投稿の本文") && reason.contains("1 個"),
            "{reason}"
        );
        assert!(reason.contains("ja-polish <そのファイル>"));
        assert!(
            decide_bash(&cmd, &known(&[JP])).is_none(),
            "記録があれば通る"
        );
    }

    #[test]
    fn a_body_file_with_inline_code_is_still_judged() {
        // 上流の assemble_body_text は、バッククォートを含む本文を「判定不能」にして素通しにする。
        let dir = tempfile::tempdir().unwrap();
        let f = write(&dir, "b.md", &format!("{JP} `cargo test` を実行した。\n"));
        let cmd = format!("gh pr create --title t --body-file {f}");
        assert!(decide_bash(&cmd, &HashSet::new()).is_some());
    }

    #[test]
    fn english_only_and_headings_only_pass() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(&dir, "b.md", "## 課題\n\nFix the bug.\n\nCloses #12\n");
        let cmd = format!("gh issue comment 3 --body-file {f}");
        assert!(decide_bash(&cmd, &HashSet::new()).is_none());
    }

    #[test]
    fn no_attribution_is_a_proxy_post_and_passes() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(
            &dir,
            "b.md",
            &format!("{JP}\n\nNo-Attribution: 本人の文の代理投稿\n"),
        );
        let cmd = format!("gh pr comment 1 --body-file {f}");
        assert!(decide_bash(&cmd, &HashSet::new()).is_none());
    }

    #[test]
    fn undeterminable_bodies_pass() {
        // 本文がコマンド置換: 中身が分からない。
        assert!(decide_bash("gh pr comment 1 --body \"$(cat x.md)\"", &HashSet::new()).is_none());
        // 読めない --body-file。
        assert!(decide_bash(
            "gh pr comment 1 --body-file /nonexistent/x.md",
            &HashSet::new()
        )
        .is_none());
        // 本文フラグが無い。
        assert!(decide_bash("gh pr view 1", &HashSet::new()).is_none());
        assert!(decide_bash("echo hello", &HashSet::new()).is_none());
    }

    #[test]
    fn inline_body_and_heredoc_are_judged() {
        let cmd = format!("gh pr comment 1 --body '{JP}'");
        assert!(decide_bash(&cmd, &HashSet::new()).is_some());
        let hd = format!("gh pr comment 1 --body \"$(cat <<'EOF'\n{JP}\nEOF\n)\"");
        assert!(decide_bash(&hd, &HashSet::new()).is_some());
        assert!(decide_bash(&hd, &known(&[JP])).is_none());
    }

    #[test]
    fn each_range_is_judged_independently() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "Plain English body.\n");
        let b = write(&dir, "b.md", &format!("{JP}\n"));
        let cmd = format!("gh pr comment 1 --body-file {a} && gh pr comment 2 --body-file {b}");
        assert!(
            decide_bash(&cmd, &HashSet::new()).is_some(),
            "2 つ目が未通過"
        );
    }

    #[test]
    fn git_commit_messages_are_judged() {
        assert!(decide_bash(&format!("git commit -m '{JP}'"), &HashSet::new()).is_some());
        assert!(decide_bash("git commit -m 'fix: english only'", &HashSet::new()).is_none());
        let dir = tempfile::tempdir().unwrap();
        let f = write(&dir, "m.txt", &format!("feat: x\n\n{JP}\n"));
        assert!(decide_bash(&format!("git commit -F {f}"), &HashSet::new()).is_some());
        assert!(decide_bash(&format!("git commit -F {f}"), &known(&[JP])).is_none());
        // 複数の -m と --message=
        let cmd = format!("git commit -m 'subject' --message='{JP}'");
        assert!(decide_bash(&cmd, &HashSet::new()).is_some());
        // git -C <dir> commit も拾う。commit 以外は拾わない。
        assert!(decide_bash(&format!("git -C /x commit -m '{JP}'"), &HashSet::new()).is_some());
        assert!(decide_bash(&format!("git log --grep '{JP}'"), &HashSet::new()).is_none());
    }

    #[test]
    fn gh_api_body_is_judged() {
        let cmd = format!("gh api repos/o/r/issues/1/comments -f body='{JP}'");
        assert!(decide_bash(&cmd, &HashSet::new()).is_some());
        assert!(decide_bash("gh api repos/o/r/issues/1", &HashSet::new()).is_none());
    }

    #[test]
    fn gmail_draft_prefers_body_then_html_and_ignores_subject() {
        let t = "mcp__claude_ai_Gmail__create_draft";
        let denied = json!({"to": ["a@example.com"], "subject": JP, "body": JP});
        assert!(decide_mcp(t, &denied, &HashSet::new()).is_some());
        // 件名だけが日本語(本文は英語): 件名は対象にしない。
        let subject_only = json!({"subject": JP, "body": "Hello."});
        assert!(decide_mcp(t, &subject_only, &HashSet::new()).is_none());
        // body が無ければ htmlBody。
        let html = json!({"htmlBody": format!("<p>{JP}</p>")});
        assert!(decide_mcp(t, &html, &HashSet::new()).is_some());
        assert!(decide_mcp(t, &denied, &known(&[JP])).is_none());
        // 検索など、対象でないツールは見ない。
        assert!(decide_mcp(
            "mcp__claude_ai_Gmail__search_threads",
            &json!({"query": JP}),
            &HashSet::new()
        )
        .is_none());
    }

    #[test]
    fn slack_and_docs_are_judged() {
        let slack = json!({"channel": "C1", "message": JP});
        assert!(decide_mcp(
            "mcp__claude_ai_Slack__slack_send_message_draft",
            &slack,
            &HashSet::new()
        )
        .is_some());
        assert!(decide_mcp(
            "mcp__claude_ai_Slack__slack_search_public",
            &slack,
            &HashSet::new()
        )
        .is_none());
        let docs = json!({"object": "node", "payload": {"markdown": format!("# 題\n\n{JP}")}});
        assert!(decide_mcp("mcp__claude_ai_Claude_Docs__create", &docs, &HashSet::new()).is_some());
        assert!(decide_mcp("mcp__claude_ai_Claude_Docs__read", &docs, &HashSet::new()).is_none());
    }
}
