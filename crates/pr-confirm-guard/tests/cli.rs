//! bash 版 `pr-confirm-guard.sh --selftest` 全 19 ケースと、Codex adapter
//! `--selftest` の 3 ケースを実バイナリに対して固定する(#415)。
//! 加えて、Claude/Codex の project 解決・縮退(bash の main / main_codex)。
//!
//! 全ケースは bash 版に対して緑を確認してから移した。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_pr-confirm-guard");

/// `--check '<cmd>'`。(終了コード, stdout)
fn check(cmd: &str, allow: bool) -> (i32, String) {
    let mut c = Command::new(BIN);
    c.args(["--check", cmd])
        .env_remove("PR_CONFIRM_GUARD_ALLOW");
    if allow {
        c.env("PR_CONFIRM_GUARD_ALLOW", "1");
    }
    let o = c.output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn assert_pass(name: &str, cmd: &str) {
    let (rc, out) = check(cmd, false);
    assert_eq!((rc, out.as_str()), (0, "pass\n"), "{name}");
}

fn assert_deny(name: &str, cmd: &str, needles: &[&str]) {
    let (rc, out) = check(cmd, false);
    assert_eq!(rc, 1, "{name}: expected deny, got {out}");
    assert!(
        out.starts_with("deny: PR 本文に次の不備があります:"),
        "{name}: {out}"
    );
    for n in needles {
        assert!(out.contains(n), "{name}: [{n}] not in {out}");
    }
}

fn pr(body: &str) -> String {
    format!("gh pr create --body '{body}'")
}

const GOOD: &str = "Closes #1

## 検証
- [x] cargo test

## 要確認
- #42 — 実機での uart 受信確認";

const UNCHECKED: &str = "Closes #1

## 検証
- [x] cargo test
- [ ] 実機で確認";

#[test]
fn st01_no_checkbox_no_confirm() {
    assert_pass("1", "gh pr create --body 'Closes #1'");
}

#[test]
fn st02_to_07_conforming_bodies_pass() {
    assert_pass("2 全チェック済み+要確認に Issue 参照", &pr(GOOD));
    assert_pass(
        "3 大文字 [X]",
        &pr("Closes #1\n\n## 検証\n- [X] cargo test"),
    );
    assert_pass(
        "4 issues URL 参照",
        &pr("Closes #1\n\n## 要確認\n- https://github.com/tarotene/dotfiles/issues/42 — 実機での uart 受信確認"),
    );
    assert_pass("5 要確認見出しのみ", &pr("Closes #1\n\n## 要確認"));
    assert_pass(
        "6 fence 内の [ ]",
        &pr("Closes #1\n\n## 検証\n```\n- [ ] not a real checkbox (fenced example)\n```"),
    );
    assert_pass(
        "7 インラインコードスパン内の [ ]",
        &pr("Closes #1\n\n## 検証\n記法の例: `- [ ] foo` の形で書く。"),
    );
}

#[test]
fn st08_to_11_unchecked_task_list_denied() {
    assert_deny("8", &pr(UNCHECKED), &["task list"]);
    assert_deny(
        "9 * マーカー",
        &pr("Closes #1\n\n## 検証\n* [ ] 実機で確認"),
        &["task list"],
    );
    assert_deny(
        "10 インデント付き",
        &pr("Closes #1\n\n## 検証\n  - [ ] インデント付きの未チェック"),
        &["task list"],
    );
    assert_deny(
        "11 要確認直下",
        &pr("Closes #1\n\n## 要確認\n- [ ] #42 — 実機での uart 受信確認"),
        &["task list"],
    );
}

#[test]
fn st12_to_14_confirm_without_issue_ref_denied() {
    assert_deny(
        "12",
        &pr("Closes #1\n\n## 要確認\n- 実機での uart 受信確認(担当者待ち)"),
        &["Issue 参照"],
    );
    assert_deny(
        "13 2 項目中 2 番目だけ参照無し",
        &pr("Closes #1\n\n## 要確認\n- #10 — 資格情報の発行\n- 実機での uart 受信確認(担当者待ち)"),
        &["項目2"],
    );
    assert_deny(
        "14 両違反は 1 つの deny に合流",
        &pr("Closes #1\n\n## 検証\n- [ ] cargo test\n\n## 要確認\n- 実機での uart 受信確認(担当者待ち)"),
        &["task list", "Issue 参照"],
    );
}

#[test]
fn st15_escape_hatch() {
    let (rc, out) = check(&pr(UNCHECKED), true);
    assert_eq!((rc, out.as_str()), (0, "pass\n"));
    // 厳密に "1" のときだけ(bash の `!= 1`)
    let o = Command::new(BIN)
        .args(["--check", &pr(UNCHECKED)])
        .env("PR_CONFIRM_GUARD_ALLOW", "true")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn st16_17_fires_in_every_repo() {
    // owner スコープを持たない: --check は project を見ないので remote に依らない
    assert_deny("16", &pr(UNCHECKED), &["task list"]);
    assert_deny(
        "17 -R 指定でも",
        &format!("gh pr create -R other/repo --body '{UNCHECKED}'"),
        &["task list"],
    );
}

#[test]
fn st18_body_file() {
    let d = tempfile::tempdir().unwrap();
    let bf = d.path().join("body.md");
    std::fs::write(&bf, UNCHECKED).unwrap();
    assert_deny(
        "18",
        &format!("gh pr create --body-file '{}'", bf.display()),
        &["task list"],
    );
}

#[test]
fn st19_not_command_position() {
    assert_pass("19", "echo 'gh pr create --body x'");
}

#[test]
fn edit_and_heredoc_are_judged() {
    assert_deny(
        "edit",
        &format!("gh pr edit 3 --body '{UNCHECKED}'"),
        &["task list"],
    );
    let cmd = format!("gh pr create --title t --body \"$(cat <<'EOF'\n{UNCHECKED}\nEOF\n)\"");
    assert_deny("heredoc", &cmd, &["task list"]);
    // 本文がコマンド置換のみ・本文フラグ無しは判定不能で通す
    assert_pass("cmd subst", "gh pr create --body \"$(cat body.md)\"");
    assert_pass("no body", "gh pr edit 3 --add-label x");
    assert_pass("unmatched quote", "gh pr create --body 'x");
}

// ---- hook 入出力(bash の main / Codex adapter の main_codex) ----

fn run_hook(args: &[&str], stdin: &str, envs: &[(&str, &str)]) -> Output {
    let mut c = Command::new(BIN);
    c.args(args)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("PR_CONFIRM_GUARD_ALLOW")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    ch.wait_with_output().unwrap()
}

fn git_repo(p: &Path) {
    std::fs::create_dir_all(p).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(p)
        .status()
        .unwrap()
        .success());
}

fn input(cwd: &str, cmd: &str) -> String {
    serde_json::json!({"tool_name": "Bash", "cwd": cwd, "tool_input": {"command": cmd}}).to_string()
}

fn decision(o: &Output) -> Option<String> {
    let s = String::from_utf8_lossy(&o.stdout);
    if s.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    Some(
        v["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

#[test]
fn codex_adapter_cases() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    git_repo(&repo);
    let cwd = repo.to_str().unwrap();
    // codex adapter selftest 1: 未チェック → deny
    let o = run_hook(&["--agent", "codex"], &input(cwd, &pr(UNCHECKED)), &[]);
    assert_eq!(decision(&o).as_deref(), Some("deny"));
    // 2: 適合本文 → 出力なし
    let ok = pr("Closes #1\n\n## 要確認\n- #42 — 実機での確認");
    let o = run_hook(&["--agent", "codex"], &input(cwd, &ok), &[]);
    assert!(o.stdout.is_empty());
    // 3: gh pr 以外
    let o = run_hook(&["--agent", "codex"], &input(cwd, "git status"), &[]);
    assert!(o.stdout.is_empty());
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn claude_hook_output_shape_and_project_resolution() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    git_repo(&repo);
    let plain = d.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let cmd = pr(UNCHECKED);

    // 出力は jq の整形どおり(hookSpecificOutput、2 空白インデント、末尾改行)
    let o = run_hook(&[], &input(repo.to_str().unwrap(), &cmd), &[]);
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.starts_with("{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"PR 本文に次の不備があります:"));
    assert!(s.ends_with("\"\n  }\n}\n"));

    // git 作業ツリー外は黙って通す
    assert!(run_hook(&[], &input(plain.to_str().unwrap(), &cmd), &[])
        .stdout
        .is_empty());
    // cwd 無し + CLAUDE_PROJECT_DIR 無しは通す
    let no_cwd = serde_json::json!({"tool_name":"Bash","tool_input":{"command":cmd}}).to_string();
    assert!(run_hook(&[], &no_cwd, &[]).stdout.is_empty());
    // Claude は CLAUDE_PROJECT_DIR を優先する(cwd が作業ツリー外でも効く)
    let o = run_hook(
        &[],
        &input(plain.to_str().unwrap(), &cmd),
        &[("CLAUDE_PROJECT_DIR", repo.to_str().unwrap())],
    );
    assert_eq!(decision(&o).as_deref(), Some("deny"));
    // Codex は CLAUDE_PROJECT_DIR を見ない(bash の main_codex は .cwd だけ)
    let o = run_hook(
        &["--agent", "codex"],
        &input(plain.to_str().unwrap(), &cmd),
        &[("CLAUDE_PROJECT_DIR", repo.to_str().unwrap())],
    );
    assert!(o.stdout.is_empty());
    // 環境変数の escape hatch
    let o = run_hook(
        &[],
        &input(repo.to_str().unwrap(), &cmd),
        &[("PR_CONFIRM_GUARD_ALLOW", "1")],
    );
    assert!(o.stdout.is_empty());
}

#[test]
fn degraded_inputs_pass_silently() {
    for stdin in [
        "",
        "{x",
        "\"str\"",
        "{}",
        r#"{"tool_name":"Read","cwd":"/"}"#,
    ] {
        let o = run_hook(&[], stdin, &[]);
        assert_eq!(o.status.code(), Some(0), "{stdin}");
        assert!(o.stdout.is_empty(), "{stdin}");
    }
    // Bash 以外
    let d = tempfile::tempdir().unwrap();
    git_repo(d.path());
    let other = serde_json::json!({"tool_name":"Edit","cwd":d.path(),"tool_input":{"command":pr(UNCHECKED)}}).to_string();
    assert!(run_hook(&[], &other, &[]).stdout.is_empty());
}
