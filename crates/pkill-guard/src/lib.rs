//! pkill-guard — `pkill -f` / `pgrep -f` の検索語が、実行する Bash のコマンド行そのものに当たる形を
//! PreToolUse で止める判定(設計と根拠は docs/agents/pkill-guard.md)。
//!
//! `-f` はプロセスのコマンド行全体を探す。エージェントが打った Bash のコマンド文字列は、実行する
//! シェル自身のコマンド行(`bash -c '<コマンド>'`)にそのまま入るので、検索語がそこにも当たり、`pkill -f` は
//! 自分のシェルを止める(終了コード 143・144。2026-10-31 に同じ事故を 2 回した)。`pgrep -f` は自分のシェルの
//! 行にだけ当たって「動いている」と誤判定する(2026-10-28)。
//!
//! 止めるのは「検索語(正規表現)が、検索語そのものの文字列に当たる」形だけ。検索語はコマンド文字列に
//! そのまま現れるので、これが自分のコマンド行に当たる条件になる。括弧の技 `pkill -f '[t]rial.sh'`
//! (1 文字を `[]` で囲む)は、正規表現としては `trial.sh` に当たるが、文字列 `[t]rial.sh` には当たらないので通す。
//! `-x`(完全一致)は、コマンド行全体が検索語に一致する必要があるので通す。
//!
//! 検出はトークン単位(`command_inspection`)。クォートされた文字列は 1 トークンになるので、
//! `echo "pkill -f foo"` やコミットメッセージの中の文言には当たらない。heredoc の本文は見ない。
//! `bash -c '<script>'` の中身は再帰して見る。トークン化できないコマンド・正規表現として読めない検索語は
//! 判定不能として通す(fail-open)。
//!
//! 逃げ道: 正当な理由があるときは、コマンドに `PKILL_GUARD_ALLOW=1` の語を足す(呼び出し単位)。人が起動前に
//! `SKIP_PKILL_GUARD=1` を設定してもよい(セッション単位)。名前と真偽は `agent_session::escape` の表が決める。
//! コマンドに残るので、あとから読める。

use agent_session::escape::PKILL_GUARD;
use agent_session::hook::ToolCall;
use agent_session::{Agent, Outcome};
use command_inspection::parse;
use command_inspection::shell::is_sep;
use regex::Regex;

/// 再帰して見る `sh -c '…'` の深さ上限。
const MAX_DEPTH: usize = 2;

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];

/// コマンド行で探すコマンド。
const PROC_COMMANDS: &[&str] = &["pkill", "pgrep"];

/// 直後の語がコマンド位置になる予約語。
const TRANSPARENT: &[&str] = &["do", "then", "else", "!", "time", "exec", "{"];

/// 値を 1 つ取る短いオプション(`-u USER` など)。値は検索語ではない。
const VALUE_SHORT: &[char] = &['u', 'U', 'g', 'G', 'P', 's', 't', 'T', 'F', 'O'];

/// 値を 1 つ取る長いオプション。
const VALUE_LONG: &[&str] = &[
    "--euid",
    "--uid",
    "--pgroup",
    "--group",
    "--parent",
    "--session",
    "--terminal",
    "--nslist",
    "--ns",
    "--pidfile",
    "--older",
    "--signal",
];

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

/// 区切り(`;`・`&&`・`||`・`|`・改行)ごとのコマンド。`2>&1` の `&` は区切りではない。
fn segments(tokens: &[String]) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let t = t.as_str();
        if !is_sep(t) {
            cur.push(t);
            continue;
        }
        let prev = i.checked_sub(1).map(|p| tokens[p].as_str());
        if t == "&" && prev.is_some_and(|p| p.ends_with('>') || p.ends_with('<')) {
            continue;
        }
        if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
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

/// `pkill` / `pgrep` の引数の読み取り結果。
#[derive(Debug, Default, PartialEq, Eq)]
struct Args<'a> {
    /// `-f` / `--full`(コマンド行全体を探す)。
    full: bool,
    /// `-x` / `--exact`(コマンド行全体の完全一致)。
    exact: bool,
    /// `-i` / `--ignore-case`。
    ignore_case: bool,
    /// 最初の位置引数(検索語)。
    pattern: Option<&'a str>,
}

fn parse_args<'a>(args: &[&'a str]) -> Args<'a> {
    let mut a = Args::default();
    let mut i = 0;
    while i < args.len() {
        let w = args[i];
        i += 1;
        if let Some(long) = w.strip_prefix("--") {
            match long {
                "full" => a.full = true,
                "exact" => a.exact = true,
                "ignore-case" => a.ignore_case = true,
                _ => {}
            }
            if VALUE_LONG.contains(&w) {
                i += 1; // 値(`--signal TERM` など)。`--signal=TERM` の形は 1 語なので読み飛ばさない
            }
        } else if let Some(short) = w.strip_prefix('-').filter(|s| !s.is_empty()) {
            // `-9`・`-TERM` のようなシグナルや `-fx` のような束ね書き。
            for (n, c) in short.chars().enumerate() {
                match c {
                    'f' => a.full = true,
                    'x' => a.exact = true,
                    'i' => a.ignore_case = true,
                    c if VALUE_SHORT.contains(&c) => {
                        // 値は同じ語の残り(`-uroot`)か、次の語(`-u root`)。
                        if n + 1 == short.chars().count() {
                            i += 1;
                        }
                        break;
                    }
                    _ => {}
                }
            }
        } else if a.pattern.is_none() {
            a.pattern = Some(w);
        }
    }
    a
}

/// 検索語の最初の英数字を `[]` で囲んだ、自分のコマンド行に当たらない形。
fn bracketed(pattern: &str) -> String {
    let mut out = String::new();
    let mut done = false;
    for c in pattern.chars() {
        if !done && c.is_ascii_alphanumeric() {
            out.push('[');
            out.push(c);
            out.push(']');
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn hint(cmd: &str, pattern: &str) -> String {
    let safe = bracketed(pattern);
    format!(
        "`{cmd} -f '{pattern}'` の検索語は、実行するシェル自身のコマンド行にも当たります。\
エージェントが打った Bash のコマンド文字列は、シェルのコマンド行(`bash -c '<コマンド>'`)にそのまま入るので、\
`pkill -f` は自分のシェルを止め(終了コード 143・144。2026-10-31 に 2 回)、`pgrep -f` は自分のシェルの行にだけ当たって\
「動いている」と誤判定します(2026-10-28)。\
検索語の 1 文字を `[]` で囲むと、正規表現としては同じ語に当たり、検索語の文字列そのものには当たらなくなります: \
`{cmd} -f '{safe}'`。完全一致で足りるなら `-x` を付ける(`-fx`)か、`-f` を外してプロセス名で探してください。\
{how_to}",
        how_to = PKILL_GUARD.how_to().unwrap_or_default()
    )
}

/// 1 つのコマンドが、自分のコマンド行に当たる `-f` の検索語を持つか。持てば理由を返す。
fn judge_segment(words: &[&str]) -> Option<String> {
    let words = command_words(words);
    let (first, rest) = words.split_first()?;
    let cmd = basename(first);
    if !PROC_COMMANDS.contains(&cmd) {
        return None;
    }
    let a = parse_args(rest);
    if !a.full || a.exact {
        return None;
    }
    let pattern = a.pattern?;
    let source = if a.ignore_case {
        format!("(?i){pattern}")
    } else {
        pattern.to_string()
    };
    // 検索語が正規表現として読めなければ、判定できないので通す。
    let re = Regex::new(&source).ok()?;
    re.is_match(pattern).then(|| hint(cmd, pattern))
}

fn judge(tokens: &[String], depth: usize) -> Option<String> {
    if PKILL_GUARD.judge(None, false, Some(tokens)).is_some() {
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
    segments(tokens)
        .iter()
        .find_map(|words| judge_segment(words))
}

/// コマンド文字列全体を判定する。deny なら理由を返す。`allow` は、セッション単位の逃げ道
/// (`SKIP_PKILL_GUARD=1`。呼び出し側が env から決める)。呼び出し単位の `PKILL_GUARD_ALLOW=1` は、
/// コマンドの語から、ここで見る。
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
    decide(&cmd, PKILL_GUARD.active(None, None).is_some()).map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied(cmd: &str) -> bool {
        decide(cmd, false).is_some()
    }

    #[test]
    fn denies_a_pattern_that_matches_its_own_text() {
        for c in [
            // 2026-10-31 に実際に打った形(exit 144・143)
            "pkill -f trial.sh",
            "pkill -f tC/cl2/db",
            // 2026-10-28: 自分のシェルの行にだけ当たった
            "pgrep -f obsidian",
            "pkill -9 -f foo",
            "pkill -TERM -f foo",
            "pkill -f 'my server'",
            "pkill -f \"my server\"",
            "pkill -fi foo",
            "pkill -u root -f foo",
            "pkill -uroot -f foo",
            "pkill --signal TERM --full foo",
            "cd /tmp && pkill -f foo",
            "FOO=1 pkill -f foo",
            "/usr/bin/pkill -f foo",
            "echo start; pkill -f foo; echo done",
            "bash -c 'pkill -f foo'",
            "kill -0 1 || pkill -f foo",
            "pkill -f 'foo.*bar'",
            "pkill -i -f FOO",
        ] {
            assert!(denied(c), "{c}");
        }
    }

    #[test]
    fn allows_the_bracket_trick_and_other_safe_forms() {
        for c in [
            // 括弧の技: 1 文字を [] で囲む
            "pkill -f '[t]rial.sh'",
            "pkill -f '[t]C/cl2/db'",
            "pgrep -f '[o]bsidian'",
            "pkill -9 -f '[f]oo'",
            // `-f` が無い(プロセス名で探す)
            "pkill foo",
            "pkill -9 foo",
            "pgrep obsidian",
            // 完全一致
            "pkill -fx 'foo bar'",
            "pkill -x -f 'foo bar'",
            "pgrep --full --exact 'a b'",
            // 先頭・末尾の固定は、検索語の文字列そのものには当たらない
            "pkill -f '^foo'",
            // 変数の展開(中身は分からない)
            "pkill -f \"$PATTERN\"",
            // 引数の値が検索語ではない
            "pkill -u root",
            "pgrep -f",
            // 別のコマンド
            "echo pkill -f foo",
            "git commit -m 'pkill -f foo を拒否する'",
            "grep -f patterns.txt file",
            "ps aux | grep foo",
            "kill 1234",
        ] {
            assert!(!denied(c), "{c}");
        }
    }

    #[test]
    fn the_hint_suggests_the_bracketed_form() {
        let r = decide("pkill -f trial.sh", false).unwrap();
        assert!(r.contains("pkill -f '[t]rial.sh'"), "{r}");
        let r = decide("pgrep -f /usr/bin/foo", false).unwrap();
        assert!(
            r.contains("pgrep -f '/usr/bin/[f]oo'") || r.contains("[u]sr"),
            "{r}"
        );
        assert!(r.contains("PKILL_GUARD_ALLOW=1"), "{r}");
    }

    #[test]
    fn the_suggested_form_is_itself_allowed() {
        for p in ["trial.sh", "tC/cl2/db", "obsidian", "my server", "foo.*bar"] {
            let cmd = format!("pkill -f '{p}'");
            let safe = bracketed(p);
            assert!(denied(&cmd), "{cmd}");
            assert!(!denied(&format!("pkill -f '{safe}'")), "{safe}");
        }
    }

    #[test]
    fn escape_hatch_by_word_and_by_env() {
        assert!(!denied("PKILL_GUARD_ALLOW=1 pkill -f foo"));
        assert!(decide("pkill -f foo", true).is_none());
    }

    #[test]
    fn unparsable_commands_and_patterns_pass() {
        assert!(!denied("pkill -f 'unterminated"));
        assert!(!denied(""));
        // 正規表現として読めない検索語は、判定できないので通す。
        assert!(!denied("pkill -f 'foo(bar'"));
    }

    #[test]
    fn parse_args_reads_flags_and_the_first_positional() {
        let a = parse_args(&["-9", "-fx", "pat", "other"]);
        assert!(a.full && a.exact && !a.ignore_case);
        assert_eq!(a.pattern, Some("pat"));
        let a = parse_args(&["-u", "root", "pat"]);
        assert!(!a.full);
        assert_eq!(a.pattern, Some("pat"));
        let a = parse_args(&["--signal", "TERM", "--full", "pat"]);
        assert!(a.full);
        assert_eq!(a.pattern, Some("pat"));
    }

    #[test]
    fn hook_reads_the_bash_command_only() {
        let bash = r#"{"tool_name":"Bash","tool_input":{"command":"pkill -f foo"}}"#;
        let read = r#"{"tool_name":"Read","tool_input":{"file_path":"pkill -f foo"}}"#;
        assert!(matches!(hook(Agent::Claude, bash), Some(Outcome::Deny(_))));
        assert!(hook(Agent::Claude, read).is_none());
    }
}
