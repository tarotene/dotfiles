//! `config/claude/hooks/decision-colocation-guard.sh --selftest` の全 11 ケース
//! (12 アサーション。bash 版の番号どおり)+ 実 checker での e2e。
//!
//! bash 版は checker を固定応答のスタブ実行ファイルに差し替えて(ケース 1〜11)
//! 判定を検査していた。Rust 版は checker を同じクレートの関数として直接呼ぶ
//! ので、スタブは `Guard::checker` に渡すクロージャになる(base 名で応答を
//! 変える点は同じ)。ケース 7(checker 実行ファイルが無い)は「どの checker を
//! 呼ぶか」の解決自体が無くなったため対応物が無く、判定不能 = 通す(ケース 5)
//! が同じ縮退を覆う。

mod common;

use common::{git, Repo};
use decision_colocation::guard::{CheckOutcome, Guard};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn stub(_: &Path, base: &str) -> CheckOutcome {
    match base {
        "ok-base" => CheckOutcome::Conforming,
        "unresolvable" => CheckOutcome::Indeterminate,
        _ => CheckOutcome::NonConforming("decision-colocation-check: 非適合(スタブ)".into()),
    }
}

/// bash selftest の「実験環境」: main を持つ git repo(github remote 付き、
/// origin/HEAD が main)。ng-base / ok-base のブランチもある。
fn env_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    common::commit_all(p, "base");
    git(
        p,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tarotene/dotfiles.git",
        ],
    );
    let head = git(p, &["rev-parse", "HEAD"]);
    git(p, &["update-ref", "refs/remotes/origin/main", &head]);
    git(
        p,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(p, &["switch", "-c", "ng-base", "-q"]);
    git(p, &["switch", "-c", "ok-base", "-q"]);
    git(p, &["switch", "main", "-q"]);
    dir
}

fn decide(skip: bool, cmd: &str, project: &Path) -> Option<String> {
    Guard {
        skip,
        checker: &stub,
    }
    .decide(cmd, project)
}

const CHECK_OUT: &str = "decision-colocation-check: 非適合(スタブ)";

#[test]
fn cases_1_to_11() {
    let dir = env_repo();
    let repo = dir.path();

    // 1: 非適合(base=main、checker が非0を返す) -> deny、checker 出力を含む
    let out = decide(false, "gh pr create --base main --title t --body b", repo).expect("1 deny");
    assert!(out.contains("決定成果物"), "1: {out}");
    assert!(out.contains(CHECK_OUT), "1 checker出力: {out}");

    // 2: 適合(base=ok-base) -> pass
    assert_eq!(
        decide(
            false,
            "gh pr create --base ok-base --title t --body b",
            repo
        ),
        None,
        "2"
    );

    // 3: --base 省略、default branch(origin/HEAD symref)= main を解決 -> deny
    let out = decide(false, "gh pr create --title t --body b", repo).expect("3 deny");
    assert!(out.contains("決定成果物"), "3: {out}");

    // 4: --base にローカルに実在しないブランチ -> origin/<branch> にも無ければ判定不能で pass
    assert_eq!(
        decide(
            false,
            "gh pr create --base no-such-branch --title t --body b",
            repo
        ),
        None,
        "4"
    );

    // 5: checker が判定不能(rc=2 相当)を返す -> pass
    assert_eq!(
        decide(
            false,
            "gh pr create --base unresolvable --title t --body b",
            repo
        ),
        None,
        "5"
    );

    // 6: SKIP_DECISION_COLOCATION_GUARD=1 -> pass
    assert_eq!(
        decide(true, "gh pr create --base main --title t --body b", repo),
        None,
        "6"
    );

    // 7: (checker 実行ファイルの不在 — 対応物なし。上の docs 参照)

    // 8: 非コマンド位置(echo の引数内)は発火しない
    assert_eq!(
        decide(false, "echo 'gh pr create --base main'", repo),
        None,
        "8"
    );

    // 9: -R 指定と --base 指定が共存しても解析が壊れない
    assert_eq!(
        decide(
            false,
            "gh pr create -R tarotene/dotfiles --base ok-base --title t --body b",
            repo
        ),
        None,
        "9"
    );

    // 10: gh pr create の前に別の文がある複合コマンド -> deny 文に前段消失の警告(#668)
    let out = decide(
        false,
        "printf x > /tmp/body.md && gh pr create --base main --title t --body-file /tmp/body.md",
        repo,
    )
    .expect("10 deny");
    assert!(out.contains("別の呼び出しに分けて"), "10: {out}");

    // 11: gh pr create 単独の deny には警告を付けない(#668)
    let out = decide(false, "gh pr create --base main --title t --body b", repo).expect("11 deny");
    assert!(!out.contains("別の呼び出しに分けて"), "11: {out}");
}

/// bash 版の deny 文と同じ全文(checker 出力を挟む)。
#[test]
fn deny_text_is_byte_identical_to_bash() {
    let dir = env_repo();
    let out = decide(false, "gh pr create --base main", dir.path()).unwrap();
    assert_eq!(
        out,
        format!(
            "決定成果物(ADR/設計文書/skill)の新規追加、または既存 ADR への Amendment 追加が、その執行点(実際に実装する変更)を同じ PR に伴っていません(config/agents/AGENTS.md「決定成果物は執行点と同じ PR に出す」、docs/adr/396-decision-colocation.md)。詳細:\n{CHECK_OUT}\n一時的に無効化するには SKIP_DECISION_COLOCATION_GUARD=1 を設定してください。"
        )
    );
    let out = decide(false, "true; gh pr create --base main", dir.path()).unwrap();
    assert!(out.ends_with(
        "\n注意: この Bash 呼び出しは全体が実行されていません。gh pr create より前の文(PR 本文ファイルの生成など)も実行されていないので、指摘を直したうえで、前段の文と gh pr create を別の呼び出しに分けて再実行してください。"
    ));
}

#[test]
fn flag_spellings_and_edge_cases() {
    let dir = env_repo();
    let repo = dir.path();
    // -B / --base= の綴り
    assert_eq!(decide(false, "gh pr create -B ok-base", repo), None);
    assert_eq!(decide(false, "gh pr create --base=ok-base", repo), None);
    assert!(decide(false, "gh pr create --base=main", repo).is_some());
    // 末尾の値なし --base は present のまま(空 base -> 判定不能で通す)
    assert_eq!(decide(false, "gh pr create --title t --base", repo), None);
    // フルパスの gh でもコマンド位置なら対象
    assert!(decide(false, "/usr/bin/gh pr create --base main", repo).is_some());
    // 範囲ごとに判定する: 2 つ目が deny
    let out = decide(
        false,
        "gh pr create --base ok-base && gh pr create --base main",
        repo,
    )
    .unwrap();
    assert!(out.contains(CHECK_OUT));
    // origin/<branch> にフォールバックする(ローカルに無く origin だけにある)
    let head = git(repo, &["rev-parse", "HEAD"]);
    git(
        repo,
        &["update-ref", "refs/remotes/origin/only-remote", &head],
    );
    assert!(decide(false, "gh pr create --base only-remote", repo).is_some());
    // repo でないディレクトリ・remote 無しで --base 省略 -> 通す
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "-q"]);
    assert_eq!(decide(false, "gh pr create --title t", bare.path()), None);
}

// ---- 実 checker(同じクレートの run_check)での e2e ------------------------

fn guard_bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_decision-colocation-guard"));
    c.env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("SKIP_DECISION_COLOCATION_GUARD");
    c
}

fn run_hook(cmd: &mut Command, stdin: &str) -> (i32, String) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

/// 新規 ADR(執行点節なし)を 1 コミット積んだ repo。base は main。
fn violating_repo() -> tempfile::TempDir {
    let r = Repo::new();
    let p = r.path();
    git(&p, &["update-ref", "refs/remotes/origin/main", &r.base_sha]);
    git(
        &p,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(
        &p,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
    git(&p, &["switch", "-c", "feature", "-q"]);
    r.write("docs/adr/0101-no-section.md", "# ADR\n\n## Context\n");
    r.commit("adr without 執行点");
    r.dir
}

#[test]
fn hook_denies_with_real_checker() {
    let dir = violating_repo();
    let input = format!(
        r#"{{"tool_name":"Bash","cwd":{:?},"tool_input":{{"command":"gh pr create --title t --body b"}}}}"#,
        dir.path()
    );
    let (code, out) = run_hook(&mut guard_bin(), &input);
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\": \"deny\""), "{out}");
    assert!(
        out.contains("docs/adr/0101-no-section.md は新規 ADR です"),
        "{out}"
    );

    // escape hatch
    let (code, out) = run_hook(
        guard_bin().env("SKIP_DECISION_COLOCATION_GUARD", "1"),
        &input,
    );
    assert_eq!((code, out.as_str()), (0, ""));

    // CLAUDE_PROJECT_DIR が cwd に優先する(cwd は git 外)
    let outside = tempfile::tempdir().unwrap();
    let input2 = format!(
        r#"{{"tool_name":"Bash","cwd":{:?},"tool_input":{{"command":"gh pr create --title t --body b"}}}}"#,
        outside.path()
    );
    let (_, out) = run_hook(&mut guard_bin(), &input2);
    assert_eq!(out, "", "git 外の cwd は通す");
    let (_, out) = run_hook(guard_bin().env("CLAUDE_PROJECT_DIR", dir.path()), &input2);
    assert!(out.contains("deny"), "{out}");
}

#[test]
fn hook_degrades_to_pass() {
    let dir = violating_repo();
    let cwd = format!("{:?}", dir.path());
    for input in [
        String::new(),
        "not json".to_string(),
        r#"{"tool_name":"Write"}"#.to_string(),
        format!(r#"{{"tool_name":"Bash","cwd":{cwd}}}"#),
        format!(r#"{{"tool_name":"Bash","cwd":{cwd},"tool_input":{{"command":"ls"}}}}"#),
        r#"{"tool_name":"Bash","tool_input":{"command":"gh pr create"}}"#.to_string(),
        format!(r#"{{"tool_name":"mcp__x__create_pull_request","cwd":{cwd}}}"#),
    ] {
        let (code, out) = run_hook(&mut guard_bin(), &input);
        assert_eq!((code, out.as_str()), (0, ""), "{input}");
    }
}

#[test]
fn manual_check_mode() {
    let dir = violating_repo();
    let p = dir.path().to_str().unwrap();
    let o = guard_bin()
        .args(["--check", "gh pr create --title t --body b", p])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("deny: 決定成果物"));
    let o = guard_bin()
        .args(["--check", "echo hi", p])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "pass\n");
}
