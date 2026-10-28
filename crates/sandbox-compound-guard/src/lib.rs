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
//! 逃げ道: 連結が要る正当な理由があるときは、コマンドに `SANDBOX_COMPOUND_GUARD_ALLOW=1` の語を
//! 足す(環境変数でも同じ)。`dangerouslyDisableSandbox: true` の呼び出しは、sandbox の外で
//! 動くので止めない。Claude Code の sandbox にだけ当たる検査なので、他のエージェントは対象外。

use agent_session::hook::{jq_r_path, ToolCall};
use agent_session::{Agent, Outcome};
use command_inspection::parse;
use command_inspection::shell::is_sep;
use std::path::{Path, PathBuf};

/// 逃げ道の語。環境変数名と、コマンドに足す `NAME=1` の両方で使う。
pub const ALLOW_ENV: &str = "SANDBOX_COMPOUND_GUARD_ALLOW";

/// 除外パターンの一覧を差し替える環境変数（試験用。値はファイルのパス）。
pub const SPEC_ENV: &str = "SANDBOX_COMPOUND_GUARD_SPEC";

/// 直後の語がコマンド位置になる予約語（`do gh pr list`、`! gh auth status` など）。
const TRANSPARENT: &[&str] = &[
    "do", "then", "else", "elif", "if", "while", "until", "!", "time", "exec", "{", "(",
];

/// 環境変数 `SANDBOX_COMPOUND_GUARD_ALLOW=1` か。
pub fn allow_from_env() -> bool {
    std::env::var(ALLOW_ENV).is_ok_and(|v| v == "1")
}

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

/// 1 つの文（区切りの間のトークン列）が、除外パターンに当たるか。
///
/// パターンは空白区切りの語。末尾が `*` なら、それより前の語が文の先頭と一致すれば当たる
/// （`gh *` は `gh pr list` に、`git commit *` は `git commit -m x` に当たる）。`*` で
/// 終わらないパターンは、文全体の完全一致。先頭の `VAR=value`、`env`、予約語は読み飛ばす。
fn statement_is_excluded(statement: &[String], patterns: &[String]) -> bool {
    let mut words = statement.iter().map(String::as_str).peekable();
    while let Some(w) = words.peek() {
        if TRANSPARENT.contains(w) || is_assignment(w) || *w == "env" {
            words.next();
        } else {
            break;
        }
    }
    let words: Vec<&str> = words.collect();
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
(`cd <dir> &&` は要りません。シェルの cwd は呼び出しをまたいで保たれます)。\
連結が要る正当な理由があるときだけ、コマンドに {ALLOW_ENV}=1 を足してください。"
    )
}

/// コマンド文字列全体を判定する。deny なら理由を返す。
pub fn decide(cmd: &str, patterns: &[String], allow: bool) -> Option<String> {
    if allow || patterns.is_empty() {
        return None;
    }
    let parsed = parse(cmd)?;
    if parsed.tokens.iter().any(|t| t == &format!("{ALLOW_ENV}=1")) {
        return None;
    }
    let statements = statements(&parsed.tokens);
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
    decide(&cmd, &excluded_patterns(), allow_from_env()).map(Outcome::Deny)
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
            "nix build *",
            "nix fmt *",
        ]
        .map(str::to_owned)
        .to_vec()
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
