//! git-shelve — worktree 単位で所有権が分かる stash push のラッパー。
//!
//! Usage: git shelve [<メモ>]
//! message に現在の worktree の絶対パスをタグ("shelve:<toplevel>: <メモ>")
//! として埋め込む。対の `git unshelve` はこのタグで自分の entry だけを解決する。
//! push は unshelve と同じ `claude-shelve.lock` に参加させる。

use std::process::{exit, Command};

fn main() {
    let note = std::env::args().nth(1).unwrap_or_default();
    let top = git_shelve::toplevel().unwrap_or_else(|c| exit(c));
    let message = git_shelve::message(&top, &note);

    let lockfile = git_shelve::lockfile_path().unwrap_or_else(|c| exit(c));
    // stash push の間保持する(スコープ末尾で drop = unlock)。
    let _lock = match git_shelve::lock(&lockfile) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("git-shelve: {lockfile}: {e}");
            exit(1);
        }
    };

    let st = Command::new("git")
        .args([
            "stash",
            "push",
            "--include-untracked",
            "--message",
            &message,
        ])
        .status();
    match st {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("git-shelve: git: {e}");
            exit(127);
        }
    }
}
