//! rulesets-write-guard — deny `gh api` writes to a repository's branch
//! rulesets endpoint from a raw Bash command (ADR-0000-rulesets-declaration-
//! in-repo D7).
//!
//! required_status_checks の正本を対象リポジトリ自身の
//! `.github/rulesets/*.json` に一本化した(D1)。その不変条件(宣言 ⊆
//! 実測 job 名)を PUT/POST の直前に検証するのは
//! `scripts/apply-rulesets.sh` の役目であり、Claude セッションが `gh api`
//! を直に叩いて ruleset を書き換える経路を残すとその検証を素通りできて
//! しまう。この hook はその直叩き経路だけを PreToolUse で deny する —
//! `apply-rulesets.sh` 自身(bash プロセスの中で行う `gh` 呼び出し)は
//! 別の OS プロセスとして実行され、Claude の Bash tool 呼び出し 1 回
//! (`bash scripts/apply-rulesets.sh ...`)の内側で完結するため、この hook
//! はそもそもそれを見ない(PreToolUse は Claude が直接発行する Bash
//! コマンド文字列だけを見る)。
//!
//! `crates/gh-edit-allow` と同じ「gh コマンド文字列を静的に解析し、判定
//! できない入力は素通しに倒す」設計を踏襲する(ADR-0024: github-audit の
//! ような巨大な既存 bash 資産を source する必要が無い hook は Rust が既定)。
//! 語分割は `hook_io::shell::split` を共有する。

use hook_io::{shell, HookInput, PermissionDecision};

const BYPASS_VAR: &str = "RULESETS_WRITE_GUARD_BYPASS";

const WRITE_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

// `gh api` が受ける主なフラグ。値を取るかどうかを知らないと後続の位置引数
// (path)を取り違えるため、未知のフラグを 1 つでも含むコマンドは判定しない
// (crates/gh-edit-allow/src/gh.rs と同じ安全側の縮退)。
const VALUE_FLAGS: &[&str] = &[
    "-H",
    "--header",
    "-F",
    "--raw-field",
    "-f",
    "--field",
    "--input",
    "-q",
    "--jq",
    "-t",
    "--template",
    "--hostname",
    "--cache",
    "-p",
    "--preview",
];
const BOOL_FLAGS: &[&str] = &[
    "-i",
    "--include",
    "--paginate",
    "--slurp",
    "--silent",
    "--verbose",
];

/// `NAME=value` の先頭語かどうか(env var 代入)。`shell::split` は `$` を
/// 含む語を既に弾いているので、ここでの値部分に展開は残らない。
fn as_env_assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    if name.is_empty()
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || name.as_bytes()[0].is_ascii_digit()
    {
        return None;
    }
    Some((name, value))
}

/// `repos/{owner}/{repo}/rulesets` または `repos/{owner}/{repo}/rulesets/{id}`
/// (先頭の `https://api.github.com/` と `/` は許容)かどうか。
fn is_rulesets_path(path: &str) -> bool {
    let p = path.strip_prefix("https://api.github.com/").unwrap_or(path);
    let p = p.strip_prefix('/').unwrap_or(p);
    let parts: Vec<&str> = p.split('/').collect();
    match parts.as_slice() {
        ["repos", owner, repo, "rulesets"] => !owner.is_empty() && !repo.is_empty(),
        ["repos", owner, repo, "rulesets", id] => {
            !owner.is_empty()
                && !repo.is_empty()
                && !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
        }
        _ => false,
    }
}

/// `words` は `gh api ...` の引数部分(先頭の `gh api` は含まない)。
/// 判定できなければ `None`。判定できれば (method, path) を返す。
fn parse_gh_api(words: &[String]) -> Option<(String, String)> {
    let mut method: Option<String> = None;
    let mut path: Option<String> = None;
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if let Some(rest) = w.strip_prefix("-X") {
            if !rest.is_empty() {
                method = Some(rest.to_ascii_uppercase());
                i += 1;
                continue;
            }
            let v = words.get(i + 1)?;
            method = Some(v.to_ascii_uppercase());
            i += 2;
            continue;
        }
        if w == "--method" {
            let v = words.get(i + 1)?;
            method = Some(v.to_ascii_uppercase());
            i += 2;
            continue;
        }
        if let Some(v) = w.strip_prefix("--method=") {
            method = Some(v.to_ascii_uppercase());
            i += 1;
            continue;
        }
        if VALUE_FLAGS.contains(&w) {
            i += 2;
            continue;
        }
        if BOOL_FLAGS.contains(&w) {
            i += 1;
            continue;
        }
        if w.starts_with("--") && w.contains('=') {
            // 未知の --flag=value 形は値ごと 1 語で消費する(gh の一般的な
            // ロングオプション形)。
            i += 1;
            continue;
        }
        if w.starts_with('-') {
            // 未知のフラグ — 位置引数を取り違えるおそれがあるため判定しない。
            return None;
        }
        if path.is_none() {
            path = Some(w.to_string());
        }
        i += 1;
    }
    let path = path?;
    let method = method.unwrap_or_else(|| "GET".to_string());
    Some((method, path))
}

/// PreToolUse: deny するなら理由付きの判定を返す。判定しない(deny しない)
/// なら `None` — この hook は allow/ask を一切出さない(deny-only)。
pub fn check(input: &HookInput) -> Option<PermissionDecision> {
    let mut words = shell::split(input.bash_command()?)?;

    // 先頭の env var 代入群をスキップしつつ、bypass 変数の有無を見る。
    let mut bypassed = false;
    while let Some((name, value)) = words.first().and_then(|w| as_env_assignment(w)) {
        if name == BYPASS_VAR && !value.is_empty() {
            bypassed = true;
        }
        words.remove(0);
    }
    if bypassed {
        return None;
    }

    if words.len() < 2 || words[0] != "gh" || words[1] != "api" {
        return None;
    }
    let (method, path) = parse_gh_api(&words[2..])?;
    if !WRITE_METHODS.contains(&method.as_str()) {
        return None;
    }
    if !is_rulesets_path(&path) {
        return None;
    }

    Some(PermissionDecision::deny(
        "ruleset の直接書換は deny。正本は対象リポジトリの .github/rulesets/*.json — \
         apply-rulesets.sh <owner/repo> [--reconcile] を使う \
         (docs/claude/rulesets-write-guard.md、rulesets-write-guard)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(cmd: &str) -> HookInput {
        let v = json!({
            "hook_event_name": "PreToolUse",
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
        });
        HookInput::parse(&v.to_string()).unwrap()
    }

    fn denies(cmd: &str) -> bool {
        check(&input(cmd)).is_some()
    }

    #[test]
    fn denies_write_methods_on_rulesets() {
        assert!(denies(
            "gh api -X PUT repos/tarotene/x/rulesets/12 --input -"
        ));
        assert!(denies("gh api --method POST repos/tarotene/x/rulesets"));
        assert!(denies("gh api -XDELETE repos/tarotene/x/rulesets/1"));
        assert!(denies(
            "gh api -X PUT https://api.github.com/repos/a/b/rulesets/1"
        ));
        assert!(denies("gh api -X PATCH repos/a/b/rulesets/1"));
        assert!(denies("gh api --method=PUT repos/a/b/rulesets/1"));
        assert!(denies("gh api repos/a/b/rulesets -X POST --input -"));
    }

    #[test]
    fn allows_read_and_unrelated_paths() {
        assert!(!denies("gh api -X GET repos/tarotene/x/rulesets/1"));
        assert!(!denies("gh api repos/tarotene/x/rulesets/1"));
        assert!(!denies("gh api repos/tarotene/x/rules/branches/main"));
        assert!(!denies("gh api -X PATCH repos/a/b"));
        assert!(!denies("gh pr view 1"));
        assert!(!denies("apply-rulesets.sh tarotene/x --reconcile"));
    }

    #[test]
    fn bypass_env_assignment_skips_judgement() {
        assert!(!denies(
            "RULESETS_WRITE_GUARD_BYPASS=1 gh api -X PUT repos/a/b/rulesets/1"
        ));
        // 値が空の代入は bypass 扱いしない。
        assert!(denies(
            "RULESETS_WRITE_GUARD_BYPASS= gh api -X PUT repos/a/b/rulesets/1"
        ));
        // 無関係な先頭の env var 代入は素通り解釈し、その後ろの gh api は
        // 引き続き判定する。
        assert!(denies("GH_TOKEN=x gh api -X PUT repos/a/b/rulesets/1"));
    }

    #[test]
    fn unparseable_input_is_not_judged() {
        // 複合コマンド・未知フラグ・非 Bash ツールは判定しない(fail-open)。
        assert!(!denies("echo x && gh api -X PUT repos/a/b/rulesets/1"));
        assert!(!denies(
            "gh api -X PUT repos/a/b/rulesets/1 --unknown-flag zzz"
        ));
        let i = HookInput::parse(
            &json!({"tool_name":"Read","tool_input":{"file_path":"x"}}).to_string(),
        )
        .unwrap();
        assert!(check(&i).is_none());
    }
}
