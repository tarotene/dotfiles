//! git-unshelve — `git shelve` で積んだ、この worktree の entry だけを
//! apply して drop する。
//!
//! drop の安全性(TOCTOU 対策)は「drop の後に、実際に消えた SHA を期待 SHA と
//! 突き合わせ、不一致なら store で同一 SHA のまま復元して再試行」。
//! list 解決 → drop → 検証全体を `claude-shelve.lock` の flock で直列化する。
//! `GIT_UNSHELVE_TEST_PRE_DROP_HOOK` はテスト専用シーム(drop 直前に実行)。

use git_shelve::{
    dropped_sha, find_entry, git_capture, lock, lockfile_path, message_for, resolve_index, tag,
    toplevel,
};
use std::io::{pipe, Read};
use std::process::{exit, Command, Stdio};

const MAX_ATTEMPTS: u32 = 3;

fn list(format: &str) -> String {
    // bash の `$(git stash list ...)` 同様、失敗は空として扱う。
    git_capture(&["stash", "list", &format!("--format={format}")]).unwrap_or_default()
}

fn run_or_exit(cmd: &mut Command) {
    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(_) => exit(127),
    }
}

fn main() {
    let lockfile = lockfile_path().unwrap_or_else(|c| exit(c));
    let top = toplevel().unwrap_or_else(|c| exit(c));
    let tag = tag(&top);

    let Some(entry) = find_entry(&list("%H%x09%gs"), &tag) else {
        eprintln!("git-unshelve: この worktree の shelve はありません({top})");
        exit(1);
    };
    let (expected, original_message) = entry.split_once('\t').unwrap_or((&entry, ""));
    let expected = expected.to_string();

    println!("==> apply {expected} ({original_message})");
    let applied = Command::new("git")
        .args(["stash", "apply", &expected])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !applied {
        eprintln!("git-unshelve: apply がコンフリクトしました。entry は残しています({expected})。");
        eprintln!(
            "  解消後、この entry は手動で drop してください: git stash list で index を確認"
        );
        exit(1);
    }

    let _lock = match lock(&lockfile) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("git-unshelve: {lockfile}: {e}");
            exit(1);
        }
    };

    let mut attempt = 0;
    loop {
        attempt += 1;

        let Some(idx) = resolve_index(&list("%gd%x09%H"), &expected) else {
            eprintln!(
                "git-unshelve: apply 済みの entry が drop 前に見当たりません({expected})。手動で確認してください。"
            );
            exit(1);
        };

        if let Ok(hook) = std::env::var("GIT_UNSHELVE_TEST_PRE_DROP_HOOK") {
            if !hook.is_empty() {
                run_or_exit(
                    Command::new("bash")
                        .args(["-c", &hook])
                        .env("toplevel", &top)
                        .env("tag", &tag)
                        .env("expected_sha", &expected)
                        .env("idx", &idx)
                        .env("attempt", attempt.to_string()),
                );
            }
        }

        // 誤って別 entry が消された場合の元 message はここから引く。
        let pre_drop_snapshot = list("%H%x09%gs");

        // `2>&1` 相当: stdout/stderr を同じパイプへ。
        let (mut reader, writer) = pipe().unwrap_or_else(|e| {
            eprintln!("git-unshelve: pipe: {e}");
            exit(1)
        });
        let writer2 = writer.try_clone().unwrap_or_else(|e| {
            eprintln!("git-unshelve: pipe: {e}");
            exit(1)
        });
        let mut child = Command::new("git")
            .args(["stash", "drop", &idx])
            .stdout(Stdio::from(writer))
            .stderr(Stdio::from(writer2))
            .spawn()
            .unwrap_or_else(|e| {
                eprintln!("git-unshelve: git: {e}");
                exit(127)
            });
        let mut raw = Vec::new();
        let _ = reader.read_to_end(&mut raw);
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let mut drop_out = String::from_utf8_lossy(&raw).into_owned();
        while drop_out.ends_with('\n') {
            drop_out.pop();
        }
        if !ok {
            eprintln!("git-unshelve: git stash drop {idx} が失敗しました: {drop_out}");
            exit(1);
        }
        println!("{drop_out}");

        let dropped = dropped_sha(&drop_out);
        if dropped == expected {
            println!("==> drop 完了: {expected}");
            exit(0);
        }
        if dropped.is_empty() {
            eprintln!("git-unshelve: drop の出力から SHA を抽出できませんでした: {drop_out}");
            exit(1);
        }

        eprintln!(
            "git-unshelve: 競合を検出(期待 {expected}, 実際に消えたのは {dropped})。復元して再試行します({attempt}/{MAX_ATTEMPTS})。"
        );

        let msg = message_for(&pre_drop_snapshot, &dropped)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("shelve-recovered: {dropped}"));
        run_or_exit(Command::new("git").args(["stash", "store", "-m", &msg, &dropped]));

        if attempt >= MAX_ATTEMPTS {
            eprintln!(
                "git-unshelve: 再試行上限に達しました。apply は済んでいますが drop できていません({expected})。手動で確認してください。"
            );
            exit(1);
        }
    }
}
