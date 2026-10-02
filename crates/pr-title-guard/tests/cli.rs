//! 実バイナリに対する characterization テスト。
//!
//! bash 版の --selftest 全ケースの移し:
//! - `st1`〜`st10`: config/claude/hooks/pr-title-guard.sh の 10 ケース
//!   (`--check` 経路)。**st9(checker 実行不能 → pass)は移せない**:
//!   bash 版は `scripts/pr-title-check` を子プロセスで解決して呼んでいたが、
//!   Rust では `pr-title-check` クレートの関数を直接呼ぶので「checker 不在」が
//!   表現不可能になった(PR_TITLE_CHECK_BIN も廃止)。代わりに同じ「判定不能は
//!   通す」縮退の `degrade_*` を置く。
//! - `codex_*` / `copilot_*`: 旧 Codex/Copilot adapter の --selftest(2 + 3 件)。
//! - `hook_*` / `extra_*`: main() の分岐と guard-core の継ぎ目の追加ケース。
//!
//! 状態(git remote)を持つので trycmd ではなく統合テストにしている
//! (wrapup-stop-gate / plan-fresh-gate と同じ型)。

use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_pr-title-guard");
const BAD: &str = "PR タイトルを直す";
const REASON_HEAD: &str =
    "PR タイトル 'PR タイトルを直す' は commit-message 契約(ADR-0031)に非適合です。";

fn repo(remote: &str) -> TempDir {
    let d = TempDir::new().unwrap();
    let git = |args: &[&str]| {
        assert!(Command::new("git")
            .arg("-C")
            .arg(d.path())
            .args(args)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success());
    };
    git(&["init", "-q"]);
    git(&["remote", "add", "origin", remote]);
    d
}

fn tarotene() -> TempDir {
    repo("https://github.com/tarotene/dotfiles.git")
}
fn other() -> TempDir {
    repo("https://github.com/example/example.git")
}

/// `--check` を走らせて (exit, stdout)。
fn check(cmd: &str, project: &Path, allow: bool) -> (i32, String) {
    let mut c = Command::new(BIN);
    c.args(["--check", cmd])
        .arg(project)
        .env_remove("PR_TITLE_GUARD_ALLOW");
    if allow {
        c.env("PR_TITLE_GUARD_ALLOW", "1");
    }
    let o = c.output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8(o.stdout).unwrap(),
    )
}

fn assert_deny(cmd: &str, project: &Path) {
    let (rc, out) = check(cmd, project, false);
    assert_eq!(rc, 1, "{cmd}: {out}");
    assert!(out.starts_with("deny: PR タイトル '"), "{out}");
    assert!(out.contains("commit-message 契約"), "{out}");
}
fn assert_pass(cmd: &str, project: &Path) {
    let (rc, out) = check(cmd, project, false);
    assert_eq!((rc, out.as_str()), (0, "pass\n"), "{cmd}");
}

/// hook として stdin JSON を渡す。
fn hook(args: &[&str], stdin: &str, envs: &[(&str, &str)]) -> String {
    let mut c = Command::new(BIN);
    c.args(args)
        .env_remove("PR_TITLE_GUARD_ALLOW")
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    let mut child = c.spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).ok();
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    String::from_utf8(o.stdout).unwrap()
}

fn json_str(s: &str) -> String {
    format!("{s:?}")
}

// --- tarotene リポジトリ ---

#[test]
fn st1_non_conforming_create_denies() {
    let r = tarotene();
    let (rc, out) = check(
        &format!("gh pr create --title '{BAD}' --body b"),
        r.path(),
        false,
    );
    assert_eq!(rc, 1);
    assert!(out.starts_with(&format!(
        "deny: {REASON_HEAD}'type(scope)?!?: subject' 形式"
    )));
    assert!(out.ends_with("PR_TITLE_GUARD_ALLOW=1 を設定してください。\n"));
}

#[test]
fn st2_conforming_create_passes() {
    assert_pass(
        "gh pr create --title 'feat: 適合するタイトル' --body b",
        tarotene().path(),
    );
}

#[test]
fn st3_no_title_passes() {
    assert_pass("gh pr create --web", tarotene().path());
}

#[test]
fn st4_edit_non_conforming_denies() {
    assert_deny(&format!("gh pr edit 1 --title '{BAD}'"), tarotene().path());
}

#[test]
fn st5_edit_without_title_passes() {
    assert_pass("gh pr edit 1 --add-label bug", tarotene().path());
}

// --- owner スコープ ---

#[test]
fn st6_other_owner_passes() {
    assert_pass(
        &format!("gh pr create --title '{BAD}' --body b"),
        other().path(),
    );
}

#[test]
fn st7_repo_flag_overrides_project_owner() {
    assert_deny(
        &format!("gh pr create -R tarotene/dotfiles --title '{BAD}'"),
        other().path(),
    );
}

// --- escape hatch / 縮退 ---

#[test]
fn st8_escape_hatch_passes() {
    let (rc, out) = check(
        &format!("gh pr create --title '{BAD}' --body b"),
        tarotene().path(),
        true,
    );
    assert_eq!((rc, out.as_str()), (0, "pass\n"));
}

#[test]
fn st10_non_command_position_passes() {
    assert_pass("echo 'gh pr create --title x'", tarotene().path());
}

#[test]
fn degrade_unresolvable_owner_passes() {
    // remote が無く owner を解決できない → fail-open。
    let d = TempDir::new().unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(d.path())
        .args(["init", "-q"])
        .status()
        .unwrap()
        .success());
    assert_pass(&format!("gh pr create --title '{BAD}'"), d.path());
}

// --- guard-core の継ぎ目(bash の parse_pr_title_tokens と同じ 1 パス) ---

#[test]
fn extra_short_and_equals_forms() {
    let r = other();
    assert_deny(&format!("gh pr create -R tarotene/x -t '{BAD}'"), r.path());
    assert_deny(
        &format!("gh pr create --repo=tarotene/x --title='{BAD}'"),
        r.path(),
    );
    assert_deny(
        &format!("gh pr create --title '{BAD}' --repo tarotene/x"),
        r.path(),
    );
}

#[test]
fn extra_title_value_is_not_read_as_flag() {
    // `--title -R` の `-R` は値であって --repo ではない(1 パス走査)。
    // 値 `-R` は非適合タイトルとして deny になる。
    let r = tarotene();
    let (rc, out) = check("gh pr create --title -R x/y", r.path(), false);
    assert_eq!(rc, 1);
    assert!(out.contains("PR タイトル '-R' は"));
}

#[test]
fn extra_second_range_denies() {
    let r = tarotene();
    assert_deny(
        &format!("gh pr create --title 'feat: ok' && gh pr edit 1 --title '{BAD}'"),
        r.path(),
    );
}

#[test]
fn extra_empty_title_passes() {
    assert_pass("gh pr create --title '' --body b", tarotene().path());
}

#[test]
fn extra_empty_repo_falls_back_to_project() {
    assert_deny(
        &format!("gh pr create --repo '' --title '{BAD}'"),
        tarotene().path(),
    );
    assert_pass(
        &format!("gh pr create --repo '' --title '{BAD}'"),
        other().path(),
    );
}

// --- hook 経路(Claude) ---

fn claude_in(cwd: &Path, cmd: &str) -> String {
    format!(
        r#"{{"tool_name":"Bash","cwd":{},"tool_input":{{"command":{}}}}}"#,
        json_str(cwd.to_str().unwrap()),
        json_str(cmd)
    )
}

#[test]
fn hook_claude_deny_bytes() {
    let r = tarotene();
    let out = hook(
        &[],
        &claude_in(r.path(), &format!("gh pr create --title '{BAD}'")),
        &[],
    );
    let want = format!(
        "{{\n  \"hookSpecificOutput\": {{\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": \"{REASON_HEAD}'type(scope)?!?: subject' 形式(type は feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)にしてください。一時的に無効化するには PR_TITLE_GUARD_ALLOW=1 を設定してください。\"\n  }}\n}}\n"
    );
    assert_eq!(out, want);
}

#[test]
fn hook_claude_project_dir_env_wins_over_cwd() {
    let t = tarotene();
    let o = other();
    // cwd は他 owner だが CLAUDE_PROJECT_DIR が tarotene → deny。
    let out = hook(
        &[],
        &claude_in(o.path(), &format!("gh pr create --title '{BAD}'")),
        &[("CLAUDE_PROJECT_DIR", t.path().to_str().unwrap())],
    );
    assert!(out.contains("deny"));
}

#[test]
fn hook_pass_cases_print_nothing() {
    let r = tarotene();
    let ok = |s: &str| assert_eq!(hook(&[], s, &[]), "", "{s}");
    ok(&claude_in(r.path(), "git status"));
    ok("not json");
    ok("");
    ok(r#"{"tool_name":"Read","cwd":"/","tool_input":{}}"#);
    ok(r#"{"tool_name":"Bash","tool_input":{"command":"gh pr create --title x"}}"#);
    // git 作業ツリーでない cwd
    let plain = TempDir::new().unwrap();
    ok(&claude_in(
        plain.path(),
        &format!("gh pr create --title '{BAD}'"),
    ));
    // MCP は対象外
    ok(r#"{"tool_name":"mcp__x__y","cwd":"/","tool_input":{"command":"gh pr create --title x"}}"#);
}

// --- Codex(旧 adapter の selftest 2 件) ---

#[test]
fn codex_deny_and_pass() {
    let r = tarotene();
    let out = hook(
        &["--agent", "codex"],
        &claude_in(r.path(), &format!("gh pr create --title '{BAD}' --body b")),
        &[],
    );
    assert!(out.contains("\"permissionDecision\": \"deny\""));
    assert!(out.contains("hookSpecificOutput"));
    assert_eq!(
        hook(
            &["--agent", "codex"],
            &claude_in(r.path(), "git status"),
            &[]
        ),
        ""
    );
}

#[test]
fn codex_ignores_claude_project_dir() {
    // adapter は .cwd だけを見る(env を見ない)。
    let t = tarotene();
    let o = other();
    let out = hook(
        &["--agent", "codex"],
        &claude_in(o.path(), &format!("gh pr create --title '{BAD}'")),
        &[("CLAUDE_PROJECT_DIR", t.path().to_str().unwrap())],
    );
    assert_eq!(out, "");
}

// --- Copilot(旧 adapter の selftest 3 件) ---

fn copilot_in(cwd: &Path, tool: &str, args: &str) -> String {
    format!(
        r#"{{"sessionId":"s","cwd":{},"toolName":{},"toolArgs":{args}}}"#,
        json_str(cwd.to_str().unwrap()),
        json_str(tool)
    )
}

#[test]
fn copilot_deny_unwrapped() {
    let r = tarotene();
    let args = format!(
        r#"{{"command":{}}}"#,
        json_str(&format!("gh pr create --title '{BAD}' --body b"))
    );
    let out = hook(
        &["--agent", "copilot"],
        &copilot_in(r.path(), "bash", &args),
        &[],
    );
    assert!(out
        .starts_with("{\n  \"permissionDecision\": \"deny\",\n  \"permissionDecisionReason\": \""));
    assert!(!out.contains("hookSpecificOutput"));
}

#[test]
fn copilot_pass_git_status() {
    let r = tarotene();
    assert_eq!(
        hook(
            &["--agent", "copilot"],
            &copilot_in(r.path(), "bash", r#"{"command":"git status"}"#),
            &[]
        ),
        ""
    );
}

#[test]
fn copilot_pass_other_tool() {
    let r = tarotene();
    assert_eq!(
        hook(
            &["--agent", "copilot"],
            &copilot_in(r.path(), "str_replace_editor", r#"{"new_str":"x"}"#),
            &[]
        ),
        ""
    );
}
