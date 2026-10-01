//! bash 版 `repo-create-guard.sh --selftest` 全 9 ケースと、`main()` の
//! バイパス・縮退を実バイナリに対して固定する(#415)。全ケースは bash 版で
//! 緑を確認済み。

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_repo-create-guard");

fn run_env(cmd_json: &str, bypass: Option<&str>) -> Option<String> {
    let mut c = Command::new(BIN);
    c.stdin(Stdio::piped()).stdout(Stdio::piped());
    match bypass {
        Some(v) => c.env("REPO_CREATE_GUARD_BYPASS", v),
        None => c.env_remove("REPO_CREATE_GUARD_BYPASS"),
    };
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(cmd_json.as_bytes())
        .unwrap();
    let o = ch.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    if o.stdout.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    Some(
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

fn run(cmd: &str) -> Option<String> {
    let j = serde_json::json!({"tool_name":"Bash","tool_input":{"command":cmd}}).to_string();
    run_env(&j, None)
}

fn pass(name: &str, cmd: &str) {
    let out = run(cmd);
    assert!(out.is_none(), "{name}: expected pass, denied: {out:?}");
}

fn deny(name: &str, cmd: &str, needle: &str) -> String {
    let out = run(cmd).unwrap_or_else(|| panic!("{name}: expected deny, but passed"));
    assert!(out.contains(needle), "{name}: [{needle}] not in {out}");
    out
}

#[test]
fn selftest_cases() {
    deny(
        "1 素の gh repo create",
        "gh repo create acme/foo --private",
        "gh repo create",
    );
    pass(
        "2 gh repo edit は対象外",
        "gh repo edit acme/foo --description x",
    );
    pass("3 gh repo view は対象外", "gh repo view acme/foo");
    deny(
        "4 gh api -X POST user/repos",
        "gh api -X POST user/repos -f name=foo",
        "repo 作成",
    );
    deny(
        "5 gh api --method POST orgs/*/repos",
        "gh api --method POST orgs/acme/repos -f name=foo",
        "repo 作成",
    );
    pass("6 GET user/repos は一覧取得", "gh api -X GET user/repos");
    pass(
        "7 PATCH repos/o/r は settings 変更",
        "gh api -X PATCH repos/acme/foo --input -",
    );
    pass(
        "8 rulesets への POST は無関係",
        "gh api -X POST repos/acme/foo/rulesets --input -",
    );
    pass("9 コマンド位置外の綴り", "echo 'gh repo create acme/foo'");
}

#[test]
fn reason_texts_are_verbatim() {
    let cli = deny("cli", "gh repo create x", "REPO_CREATE_GUARD_BYPASS=1");
    assert!(cli.starts_with("素の `gh repo create` は使わないでください。\n\nrepo-charter スキルの手順(config/claude/skills/repo-charter/SKILL.md)\nを経由してください:\n\n  1. §1 の命名インタビュー"));
    assert!(cli.ends_with("(ADR-0013 Amendment 2026-09-29 参照)。"));
    let api = deny("api", "gh api -X POST /orgs/acme/repos", "");
    assert!(api.starts_with("`gh api` での repo 作成(/orgs/acme/repos)は使わないでください。\n\n"));
}

#[test]
fn compound_and_path_forms() {
    deny(
        "複合コマンドの 2 番目",
        "git push && gh repo create a/b",
        "gh repo create",
    );
    deny(
        "フルパスの gh",
        "/usr/bin/gh repo create a/b",
        "gh repo create",
    );
    deny(
        "method 小文字",
        "gh api --method=post user/repos",
        "repo 作成",
    );
    // 明示の POST が無ければ(既定メソッドは推測しない)通す
    pass("method 無し", "gh api user/repos -f name=x");
    pass("別の gh サブコマンド", "gh repo clone a/b");
    pass("unmatched quote は判定不能", "gh repo create 'x");
}

#[test]
fn bypass_and_degraded_inputs() {
    let j = serde_json::json!({"tool_name":"Bash","tool_input":{"command":"gh repo create a/b"}})
        .to_string();
    assert!(run_env(&j, None).is_some());
    assert!(run_env(&j, Some("1")).is_none());
    // 空文字なら有効(bash の `${VAR:-}` が空)
    assert!(run_env(&j, Some("")).is_some());
    for s in [
        "",
        "{x",
        "{}",
        r#"{"tool_name":"mcp__github__x","tool_input":{}}"#,
        r#"{"tool_name":"Bash","tool_input":{"command":""}}"#,
    ] {
        assert!(run_env(s, None).is_none(), "{s}");
    }
}
