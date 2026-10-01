//! git-shelve / git-unshelve の結合テスト。旧 bash 版には `--selftest` が
//! 無かった(git-unshelve は引数を見ず通常実行になる。`GIT_UNSHELVE_TEST_PRE_DROP_HOOK`
//! シームだけが競合復元パスのために用意されていた)ので、ここで挙動を
//! 一から特性化する。bash の観測可能な挙動を 1 対 1 で押さえる:
//!
//!   shelve_tags_message_with_note_and_includes_untracked   タグ + メモ / --include-untracked
//!   shelve_without_note_uses_tag_only                      メモ無しはタグのみ
//!   unshelve_applies_and_drops_only_own_entry              自 worktree の entry のみ(他 worktree は残る)
//!   unshelve_without_shelve_exits_1                        「shelve はありません」
//!   unshelve_conflict_on_apply_keeps_entry                 apply 失敗 → entry 保持・exit 1
//!   unshelve_recovers_when_another_entry_is_dropped        PRE_DROP_HOOK で競合 → store で復元 → 再試行成功
//!   unshelve_gives_up_after_three_attempts                 毎回競合 → 上限 3 で exit 1、他 entry は無傷
//!   shelve_waits_for_the_lock                              claude-shelve.lock の flock 直列化
//!   outside_repo_fails_with_git_status                     repo 外: git の終了コード 128
//!   (純関数の単体テストは src/lib.rs)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

fn base_cmd(prog: &str, dir: &Path) -> Command {
    let mut c = Command::new(prog);
    c.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env_remove("GIT_UNSHELVE_TEST_PRE_DROP_HOOK")
        .stdin(Stdio::null());
    c
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = base_cmd("git", dir).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn shelve(dir: &Path, args: &[&str]) -> Output {
    base_cmd(env!("CARGO_BIN_EXE_git-shelve"), dir)
        .args(args)
        .output()
        .unwrap()
}

fn unshelve(dir: &Path, hook: Option<&str>) -> Output {
    let mut c = base_cmd(env!("CARGO_BIN_EXE_git-unshelve"), dir);
    if let Some(h) = hook {
        c.env("GIT_UNSHELVE_TEST_PRE_DROP_HOOK", h);
    }
    c.output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// 追跡ファイル a.txt を 1 つ持つ repo。
fn repo() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let r = t.path().join("repo");
    fs::create_dir(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    fs::write(r.join("a.txt"), "base\n").unwrap();
    git(&r, &["add", "a.txt"]);
    git(&r, &["commit", "-q", "-m", "base"]);
    (t, r)
}

fn toplevel(dir: &Path) -> String {
    git(dir, &["rev-parse", "--show-toplevel"])
}

fn stash_subjects(dir: &Path) -> Vec<String> {
    let s = git(dir, &["stash", "list", "--format=%gs"]);
    s.lines().map(str::to_string).collect()
}

#[test]
fn shelve_tags_message_with_note_and_includes_untracked() {
    let (_t, r) = repo();
    fs::write(r.join("a.txt"), "changed\n").unwrap();
    fs::write(r.join("new.txt"), "n\n").unwrap();
    let o = shelve(&r, &["rebase 前の退避"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let top = toplevel(&r);
    let subs = stash_subjects(&r);
    assert_eq!(subs.len(), 1);
    assert!(
        subs[0].ends_with(&format!("shelve:{top}: rebase 前の退避")),
        "{subs:?}"
    );
    assert!(!r.join("new.txt").exists(), "untracked も退避される");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "base\n");
}

#[test]
fn shelve_without_note_uses_tag_only() {
    let (_t, r) = repo();
    fs::write(r.join("a.txt"), "changed\n").unwrap();
    let o = shelve(&r, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let top = toplevel(&r);
    let subs = stash_subjects(&r);
    assert!(subs[0].ends_with(&format!("shelve:{top}:")), "{subs:?}");
    assert!(!subs[0].ends_with(": "), "{subs:?}");
}

#[test]
fn unshelve_applies_and_drops_only_own_entry() {
    let (t, r) = repo();
    let wt = t.path().join("wt");
    git(
        &r,
        &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
    );

    fs::write(wt.join("a.txt"), "from-wt\n").unwrap();
    assert_eq!(shelve(&wt, &["wt-memo"]).status.code(), Some(0));
    fs::write(r.join("a.txt"), "from-main\n").unwrap();
    assert_eq!(shelve(&r, &["main-memo"]).status.code(), Some(0));
    assert_eq!(stash_subjects(&r).len(), 2);

    // wt から unshelve: wt の entry だけが戻り、main の entry は残る。
    let o = unshelve(&wt, None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "from-wt\n");
    let so = out(&o);
    assert!(so.starts_with("==> apply "), "{so}");
    assert!(
        so.contains(&format!("shelve:{}: wt-memo)", toplevel(&wt))),
        "{so}"
    );
    assert!(so.contains("Dropped"), "{so}");
    assert!(so.trim_end().contains("==> drop 完了: "), "{so}");
    let subs = stash_subjects(&r);
    assert_eq!(subs.len(), 1);
    assert!(subs[0].contains(&format!("shelve:{}: main-memo", toplevel(&r))));

    // main 側も同様に戻る。
    fs::write(r.join("a.txt"), "base\n").unwrap();
    let o = unshelve(&r, None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "from-main\n");
    assert!(stash_subjects(&r).is_empty());
}

#[test]
fn unshelve_without_shelve_exits_1() {
    let (_t, r) = repo();
    let o = unshelve(&r, None);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        err(&o),
        format!(
            "git-unshelve: この worktree の shelve はありません({})\n",
            toplevel(&r)
        )
    );
    assert!(out(&o).is_empty());
}

#[test]
fn unshelve_conflict_on_apply_keeps_entry() {
    let (_t, r) = repo();
    fs::write(r.join("a.txt"), "shelved\n").unwrap();
    assert_eq!(shelve(&r, &[]).status.code(), Some(0));
    fs::write(r.join("a.txt"), "local-uncommitted\n").unwrap();
    let o = unshelve(&r, None);
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(
        e.contains("git-unshelve: apply がコンフリクトしました。entry は残しています("),
        "{e}"
    );
    assert!(
        e.contains("解消後、この entry は手動で drop してください: git stash list で index を確認"),
        "{e}"
    );
    assert_eq!(stash_subjects(&r).len(), 1);
}

#[test]
fn unshelve_recovers_when_another_entry_is_dropped() {
    let (t, r) = repo();
    fs::write(r.join("a.txt"), "shelved\n").unwrap();
    assert_eq!(shelve(&r, &["mine"]).status.code(), Some(0));
    let marker = t.path().join("once");
    // drop 直前(index 解決の後)に別 worktree が push した状況を作る。
    // 1 回目だけ割り込み、index がずれて別 entry が消える → 復元 → 再試行。
    let hook = format!(
        "[ -e '{m}' ] || {{ : > '{m}'; w=$(git commit-tree 'HEAD^{{tree}}' -m w); c=$(git commit-tree 'HEAD^{{tree}}' -p HEAD -p \"$w\" -m x); git stash store -m 'shelve:/other/wt: theirs' \"$c\"; }}",
        m = marker.display()
    );
    let o = unshelve(&r, Some(&hook));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let e = err(&o);
    assert!(e.contains("競合を検出(期待 "), "{e}");
    assert!(e.contains("復元して再試行します(1/3)。"), "{e}");
    assert!(out(&o).contains("==> drop 完了: "));
    // 他 worktree の entry は元の message のまま無傷で残る。
    let subs = stash_subjects(&r);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert!(subs[0].contains("shelve:/other/wt: theirs"), "{subs:?}");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "shelved\n");
}

#[test]
fn unshelve_gives_up_after_three_attempts() {
    let (_t, r) = repo();
    fs::write(r.join("a.txt"), "shelved\n").unwrap();
    assert_eq!(shelve(&r, &["mine"]).status.code(), Some(0));
    // 毎回割り込む → 3 回とも誤 drop → 上限で exit 1。
    let hook = "w=$(git commit-tree 'HEAD^{tree}' -m w$attempt); c=$(git commit-tree 'HEAD^{tree}' -p HEAD -p \"$w\" -m x$attempt); git stash store -m 'shelve:/other/wt: theirs' \"$c\"";
    let o = unshelve(&r, Some(hook));
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(e.contains("復元して再試行します(1/3)。"), "{e}");
    assert!(e.contains("復元して再試行します(2/3)。"), "{e}");
    assert!(e.contains("復元して再試行します(3/3)。"), "{e}");
    assert!(
        e.contains(
            "git-unshelve: 再試行上限に達しました。apply は済んでいますが drop できていません("
        ),
        "{e}"
    );
    // 自分の entry 1 + 他 worktree の 3 本、どれも失われていない。
    let subs = stash_subjects(&r);
    assert_eq!(subs.len(), 4, "{subs:?}");
    assert_eq!(
        subs.iter()
            .filter(|s| s.contains("shelve:/other/wt: theirs"))
            .count(),
        3
    );
    assert_eq!(subs.iter().filter(|s| s.contains(": mine")).count(), 1);
}

#[test]
fn shelve_waits_for_the_lock() {
    let (_t, r) = repo();
    fs::write(r.join("a.txt"), "changed\n").unwrap();
    let common = git(
        &r,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    let lockpath = Path::new(&common).join("claude-shelve.lock");
    let held = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lockpath)
        .unwrap();
    held.lock().unwrap();

    let mut child = base_cmd(env!("CARGO_BIN_EXE_git-shelve"), &r)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(child.try_wait().unwrap().is_none(), "ロック保持中は待つ");
    assert!(stash_subjects(&r).is_empty());

    drop(held);
    let st = child.wait().unwrap();
    assert!(st.success());
    assert_eq!(stash_subjects(&r).len(), 1);
}

#[test]
fn outside_repo_fails_with_git_status() {
    let t = tempfile::tempdir().unwrap();
    for (bin, args) in [
        (env!("CARGO_BIN_EXE_git-shelve"), Vec::<&str>::new()),
        (env!("CARGO_BIN_EXE_git-unshelve"), Vec::<&str>::new()),
    ] {
        let o = base_cmd(bin, t.path())
            .args(args)
            .env("GIT_CEILING_DIRECTORIES", t.path().parent().unwrap())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(128), "{bin}: {}", err(&o));
        assert!(err(&o).contains("not a git repository"), "{}", err(&o));
    }
}
