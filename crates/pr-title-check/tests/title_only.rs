//! 契約は PR タイトルだけを見る。git の履歴を見ない(ADR-0031 の Amendment 2026-10-21)。
//!
//! D7 は、既存の非適合 commit(23 件、全 20 リポの直近 100 commit の実測)を、履歴書き換え
//! と再署名でバックフィルする方針だった。書き換えは Security ruleset を外して force push し、
//! YubiKey のタッチが 150〜180 回、merge commit 9 件の書き換えは履歴の形そのものを変える。
//! そこで、#337 の完了日(2026-10-01)より前の commit は、書き換えず、契約の対象外(許容)
//! とする第二の縮小を採った。この縮小が成り立つのは、サーバ側の検査(この crate)が、
//! 判定の入力を PR タイトルに限り、履歴に既に非適合 commit があっても、タイトルが適合
//! していれば通すから。その性質を固定する(履歴が判定に入り込むと、許容した過去の commit
//! が、無関係な PR を落とし続ける)。

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pr-title-check-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.hooksPath=",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn check(cwd: &Path, title: &str) -> i32 {
    Command::new(env!("CARGO_BIN_EXE_pr-title-check"))
        .current_dir(cwd)
        .arg(title)
        .output()
        .unwrap()
        .status
        .code()
        .unwrap()
}

#[test]
fn verdict_depends_only_on_the_title_not_on_history() {
    // 履歴に、非適合な subject の commit(merge commit と root commit を含む形の代表として
    // 通常の 1 件)が既にあるリポジトリ。
    let repo = scratch("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "wip stuff"]);
    git(
        &repo,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Merge branch 'x' into main",
        ],
    );

    // 適合したタイトルは、履歴が非適合でも通る(許容した過去の commit が PR を落とさない)。
    assert_eq!(check(&repo, "feat(scope): 適合したタイトル"), 0);
    // 非適合なタイトルは、履歴が何であれ落ちる(検査自体は緩めていない)。
    assert_eq!(check(&repo, "wip stuff"), 1);

    // git の外でも、同じ入力に同じ判定(履歴の有無に依存しない)。
    let outside = scratch("outside");
    assert_eq!(check(&outside, "feat(scope): 適合したタイトル"), 0);
    assert_eq!(check(&outside, "wip stuff"), 1);

    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&outside);
}
