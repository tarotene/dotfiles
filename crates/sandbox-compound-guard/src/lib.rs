//! sandbox-compound-guard — sandbox の外で動かす道具を、別のコマンドと連結した呼び出しを
//! PreToolUse で止め、「単体で呼ぶ」へ案内する判定(設計と根拠は
//! docs/agents/sandbox-compound-guard.md)。
//!
//! Claude Code の `sandbox.excludedCommands`(`gh *`・`git commit *`・`nix build *` など。正本は
//! `home/modules/claude.nix`)は、**呼び出し全体のコマンドが除外パターンに合わないと、全体が
//! sandbox の中に残る**。`gh auth status; echo "exit=$?"` は `gh` が認証できず HTTP 401、
//! `git add x && git commit` は署名が `No secret key`、`nix build … | tail` は `Read-only file
//! system` になる(2026-10-28、vega で実測。#1164)。文章の規範(`config/claude/CLAUDE.md`)は既に
//! あったが、1 セッションで 5 回以上破られた。この検査は、連結を実行前に止める。
//!
//! 検出はトークン単位(`command_inspection`)。クォートされた文字列は 1 トークンになるので、
//! `git commit -m "a; b"` や `echo "gh pr list | head"` には当たらない。heredoc の本文は見ない。
//! `for … do gh …; done` のような予約語の直後もコマンド位置として数える。`bash -c '<script>'`
//! の中身は見ない(それ自体が除外パターンに合わない 1 つのコマンドで、中で何を呼んでも
//! sandbox の中)。トークン化できないコマンドは判定不能として通す(fail-open)。
//!
//! 除外パターンは、実行時に `~/.config/agent-native/claude-permissions.json` の
//! `sandbox.excluded_commands`(home-manager が配備する、設定の正本の写し)から読む。
//! 一覧を持ち込まないので、除外の宣言を変えても、この検査の更新は要らない。ファイルが無い・
//! 読めない・空なら何も止めない(fail-open)。
//!
//! 文が 1 つの呼び出しでも、先頭の `VAR=value` や `env` を剥がすと除外パターンに当たるのに、
//! Claude Code が剥がす範囲(既知の安全な変数だけ。`SAFE_ENV`)では当たらないものは止める。
//! 前置が除外を打ち消し、`GIT_EDITOR=true git rebase --continue` が sandbox の中で動いて署名が
//! 失敗した(2026-11-05)。案内は「前置せず単体で呼ぶ」「常に要る変数は設定の `env` に宣言する」。
//!
//! 逃げ道: 連結が要る正当な理由があるときは、コマンドに `SANDBOX_COMPOUND_GUARD_ALLOW=1` の語を
//! 足す(呼び出し単位)。人が起動前に `SKIP_SANDBOX_COMPOUND_GUARD=1` を設定してもよい(セッション単位)。`dangerouslyDisableSandbox: true` の呼び出しは、sandbox の外で
//! 動くので止めない。Claude Code の sandbox にだけ当たる検査なので、他のエージェントは対象外。

use agent_session::escape::SANDBOX_COMPOUND_GUARD;
use agent_session::hook::{jq_r_path, ToolCall};
use agent_session::{Agent, Outcome};
use command_inspection::parse;
use command_inspection::shell::is_sep;
use std::path::{Path, PathBuf};

/// 除外パターンの一覧を差し替える環境変数（試験用。値はファイルのパス）。
pub const SPEC_ENV: &str = "SANDBOX_COMPOUND_GUARD_SPEC";

/// 直後の語がコマンド位置になる予約語（`do gh pr list`、`! gh auth status` など）。
const TRANSPARENT: &[&str] = &[
    "do", "then", "else", "elif", "if", "while", "until", "!", "time", "exec", "{", "(",
];

/// 除外パターンの一覧の置き場所。
fn spec_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(SPEC_ENV).filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join(".config/agent-native/claude-permissions.json"))
}

/// `sandbox.excluded_commands`。読めない・形が違うときは空（= 何も止めない）。
pub fn excluded_patterns() -> Vec<String> {
    let Some(path) = spec_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    patterns_from_json(&text)
}

fn patterns_from_json(text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    v.pointer("/sandbox/excluded_commands")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Claude Code が、権限規則の照合の前に剥がす「既知の安全な環境変数」の前置。一覧は公開されて
/// いない。Claude Code 2.1.285 の実行ファイルにある集合を写した(取得 2026-11-05。同じ実行
/// ファイルは、この集合の外の前置を "an environment variable prefix outside the safe list" と
/// 呼ぶ)。文書は「`NODE_ENV=test npm test` は `Bash(npm test *)` に合うが、allow 規則は他の変数の
/// 前置を越えて合わない」と書く(Anthropic, "Configure permissions"、Wrappers の節)。この外の
/// 前置は、除外パターンの照合でも剥がされないと推論する(下の `prefix_defeats_exclusion`)。
const SAFE_ENV: &[&str] = &[
    "GOEXPERIMENT",
    "GOOS",
    "GOARCH",
    "CGO_ENABLED",
    "GO111MODULE",
    "RUST_BACKTRACE",
    "RUST_LOG",
    "NODE_ENV",
    "PYTHONUNBUFFERED",
    "PYTHONDONTWRITEBYTECODE",
    "PYTEST_DISABLE_PLUGIN_AUTOLOAD",
    "PYTEST_DEBUG",
    "ANTHROPIC_API_KEY",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_TIME",
    "CHARSET",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "FORCE_COLOR",
    "TZ",
    "LS_COLORS",
    "LSCOLORS",
    "GREP_COLOR",
    "GREP_COLORS",
    "GCC_COLORS",
    "TIME_STYLE",
    "BLOCK_SIZE",
    "BLOCKSIZE",
    "COLUMNS",
    "LINES",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "CI",
    "DEBIAN_FRONTEND",
    "GIT_TERMINAL_PROMPT",
];

/// `NAME=value` の NAME が、安全な変数か。
fn is_safe_assignment(w: &str) -> bool {
    w.split_once('=')
        .is_some_and(|(name, _)| SAFE_ENV.contains(&name))
}

/// 文の先頭から、パターンと突き合わせるコマンドの語を取り出す。予約語は常に読み飛ばす。
/// `strip_all` なら、先頭の `VAR=value` と `env` も読み飛ばす(実際に動くコマンドを見る)。
/// そうでなければ、Claude Code が照合の前に剥がす安全な変数の前置だけを読み飛ばす
/// (Claude Code が見るコマンドの形)。
fn command_words(statement: &[String], strip_all: bool) -> Vec<&str> {
    let mut words = statement.iter().map(String::as_str).peekable();
    while let Some(w) = words.peek() {
        let skip = TRANSPARENT.contains(w)
            || (is_assignment(w) && (strip_all || is_safe_assignment(w)))
            || (strip_all && *w == "env");
        if !skip {
            break;
        }
        words.next();
    }
    words.collect()
}

/// 1 つの文（区切りの間のトークン列）が、除外パターンに当たるか。
///
/// パターンは空白区切りの語。末尾が `*` なら、それより前の語が文の先頭と一致すれば当たる
/// （`gh *` は `gh pr list` に、`git commit *` は `git commit -m x` に当たる）。`*` で
/// 終わらないパターンは、文全体の完全一致。先頭の `VAR=value`、`env`、予約語は読み飛ばす。
fn statement_is_excluded(statement: &[String], patterns: &[String]) -> bool {
    words_match(&command_words(statement, true), patterns)
}

/// 先頭の前置を剥がせば除外パターンに当たるが、Claude Code が剥がす範囲(安全な変数だけ)では
/// 当たらない文か。前置のせいで、その呼び出しは sandbox の中に残る(`GIT_EDITOR=true git rebase
/// --continue` は署名が `No secret key` で失敗した)。単体の呼び出しでも起きる。
fn prefix_defeats_exclusion(statement: &[String], patterns: &[String]) -> bool {
    statement_is_excluded(statement, patterns)
        && !words_match(&command_words(statement, false), patterns)
}

fn words_match(words: &[&str], patterns: &[String]) -> bool {
    if words.is_empty() {
        return false;
    }
    patterns.iter().any(|pattern| {
        let want: Vec<&str> = pattern.split_whitespace().collect();
        match want.split_last() {
            Some((&"*", prefix)) => {
                !prefix.is_empty()
                    && words.len() >= prefix.len()
                    && words.iter().zip(prefix).all(|(w, p)| w == p)
            }
            Some(_) => words == want,
            None => false,
        }
    })
}

/// `NAME=value`（先頭が英字か `_` の名前 + `=`）。
fn is_assignment(w: &str) -> bool {
    let Some((name, _)) = w.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
}

/// 区切りで分けた文の列。空の文は捨てる。
fn statements(tokens: &[String]) -> Vec<&[String]> {
    tokens
        .split(|t| is_sep(t))
        .filter(|s| !s.is_empty())
        .collect()
}

fn hint(tool: &str) -> String {
    format!(
        "`{tool}` は sandbox の外で動かす道具(sandbox.excludedCommands)ですが、別のコマンドと連結すると、\
呼び出し全体が除外パターンに合わず、sandbox の中に残ります(`gh` は HTTP 401、`git commit` は署名が \
`No secret key`、`nix` は `Read-only file system` で失敗します。#1164)。\
`;`・`&&`・`||`・`|`・ループ・改行で並べず、単体で呼んでください。\
出力の加工が要るときは `gh … --jq` のように道具側の機能を使い、複数のコマンドは別々の Bash 呼び出しに分けます\
(プロジェクトのディレクトリと追加したディレクトリの中では、`cd <dir> &&` は要りません。シェルの cwd が呼び出しをまたいで保たれるからです。\
それらの外の worktree では cwd が呼び出しごとに戻るので、承認を得て `dangerouslyDisableSandbox` で `cd <dir> && …` と呼びます)。\
{how_to}",
        how_to = SANDBOX_COMPOUND_GUARD.how_to().unwrap_or_default()
    )
}

/// 前置の拒否文。`prefix` は、剥がされない前置の語(`GIT_EDITOR=true`、`env`)。
fn prefix_hint(tool: &str, prefix: &str) -> String {
    format!(
        "`{prefix}` の前置は、Claude Code が除外パターンの照合の前に剥がす範囲(既知の安全な変数だけ)の外です。\
`{tool}` は sandbox の外で動かす道具(sandbox.excludedCommands)ですが、前置があると呼び出し全体が除外パターンに\
合わず、sandbox の中に残ります(`git rebase` の署名は `No secret key`、`gh` は HTTP 401 で失敗します。#1164)。\
前置せず、単体で呼んでください。常に要る変数は、前置でなく設定の `env` に宣言します\
(`dotfiles.claudePermissions.env`、docs/agents/agent-permissions.md。`GIT_EDITOR=true` は Claude Code が既に入れています)。\
`git -c core.editor=true rebase` のように `-c` で渡す形も、`git rebase *` に合わないので同じく sandbox の中に残ると考えられます(未実測)。\
{how_to}",
        how_to = SANDBOX_COMPOUND_GUARD.how_to().unwrap_or_default()
    )
}

/// 文の先頭にある、剥がされない前置の語(安全でない `VAR=value` と `env`)。
fn unstripped_prefix(statement: &[String]) -> String {
    let mut prefix = Vec::new();
    for w in statement.iter().map(String::as_str) {
        if is_assignment(w) || w == "env" {
            if w == "env" || !is_safe_assignment(w) {
                prefix.push(w);
            }
        } else if !TRANSPARENT.contains(&w) {
            break;
        }
    }
    prefix.join(" ")
}

/// コマンド文字列全体を判定する。deny なら理由を返す。
pub fn decide(cmd: &str, patterns: &[String], allow: bool) -> Option<String> {
    if allow || patterns.is_empty() {
        return None;
    }
    let parsed = parse(cmd)?;
    if SANDBOX_COMPOUND_GUARD
        .judge(None, false, Some(&parsed.tokens))
        .is_some()
    {
        return None;
    }
    let statements = statements(&parsed.tokens);
    if let [only] = statements.as_slice() {
        // 文が 1 つなら連結ではないが、前置が除外を打ち消すことがある。
        return prefix_defeats_exclusion(only, patterns)
            .then(|| prefix_hint(&display_name(only), &unstripped_prefix(only)));
    }
    if statements.len() < 2 {
        return None;
    }
    statements
        .iter()
        .find(|s| statement_is_excluded(s, patterns))
        .map(|s| hint(&display_name(s)))
}

/// 拒否の文に出す、除外対象のコマンドの語（`gh`、`git commit` など）。
fn display_name(statement: &[String]) -> String {
    let words: Vec<&str> = statement
        .iter()
        .map(String::as_str)
        .skip_while(|w| TRANSPARENT.contains(w) || is_assignment(w) || *w == "env")
        .take(2)
        .collect();
    match words.as_slice() {
        ["git", sub] | ["nix", sub] => format!("{} {sub}", words[0]),
        [first, ..] => (*first).to_owned(),
        [] => String::new(),
    }
}

/// `dangerouslyDisableSandbox: true` の呼び出しか（sandbox の外で動くので、連結してよい）。
fn runs_outside_sandbox(call: &ToolCall) -> bool {
    jq_r_path(&call.raw, &["tool_input", "dangerouslyDisableSandbox"])
        .ok()
        .flatten()
        .is_some_and(|v| v == "true")
}

/// hook 本体。`hook-router` が同じ関数を呼ぶ。入力が読めない・対象外の tool などは `None`。
pub fn hook(agent: Agent, raw: &str) -> Option<Outcome> {
    if agent != Agent::Claude {
        return None;
    }
    let call = ToolCall::parse(agent, raw)?;
    let cmd = call.bash_command()?;
    if runs_outside_sandbox(&call) {
        return None;
    }
    decide(
        &cmd,
        &excluded_patterns(),
        SANDBOX_COMPOUND_GUARD.active(None, None).is_some(),
    )
    .map(Outcome::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns() -> Vec<String> {
        [
            "gh *",
            "git commit *",
            "git push *",
            "git add *",
            "git fetch *",
            "git rebase *",
            "nix build *",
            "nix fmt *",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn denies_a_single_call_whose_prefix_defeats_the_exclusion() {
        // 2026-11-05: `GIT_EDITOR=true git rebase --continue` が sandbox の中で動き、署名が失敗した。
        for (c, prefix) in [
            ("GIT_EDITOR=true git rebase --continue", "`GIT_EDITOR=true`"),
            ("JUST_TEMPDIR=/x gh pr list", "`JUST_TEMPDIR=/x`"),
            ("env FOO=1 git push origin b", "`env FOO=1`"),
            // 安全な変数は剥がされるので、安全でない方だけを名指す。
            (
                "LANG=C GIT_EDITOR=true git rebase main",
                "`GIT_EDITOR=true`",
            ),
            ("! GIT_EDITOR=true git add x", "`GIT_EDITOR=true`"),
        ] {
            let r = reason(c).unwrap_or_else(|| panic!("{c}"));
            assert!(r.contains(prefix), "{c}: {r}");
            assert!(r.contains("前置せず、単体で呼んでください"), "{c}");
            assert!(r.contains("設定の `env` に宣言"), "{c}");
            assert!(r.contains("git -c core.editor=true rebase"), "{c}");
        }
        assert!(reason("GIT_EDITOR=true git rebase --continue")
            .unwrap()
            .contains("`git rebase` は sandbox の外"));
    }

    #[test]
    fn passes_a_prefix_that_changes_nothing() {
        for c in [
            // 安全な変数は Claude Code が剥がすので、除外はそのまま効く。
            "LANG=C git rebase main",
            "CI=1 gh pr list",
            "NODE_ENV=test TZ=UTC git fetch origin",
            // 前置があっても、除外対象のコマンドでなければ関係ない。
            "GIT_EDITOR=true git status",
            "JUST_TEMPDIR=/x just ci",
            "UV_TOOL_DIR=/x uvx ruff --version",
            "env FOO=1 ls",
            // 前置の無い単体の呼び出し。
            "git rebase --continue",
            // 逃げ道。
            "SANDBOX_COMPOUND_GUARD_ALLOW=1 GIT_EDITOR=true git rebase --continue",
        ] {
            assert_eq!(reason(c), None, "{c}");
        }
    }

    fn reason(cmd: &str) -> Option<String> {
        decide(cmd, &patterns(), false)
    }

    #[test]
    fn denies_the_forms_that_failed_on_2026_10_28() {
        for c in [
            // #1164 の実測と、1 セッションで繰り返された形
            "gh auth status; echo \"exit=$?\"",
            "gh issue list -R o/r --json number | sort",
            "git add crates/x && git commit -m msg",
            "cd crates && gh pr list",
            "nix build .#x --no-link 2>&1 | tail -5",
            "for n in 1 2 3; do gh pr view $n; done",
            "gh issue view 5 ; git switch -c b",
            "git fetch origin\ngit rebase origin/main",
            "FOO=1 git push origin b && echo done",
            "true || gh pr checks 12",
        ] {
            let r = reason(c);
            assert!(r.is_some(), "{c}");
            assert!(r.unwrap().contains("単体で呼んでください"), "{c}");
        }
    }

    #[test]
    fn names_the_excluded_command_in_the_reason() {
        assert!(reason("gh auth status; echo x").unwrap().contains("`gh`"));
        assert!(reason("git add a && git commit -m m")
            .unwrap()
            .contains("`git add`"));
        assert!(reason("echo a; nix build .#x")
            .unwrap()
            .contains("`nix build`"));
    }

    #[test]
    fn passes_single_commands_and_unrelated_compounds() {
        for c in [
            "gh auth status",
            "gh issue list -R o/r --json number --jq '.[].number'",
            "git commit -m \"a; b && c | d\"",
            "git commit -m \"$(cat <<'EOF'\nfix: x; y\nEOF\n)\"",
            "echo \"gh pr list | head\"",
            "cargo test -p x && cargo clippy",
            "git status | head",
            "ls; pwd",
            "git log --oneline -3 | cat",
            "git -C /tmp/x log | head",
        ] {
            assert_eq!(reason(c), None, "{c}");
        }
    }

    #[test]
    fn exact_patterns_need_a_full_match() {
        let p = vec!["nix flake check".to_owned()];
        assert!(decide("nix flake check; echo x", &p, false).is_some());
        assert!(decide("nix flake check --all; echo x", &p, false).is_none());
    }

    #[test]
    fn escape_hatches() {
        assert_eq!(reason("SANDBOX_COMPOUND_GUARD_ALLOW=1 gh a; echo x"), None);
        assert_eq!(
            decide("gh auth status; echo x", &patterns(), true),
            None,
            "env"
        );
        assert_eq!(decide("gh a; echo x", &[], false), None, "no patterns");
    }

    #[test]
    fn unparsable_commands_pass() {
        assert_eq!(reason("gh pr list; echo \"unterminated"), None);
    }

    #[test]
    fn reads_patterns_from_the_deployed_spec() {
        let json = r#"{"sandbox":{"excluded_commands":["gh *","git commit *"],"enabled":true}}"#;
        assert_eq!(patterns_from_json(json), ["gh *", "git commit *"]);
        for bad in [
            "",
            "{",
            r#"{"sandbox":{}}"#,
            r#"{"sandbox":{"excluded_commands":"x"}}"#,
        ] {
            assert!(patterns_from_json(bad).is_empty(), "{bad}");
        }
    }

    fn call(command: &str, extra: &str) -> String {
        format!(
            r#"{{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{{"command":{}{extra}}},"cwd":"/"}}"#,
            serde_json::to_string(command).unwrap()
        )
    }

    #[test]
    fn hook_reads_the_spec_and_honours_dangerously_disable_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let spec = dir.path().join("claude-permissions.json");
        std::fs::write(&spec, r#"{"sandbox":{"excluded_commands":["gh *"]}}"#).unwrap();
        // 環境変数はプロセス全体のものなので、この試験だけが触る。
        std::env::set_var(SPEC_ENV, &spec);
        let compound = call("gh auth status; echo x", "");
        let outcome = hook(Agent::Claude, &compound);
        assert!(matches!(outcome, Some(Outcome::Deny(_))));
        // sandbox の外で動く呼び出しは止めない。
        let outside = call(
            "gh auth status; echo x",
            r#","dangerouslyDisableSandbox":true"#,
        );
        assert!(hook(Agent::Claude, &outside).is_none());
        // Claude Code の sandbox にだけ当たる。
        assert!(hook(Agent::Codex, &compound).is_none());
        // 単体の呼び出しは通る。
        assert!(hook(Agent::Claude, &call("gh auth status", "")).is_none());
        // ファイルが無ければ止めない（fail-open）。
        std::env::set_var(SPEC_ENV, dir.path().join("absent.json"));
        assert!(hook(Agent::Claude, &compound).is_none());
        std::env::remove_var(SPEC_ENV);
    }
}
