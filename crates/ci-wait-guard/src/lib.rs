//! ci-wait-guard — CI の待ち方を PreToolUse で止め、`pr-checks-wait` へ案内する判定
//! (設計と根拠は docs/agents/ci-wait-guard.md)。
//!
//! CI を待つ正しい手は `pr-checks-wait <PR>` を `run_in_background` で呼ぶこと(赤 1 件で
//! 即 NG、required 全緑で即 OK。判定の正本は `crates/pr-gate/src/gates/ci.rs`)。それ以外の
//! 待ち方 — `gh pr checks --watch`、`gh run watch`、`until`/`while`/`sleep`/`watch` で
//! `gh pr checks` などを繰り返し引く形 — は、集約ジョブの完了まで NG が遅れる・required で
//! ないチェックの完了まで OK が出ない、のどちらかでセッションが終わらなくなるので、実行前に
//! deny する。対象は `Bash` と `Monitor`(`tool_input.command` を持つ)。
//!
//! 検出はトークン単位(`command_inspection`)。クォートされた文字列は 1 トークンになるので、
//! コミットメッセージや `echo "…"` の中の文言には当たらない。heredoc の本文も見ない。
//! `bash -c '<script>'` の中身は再帰して見る。トークン化できない(unmatched quote)コマンドは
//! 判定不能として通す(fail-open)。
//!
//! 逃げ道: PR に紐づかない workflow run(release など)を待つときのために、コマンドに
//! `CI_WAIT_GUARD_ALLOW=1` の語を足す(環境変数でも同じ)。静かな無効化ではなく、
//! コマンドに残るので、あとから読める。

use agent_session::hook::{jq_r_path, ToolCall};
use agent_session::{Agent, Outcome};
use command_inspection::command::{gh_command_at, is_gh};
use command_inspection::parse;
use command_inspection::shell::is_sep;

/// 逃げ道の語。環境変数名と、コマンドに足す `NAME=1` の両方で使う。
pub const ALLOW_ENV: &str = "CI_WAIT_GUARD_ALLOW";

/// 再帰して見る `sh -c '…'` の深さ上限。
const MAX_DEPTH: usize = 2;

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];

/// 待つ・繰り返すためのコマンド(コマンド位置にあるもの)。
const WAITERS: &[&str] = &["until", "while", "sleep", "watch"];

/// 直後の語がコマンド位置になる予約語(`do sleep 5`、`! gh pr checks` など)。
const TRANSPARENT: &[&str] = &["do", "then", "else", "!", "time", "exec", "{"];

const WATCH_HINT: &str = "CI を待つのに `gh pr checks --watch` / `gh run watch` は使いません。\
`pr-checks-wait <PR番号>` を run_in_background で実行し、完了通知を待ってください。\
赤が 1 件でも出れば即 NG(exit 1)、required が全緑になれば即 OK(exit 0)で終わります\
(`gh pr checks --required --watch` は集約ジョブ `CI passed` の完了まで NG が返らず、\
`--required` なしは required でないチェックの完了まで OK が出ません)。\
現在の状態を 1 回読むだけなら `gh pr checks <PR番号> --required`。\
PR に紐づかない workflow run(release など)を待つときだけ、コマンドに CI_WAIT_GUARD_ALLOW=1 を足してください。";

const POLL_HINT: &str = "CI を `until` / `while` / `sleep` / `watch` のポーリングで待ちません。\
`pr-checks-wait <PR番号>` を run_in_background で実行し、完了通知を待ってください。\
赤が 1 件でも出れば即 NG(exit 1)、required が全緑になれば即 OK(exit 0)で終わります。\
現在の状態を 1 回読むだけなら `gh pr checks <PR番号> --required`(ループに入れない)。\
PR に紐づかない workflow run(release など)を待つときだけ、コマンドに CI_WAIT_GUARD_ALLOW=1 を足してください。";

/// 環境変数 `CI_WAIT_GUARD_ALLOW=1` か。
pub fn allow_from_env() -> bool {
    std::env::var(ALLOW_ENV).is_ok_and(|v| v == "1")
}

/// `tokens[i]` がコマンド位置にあるか(先頭・区切りの直後・予約語の直後)。
fn at_command_position(tokens: &[String], i: usize) -> bool {
    i == 0 || is_sep(&tokens[i - 1]) || TRANSPARENT.contains(&tokens[i - 1].as_str())
}

/// `i` から次の区切りまで(区切りは含まない)。
fn segment(tokens: &[String], i: usize) -> &[String] {
    let end = tokens[i..]
        .iter()
        .position(|t| is_sep(t))
        .map_or(tokens.len(), |p| i + p);
    &tokens[i..end]
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// `bash -c` / `-lc` のようなコマンド文字列を取るフラグか。
fn is_command_flag(t: &str) -> bool {
    t.starts_with('-') && !t.starts_with("--") && t.len() >= 2 && t.ends_with('c')
}

/// 1 つの `gh …` が「CI の状態を引く」ものか(`i` は `gh` の位置)。
fn is_ci_query_at(tokens: &[String], i: usize) -> bool {
    if !is_gh(&tokens[i]) {
        return false;
    }
    let seg = segment(tokens, i);
    let any = |f: &dyn Fn(&str) -> bool| seg.iter().any(|t| f(t));
    gh_command_at(tokens, i, &["pr", "checks"])
        || gh_command_at(tokens, i, &["run", "view"])
        || gh_command_at(tokens, i, &["run", "list"])
        || (gh_command_at(tokens, i, &["pr", "view"]) && any(&|t| t.contains("statusCheckRollup")))
        || (gh_command_at(tokens, i, &["api"])
            && any(&|t| {
                t.contains("check-runs")
                    || t.contains("check-suites")
                    || t.ends_with("/status")
                    || t.contains("/statuses")
            }))
}

fn judge(tokens: &[String], depth: usize) -> Option<String> {
    if tokens.iter().any(|t| t == &format!("{ALLOW_ENV}=1")) {
        return None;
    }
    // `bash -c '<script>'` の中身。
    if depth < MAX_DEPTH {
        for i in 1..tokens.len().saturating_sub(1) {
            if is_command_flag(&tokens[i]) && SHELLS.contains(&basename(&tokens[i - 1])) {
                if let Some(inner) = parse(&tokens[i + 1]) {
                    if let Some(reason) = judge(&inner.tokens, depth + 1) {
                        return Some(reason);
                    }
                }
            }
        }
    }

    // 1. `gh pr checks … --watch` / `gh run watch`。
    for i in 0..tokens.len() {
        if gh_command_at(tokens, i, &["pr", "checks"])
            && segment(tokens, i)
                .iter()
                .any(|t| t == "--watch" || t.starts_with("--watch="))
        {
            return Some(WATCH_HINT.to_string());
        }
        if gh_command_at(tokens, i, &["run", "watch"]) {
            return Some(WATCH_HINT.to_string());
        }
    }

    // 2. 待つコマンド(`until`/`while`/`sleep`/`watch`)と CI の状態を引く `gh` が同じコマンドにある。
    let waits = (0..tokens.len())
        .any(|i| WAITERS.contains(&tokens[i].as_str()) && at_command_position(tokens, i));
    if waits && (0..tokens.len()).any(|i| is_ci_query_at(tokens, i)) {
        return Some(POLL_HINT.to_string());
    }
    None
}

/// コマンド文字列全体を判定する。deny なら理由を返す。`allow` は
/// `CI_WAIT_GUARD_ALLOW=1`(呼び出し側が env から決める)。
pub fn decide(cmd: &str, allow: bool) -> Option<String> {
    if allow {
        return None;
    }
    let parsed = parse(cmd)?;
    judge(&parsed.tokens, 0)
}

/// 判定の対象になるコマンド文字列。`Bash` のコマンド、または `Monitor` の `tool_input.command`
/// (`ws` の Monitor は `command` を持たないので対象外)。
fn command_of(call: &ToolCall) -> Option<String> {
    if let Some(c) = call.bash_command() {
        return Some(c);
    }
    if call.tool == "Monitor" && call.agent != Agent::Copilot {
        return jq_r_path(&call.raw, &["tool_input", "command"])
            .ok()
            .flatten()
            .filter(|c| !c.is_empty());
    }
    None
}

/// hook 本体。`hook-router` が同じ関数を呼ぶ。入力が読めない・対象外の tool などは `None`。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    let call = ToolCall::parse(agent, raw)?;
    let cmd = command_of(&call)?;
    decide(&cmd, allow_from_env()).map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied(cmd: &str) -> bool {
        decide(cmd, false).is_some()
    }

    #[test]
    fn denies_watch_forms() {
        for c in [
            "gh pr checks 12 --watch",
            "gh pr checks 12 --required --watch --fail-fast",
            "/usr/bin/gh pr checks --watch -R o/r",
            "gh pr checks 12 --watch=true",
            "gh run watch 123 --exit-status",
            "cd x && gh pr checks 12 --watch | tail -5",
        ] {
            let r = decide(c, false);
            assert_eq!(r.as_deref(), Some(WATCH_HINT), "{c}");
        }
    }

    #[test]
    fn denies_polling_loops() {
        for c in [
            // 2026-10-25 に実際に張られた形(#1001)
            "until ! gh pr checks 1000 | grep -q pending; do sleep 20; done",
            "while true; do s=$(gh pr checks 123 --json name,bucket); sleep 30; done",
            "sleep 30; gh pr checks 12",
            "for i in 1 2 3; do gh run view 99 --json status; sleep 10; done",
            "until gh api repos/o/r/commits/abc/check-runs --jq .total_count | grep -q 9; do sleep 5; done",
            "while sleep 10; do gh pr view 12 --json statusCheckRollup; done",
            "watch -n 30 gh pr checks 12",
            "bash -c 'until gh pr checks 12; do sleep 5; done'",
            "bash -lc \"gh run list --limit 1; sleep 5\"",
        ] {
            let r = decide(c, false);
            assert_eq!(r.as_deref(), Some(POLL_HINT), "{c}");
        }
    }

    #[test]
    fn allows_the_right_way_and_one_shot_reads() {
        for c in [
            "pr-checks-wait 12",
            "pr-checks-wait 12 --timeout 600 -R o/r",
            "gh pr checks 12",
            "gh pr checks 12 --required --json name,bucket",
            "gh pr view 12 --json state",
            "gh run view 99 --log-failed",
            "gh run list --limit 3",
            "gh api repos/o/r/rulesets",
            // 待つコマンドだけ・CI を引く gh だけは、それぞれ通す
            "sleep 5",
            "until false; do sleep 1; done",
            "tail -f build.log | grep --line-buffered ERROR",
            "pr-checks-wait 12 && gh pr checks 12 --required",
        ] {
            assert!(!denied(c), "{c}");
        }
    }

    #[test]
    fn quoted_mentions_and_heredoc_bodies_are_not_commands() {
        for c in [
            r#"git commit -m "docs: gh pr checks --watch を使わない""#,
            r#"echo "until gh pr checks 1; do sleep 1; done""#,
            r#"rg -n "gh run watch" docs"#,
            "git commit -F - <<'EOF'\nfix: gh pr checks --watch をやめる\nuntil gh pr checks 1; do sleep 1; done\nEOF",
            "gh pr create --title 'fix' --body 'gh run watch は使わない'",
        ] {
            assert!(!denied(c), "{c}");
        }
    }

    #[test]
    fn escape_hatch_and_fail_open() {
        assert!(decide("gh run watch 5", true).is_none());
        assert!(!denied(
            "CI_WAIT_GUARD_ALLOW=1 gh run watch 5 --exit-status"
        ));
        // 別の値は逃げ道にならない
        assert!(denied("CI_WAIT_GUARD_ALLOW=0 gh run watch 5"));
        // トークン化できない(unmatched quote)・空は判定不能 → 通す
        assert!(!denied("gh pr checks 12 --watch 'x"));
        assert!(!denied(""));
    }

    #[test]
    fn hook_reads_bash_and_monitor_only() {
        let bash = |c: &str| {
            serde_json::json!({"tool_name":"Bash","tool_input":{"command":c}}).to_string()
        };
        let monitor = |c: &str| {
            serde_json::json!({"tool_name":"Monitor","tool_input":{"command":c,"description":"d","timeout_ms":1000}})
                .to_string()
        };
        let is_deny = |o: Option<Outcome>| matches!(o, Some(Outcome::Deny(_)));
        assert!(is_deny(hook(
            Agent::Claude,
            &bash("gh pr checks 1 --watch")
        )));
        assert!(is_deny(hook(Agent::Codex, &bash("gh pr checks 1 --watch"))));
        assert!(is_deny(hook(
            Agent::Claude,
            &monitor("until ! gh pr checks 1 | grep -q pending; do sleep 20; done")
        )));
        assert!(hook(Agent::Claude, &monitor("pr-checks-wait 1")).is_none());
        assert!(hook(Agent::Claude, &monitor("tail -f log | grep ERROR")).is_none());
        // ws の Monitor は command を持たない。他の tool・壊れた JSON は見ない。
        let ws =
            r#"{"tool_name":"Monitor","tool_input":{"ws":{"url":"wss://x"},"description":"d"}}"#;
        assert!(hook(Agent::Claude, ws).is_none());
        let write = r#"{"tool_name":"Write","tool_input":{"command":"gh pr checks 1 --watch"}}"#;
        assert!(hook(Agent::Claude, write).is_none());
        assert!(hook(Agent::Claude, "{").is_none());
    }
}
