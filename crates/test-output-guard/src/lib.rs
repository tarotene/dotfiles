//! test-output-guard — `cargo test` / `cargo nextest` の出力を `| head` / `| tail` で切る形を
//! PreToolUse で止める判定(設計と根拠は docs/agents/test-output-guard.md)。
//!
//! `cargo test … | head -10` は、失敗を示す行(`FAILED`・`panicked`)が先頭 10 行の外に出ると見えず、
//! 落ちるテストを見落とす。さらにパイプの終了コードは `head` のものになり、失敗が成功に見える。
//! 2026-10-28 に、これで落ちるテストを PR #1199 に載せた(wrap-up の気づき)。止めるのは「切る」形だけで、
//! `| grep -E 'FAILED|panicked|test result'` のように必要な行を拾う形や、ファイルに書いて読む形は通す。
//!
//! 検出はトークン単位(`command_inspection`)。クォートされた文字列は 1 トークンになるので、
//! `echo "cargo test | head"` や コミットメッセージの中の文言には当たらない。heredoc の本文は見ない。
//! `bash -c '<script>'` の中身は再帰して見る。トークン化できないコマンドは判定不能として通す(fail-open)。
//!
//! 逃げ道: 切り詰めが要る正当な理由があるときは、コマンドに `TEST_OUTPUT_GUARD_ALLOW=1` の語を足す
//! (環境変数でも同じ)。コマンドに残るので、あとから読める。

use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use command_inspection::parse;
use command_inspection::shell::is_sep;

/// 逃げ道の語。環境変数名と、コマンドに足す `NAME=1` の両方で使う。
pub const ALLOW_ENV: &str = "TEST_OUTPUT_GUARD_ALLOW";

/// 再帰して見る `sh -c '…'` の深さ上限。
const MAX_DEPTH: usize = 2;

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];

/// `cargo` の後ろで、テストの出力を出すサブコマンド。
const TEST_SUBCOMMANDS: &[&str] = &["test", "t", "nextest"];

/// 出力を切るコマンド。
const TRUNCATORS: &[&str] = &["head", "tail"];

/// 出力をそのまま次へ流すコマンド(間に挟まっても、切る形は変わらない)。
const PASS_THROUGH: &[&str] = &["tee", "cat"];

/// 直後の語がコマンド位置になる予約語。
const TRANSPARENT: &[&str] = &["do", "then", "else", "!", "time", "exec", "{"];

const HINT: &str = "`cargo test` / `cargo nextest` の出力を `| head` / `| tail` で切りません。\
失敗を示す行(`FAILED`・`panicked`)が切れた範囲の外に出ると見えず、落ちるテストを見落とします\
(パイプの終了コードは `head` / `tail` のものなので、失敗が成功に見えます。2026-10-28、PR #1199)。\
絞るなら `cargo test … 2>&1 | grep -E 'FAILED|panicked|test result'` のように必要な行を拾うか、\
出力をファイルに書いて(`> $TMPDIR/test.log 2>&1`)終了コードを別に確かめ、必要な行だけを読んでください。\
切り詰めが要る正当な理由があるときだけ、コマンドに TEST_OUTPUT_GUARD_ALLOW=1 を足してください。";

/// 環境変数 `TEST_OUTPUT_GUARD_ALLOW=1` か。
pub fn allow_from_env() -> bool {
    std::env::var(ALLOW_ENV).is_ok_and(|v| v == "1")
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// `bash -c` / `-lc` のようなコマンド文字列を取るフラグか。
fn is_command_flag(t: &str) -> bool {
    t.starts_with('-') && !t.starts_with("--") && t.len() >= 2 && t.ends_with('c')
}

/// `NAME=value`(先頭が英字か `_` の名前 + `=`)。
fn is_assignment(w: &str) -> bool {
    let Some((name, _)) = w.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
}

/// パイプでつながった 1 つのコマンド(区切りの間の語)。`piped` は直前の区切りが単独の `|` か。
struct Segment<'a> {
    words: Vec<&'a str>,
    piped: bool,
}

/// トークン列を、区切りごとのコマンドに分ける。
///
/// 区切りは 1 文字ずつ別トークンになる(`&&` は `&` `&`、`||` は `|` `|`、`2>&1` は `2>` `&` `1`)。
/// 単独の `|`(前後が `|` でない)だけを、パイプとして数える。リダイレクトの `>&` / `<&` の `&` は
/// 区切りではないので、同じコマンドの一部として読み飛ばす。
fn segments(tokens: &[String]) -> Vec<Segment<'_>> {
    let mut out: Vec<Segment<'_>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut piped = false;
    for (i, t) in tokens.iter().enumerate() {
        let t = t.as_str();
        if !is_sep(t) {
            cur.push(t);
            continue;
        }
        let prev = i.checked_sub(1).map(|p| tokens[p].as_str());
        let next = tokens.get(i + 1).map(String::as_str);
        // `2>&1`・`>&2` の `&`。
        if t == "&" && prev.is_some_and(|p| p.ends_with('>') || p.ends_with('<')) {
            continue;
        }
        // `|&`(標準エラーも流すパイプ)の `&`。直前の `|` が単独のパイプのときだけ。
        if t == "&" && prev == Some("|") && i >= 2 && tokens[i - 2] != "|" {
            continue;
        }
        let is_pipe = t == "|" && prev != Some("|") && next != Some("|");
        if !cur.is_empty() {
            out.push(Segment {
                words: std::mem::take(&mut cur),
                piped,
            });
        }
        piped = is_pipe;
    }
    if !cur.is_empty() {
        out.push(Segment { words: cur, piped });
    }
    out
}

/// コマンド位置の語(先頭の `VAR=value`・`env`・予約語を読み飛ばした先)から始まる語の列。
fn command_words<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let skip = words
        .iter()
        .take_while(|w| TRANSPARENT.contains(w) || is_assignment(w) || **w == "env")
        .count();
    &words[skip..]
}

/// `cargo [+toolchain] [フラグ…] test|t|nextest …` か。
fn is_test_command(words: &[&str]) -> bool {
    let words = command_words(words);
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    if basename(first) != "cargo" {
        return false;
    }
    rest.iter()
        .find(|w| !w.starts_with('+') && !w.starts_with('-'))
        .is_some_and(|sub| TEST_SUBCOMMANDS.contains(sub))
}

fn command_name<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    command_words(words).first().map(|w| basename(w))
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

    // テストのコマンドから、`tee` / `cat` だけを挟んでパイプでつながった先に `head` / `tail` があるか。
    // `grep` などの絞り込みが間に入れば、それは「必要な行を拾う」形なので通す。
    let mut armed = false;
    for seg in segments(tokens) {
        if !seg.piped {
            armed = false;
        }
        if armed {
            match command_name(&seg.words) {
                Some(c) if TRUNCATORS.contains(&c) => return Some(HINT.to_string()),
                Some(c) if PASS_THROUGH.contains(&c) => {}
                _ => armed = false,
            }
        }
        if is_test_command(&seg.words) {
            armed = true;
        }
    }
    None
}

/// コマンド文字列全体を判定する。deny なら理由を返す。`allow` は
/// `TEST_OUTPUT_GUARD_ALLOW=1`(呼び出し側が env から決める)。
pub fn decide(cmd: &str, allow: bool) -> Option<String> {
    if allow {
        return None;
    }
    let parsed = parse(cmd)?;
    judge(&parsed.tokens, 0)
}

/// hook 本体。`hook-router` が同じ関数を呼ぶ。入力が読めない・Bash 以外の tool などは `None`。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    let call = ToolCall::parse(agent, raw)?;
    let cmd = call.bash_command()?;
    decide(&cmd, allow_from_env()).map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied(cmd: &str) -> bool {
        decide(cmd, false).is_some()
    }

    #[test]
    fn denies_truncated_test_output() {
        for c in [
            // 2026-10-28 に実際に打った形(PR #1199)
            "cargo test -p agent-settings-reconcile 2>&1 | head -10",
            "cargo test | head",
            "cargo test -p x 2>&1 | tail -20",
            "cargo nextest run --workspace | tail -n 5",
            "cargo +nightly test --workspace | head -30",
            "cargo --locked test | head",
            "cargo t | head -3",
            "cd crates/x && cargo test | head",
            "FOO=1 cargo test | head",
            "cargo test 2>&1 | tee $TMPDIR/t.log | head -20",
            "cargo test -p x |& head -5",
            "/usr/bin/cargo test | tail",
            "bash -c 'cargo test | head -5'",
            "time cargo test | head",
        ] {
            assert_eq!(decide(c, false).as_deref(), Some(HINT), "{c}");
        }
    }

    #[test]
    fn allows_filtering_and_file_output() {
        for c in [
            "cargo test 2>&1 | grep -E 'FAILED|panicked|test result'",
            "cargo test 2>&1 | grep -E 'FAILED|test result' | head -5",
            "cargo test 2>&1 | rg FAILED",
            "cargo test > $TMPDIR/test.log 2>&1",
            "cargo test -p x",
            "cargo test 2>&1 | tee $TMPDIR/t.log",
            "cargo build 2>&1 | head -20",
            "cargo fmt --all --check | head",
            "git log --oneline | head -5",
            "cat Cargo.toml | head -3",
            "cargo test; head -5 $TMPDIR/test.log",
            "cargo test && echo ok | head",
            "cargo test || echo failed | tail -1",
            "echo 'cargo test | head'",
            "git commit -m 'cargo test | head を拒否する'",
            "cargo run -- test | head",
        ] {
            assert!(!denied(c), "{c}");
        }
    }

    #[test]
    fn escape_hatch_by_word_and_by_env() {
        assert!(!denied("TEST_OUTPUT_GUARD_ALLOW=1 cargo test | head"));
        assert!(decide("cargo test | head", true).is_none());
    }

    #[test]
    fn unparsable_commands_pass() {
        assert!(!denied("cargo test | head 'unterminated"));
        assert!(!denied(""));
    }

    #[test]
    fn hook_reads_the_bash_command_only() {
        let bash = r#"{"tool_name":"Bash","tool_input":{"command":"cargo test | head"}}"#;
        let read = r#"{"tool_name":"Read","tool_input":{"file_path":"cargo test | head"}}"#;
        assert!(matches!(hook(Agent::Claude, bash), Some(Outcome::Deny(_))));
        assert!(hook(Agent::Claude, read).is_none());
    }
}
