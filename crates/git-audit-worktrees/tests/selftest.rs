//! bash 版 `selftest()` の移植(prunable / orphaned / notify)。
//!
//! bash の 1 本の逐次シナリオを、独立に走る個別テストへ分けてある。対応:
//!
//! | bash selftest のケース                                  | テスト                                   |
//! |---------------------------------------------------------|------------------------------------------|
//! | stale 検出が非 0 終了 + class=prunable の行              | stale_prunable_is_reported               |
//! | orphaned(no-upstream, unique=0)を検出                   | orphaned_no_upstream_zero_unique_detected |
//! | dirty は orphaned にしない                               | dirty_worktree_never_orphaned            |
//! | unique commit を持つものは orphaned にしない             | worktree_with_unique_commit_never_orphaned |
//! | stash list 失敗時 fail-closed(shelved 扱い)             | stash_list_failure_fails_closed          |
//! | shallow リポジトリでは unique 比較をスキップ             | shallow_repo_skips_unique_comparison     |
//! | herdr 到達不能で orphaned 0 件(fail-closed)             | herdr_unreachable_fails_closed           |
//! | orphaned([gone]) を検出                                  | orphaned_gone_upstream_detected          |
//! | shelve 済みは orphaned にしない                          | shelved_gone_worktree_exempt             |
//! | notify の de-dup(呼び出し回数 = 2)                      | notify_dedups_by_fingerprint             |
//! | reason=disabled は exit 非 0 + メッセージ(#89)           | notify_disabled_fails_the_unit           |
//! | prune 後に branch が残り、再実行が exit 0                | after_prune_branch_remains_and_audit_clean |

mod common;
use common::*;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// `ghr/.../repo` と、checkout を消して prunable にした `test/stale`。
fn fixture_with_stale() -> (Fx, PathBuf) {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = fx.root.join("disappeared");
    git(
        &fx.repo,
        &["worktree", "add", "-qb", "test/stale", wt.to_str().unwrap()],
    );
    fs::remove_dir_all(&wt).unwrap();
    (fx, wt)
}

fn add_wt(fx: &Fx, name: &str, branch: &str) -> PathBuf {
    let wt = fx.root.join(name);
    git(
        &fx.repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", branch],
    );
    wt
}

fn mentions(out: &str, p: &Path) -> bool {
    out.contains(p.to_str().unwrap())
}

fn orphaned_row(out: &str, p: &Path) -> bool {
    out.lines()
        .any(|l| l.contains(p.to_str().unwrap()) && l.contains("\torphaned\t"))
}

#[test]
fn stale_prunable_is_reported() {
    let (fx, wt) = fixture_with_stale();
    let o = fx.run(&[]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.is_empty());
    let line = o
        .stdout
        .lines()
        .find(|l| l.contains(&format!("path={} branch=test/stale", wt.display())))
        .unwrap_or_else(|| panic!("no stale line: {}", o.stdout));
    assert!(line.contains("class=prunable"), "{line}");
    assert!(
        line.starts_with(&format!(
            "stale worktree: repo={} path={} branch=test/stale class=prunable reason=",
            fx.repo.display(),
            wt.display()
        )),
        "{line}"
    );
    assert!(
        o.stdout.ends_with("total: 1 stale worktree finding(s)\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn orphaned_no_upstream_zero_unique_detected() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-empty", "worktree/orphan-empty");
    let o = fx.run(&["--porcelain"]);
    assert!(orphaned_row(&o.stdout, &wt), "{}", o.stdout);
    let row = o
        .stdout
        .lines()
        .find(|l| l.contains("orphan-empty"))
        .unwrap();
    assert_eq!(
        row,
        format!(
            "{}/.git\t{}\t{}\tworktree/orphan-empty\torphaned\tupstream 未設定・main に対して unique commit 0",
            fx.repo.display(),
            fx.repo.display(),
            wt.display()
        )
    );
}

#[test]
fn dirty_worktree_never_orphaned() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-empty", "worktree/orphan-empty");
    fs::write(wt.join("dirty.txt"), "").unwrap();
    let o = fx.run(&["--porcelain"]);
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
    // 元に戻せば検出される(上の否定が guard の効果であることの対照)。
    fs::remove_file(wt.join("dirty.txt")).unwrap();
    assert!(orphaned_row(&fx.run(&["--porcelain"]).stdout, &wt));
}

#[test]
fn worktree_with_unique_commit_never_orphaned() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-empty", "worktree/orphan-empty");
    git(&wt, &["commit", "--allow-empty", "-qm", "real work"]);
    let o = fx.run(&["--porcelain"]);
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
}

#[test]
fn stash_list_failure_fails_closed() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-stashfail", "worktree/orphan-stashfail");
    // stash list だけ失敗させる薄いラッパーを PATH 先頭に置く。
    let flag = fx.root.join("fail-stash");
    write_exec(
        &fx.bin.join("git"),
        &format!(
            "#!/usr/bin/env bash\nif [[ \" $* \" == *\" stash list \"* && -e \"{}\" ]]; then\n  exit 1\nfi\nexec \"{}\" \"$@\"\n",
            flag.display(),
            real_git()
        ),
    );
    let path = format!("{}:{}", fx.bin.display(), std::env::var("PATH").unwrap());
    // 対照: 失敗させない間は検出される(ラッパー自体は透過)。
    let o = finish(fx.command().env("PATH", &path).arg("--porcelain"));
    assert!(orphaned_row(&o.stdout, &wt), "{}", o.stdout);
    fs::write(&flag, "").unwrap();
    let o = finish(fx.command().env("PATH", &path).arg("--porcelain"));
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
}

#[test]
fn shallow_repo_skips_unique_comparison() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let shallow = fx.add_shallow_repo();
    let wt = fx.root.join("orphan-shallow");
    git(
        &shallow,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "worktree/orphan-shallow",
        ],
    );
    // 対照: 通常 repo の同じ形は同じ run で検出される。
    let normal = add_wt(&fx, "orphan-normal", "worktree/orphan-normal");
    let o = fx.run(&["--porcelain"]);
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
    assert!(orphaned_row(&o.stdout, &normal), "{}", o.stdout);
}

#[test]
fn herdr_unreachable_fails_closed() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-herdrdown", "worktree/orphan-herdrdown");
    assert!(orphaned_row(&fx.run(&["--porcelain"]).stdout, &wt));
    fx.set_herdr(HERDR_DOWN_STUB);
    let o = fx.run(&["--porcelain"]);
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
}

/// `[gone]` fixture: 本物の push / リモート削除 / fetch --prune を通す。
fn gone_fixture(fx: &Fx) -> PathBuf {
    let wt = add_wt(fx, "orphan-gone", "worktree/orphan-gone");
    git(&wt, &["push", "-q", "-u", "origin", "worktree/orphan-gone"]);
    git(
        &fx.repo,
        &["push", "-q", "origin", "--delete", "worktree/orphan-gone"],
    );
    git(&wt, &["fetch", "-q", "--prune", "origin"]);
    wt
}

#[test]
fn orphaned_gone_upstream_detected() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = gone_fixture(&fx);
    let o = fx.run(&["--porcelain"]);
    assert!(orphaned_row(&o.stdout, &wt), "{}", o.stdout);
    assert!(o
        .stdout
        .contains("\torphaned\tupstream ブランチが削除済み([gone])\n"));
}

#[test]
fn shelved_gone_worktree_exempt() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = gone_fixture(&fx);
    fs::write(wt.join("wip.txt"), "").unwrap();
    git(
        &wt,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "stash",
            "push",
            "--include-untracked",
            "-m",
            &format!("shelve:{}: wip", wt.display()),
        ],
    );
    let o = fx.run(&["--porcelain"]);
    assert!(!mentions(&o.stdout, &wt), "{}", o.stdout);
    // 退避を消せば再び検出される(shelve が理由であることの対照)。
    git(&wt, &["-c", "core.hooksPath=/dev/null", "stash", "clear"]);
    assert!(orphaned_row(&fx.run(&["--porcelain"]).stdout, &wt));
}

#[test]
fn notify_dedups_by_fingerprint() {
    let (fx, _wt) = fixture_with_stale();
    fx.set_reason("busy");
    assert_eq!(fx.run(&["--notify"]).code, 0);
    fx.set_reason("shown");
    assert_eq!(fx.run(&["--notify"]).code, 0);
    assert_eq!(fx.run(&["--notify"]).code, 0);
    assert_eq!(fx.calls(), 2, "notify 呼び出し回数 (期待=2)");
}

#[test]
fn notify_disabled_fails_the_unit() {
    let (fx, _wt) = fixture_with_stale();
    let _ = fs::remove_file(fx.state_json());
    fx.set_reason("disabled");
    let o = fx.run(&["--notify"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("reason=disabled"), "{}", o.stderr);
    assert!(
        o.stderr
            .contains("1件の stale worktree が無音で未通知です。"),
        "{}",
        o.stderr
    );
}

#[test]
fn after_prune_branch_remains_and_audit_clean() {
    let (fx, _wt) = fixture_with_stale();
    git(
        &fx.repo,
        &["worktree", "prune", "--verbose", "--expire=now"],
    );
    assert!(git_ok(
        &fx.repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/test/stale"]
    ));
    let o = fx.run(&[]);
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "stale worktree はありません。\n");
}
