//! worktree-fresh-base — pristine な worktree を SessionStart で
//! `origin/<base>` へ黙って fast-forward する(`config/claude/hooks/
//! worktree-fresh-base.sh` の移植、ADR-0024)。設計と根拠:
//! docs/claude/worktree-fresh-base.md。
//!
//! 動かしてよいのは「何も積んでいない worktree」だけ: 作業ツリーがクリーン
//! かつ ahead==0 かつ behind>0。`git merge --ff-only` で原子的に前提を再強制
//! する。全失敗経路は fail-open(無出力 exit 0)。動かした場合だけ
//! `additionalContext` を stdout に出す。
//!
//! 環境変数: `CLAUDE_PROJECT_DIR`(無ければ stdin の `.cwd`)、
//! `WORKTREE_FRESH_BASE_FETCH_TTL`(秒、既定 600: FETCH_HEAD がこれより新しけ
//! れば fetch を省く)。

use serde_json::json;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// `git -C <dir> <args>` の stdout(末尾改行除去)。起動失敗・非 0 終了は `None`。
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// `timeout 15 git fetch --quiet origin <base>` 相当。成功時のみ true。
fn fetch(dir: &Path, base: &str) -> bool {
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["fetch", "--quiet", "origin", base])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if start.elapsed() < FETCH_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn run() -> Option<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok();
    let input = hook_io::HookInput::parse(&raw).unwrap_or_default();
    let project = input.project_dir()?;
    let project = project.as_path();

    git(project, &["rev-parse", "--is-inside-work-tree"])?;

    let branch = git(project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return None; // detached HEAD / rebase 中は触らない
    }
    let base = hook_io::git::default_branch(project)?;
    if branch == base {
        return None; // 自分自身が base なら FF する意味がない
    }
    // status が失敗しても出力は空として続行する(bash 版と同じ)。
    if !git(project, &["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty()
    {
        return None;
    }

    let common_dir = hook_io::git::git_common_dir(project)?;
    let ttl: u64 = std::env::var("WORKTREE_FRESH_BASE_FETCH_TTL")
        .ok()
        .filter(|v| !v.is_empty())
        .map_or(600, |v| v.parse().unwrap_or(0));
    let mut do_fetch = true;
    if let Ok(meta) = std::fs::metadata(common_dir.join("FETCH_HEAD")) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now.saturating_sub(mtime) < ttl {
            do_fetch = false;
        }
    }
    if do_fetch && !fetch(project, &base) {
        return None;
    }
    let origin_base = format!("origin/{base}");
    git(project, &["rev-parse", "--verify", "-q", &origin_base])?;

    let ab = git(
        project,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{origin_base}"),
        ],
    )?;
    let ahead = ab.split('\t').next().unwrap_or("");
    let behind = ab.rsplit('\t').next().unwrap_or("");
    if ahead != "0" {
        return None;
    }
    let behind_n: u64 = behind.parse().ok()?;
    if behind_n == 0 {
        return None;
    }

    let before = git(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    git(project, &["merge", "--ff-only", "--quiet", &origin_base])?;
    let after = git(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    if before.is_empty() || after.is_empty() || before == after {
        return None;
    }

    let ctx = format!("[worktree-fresh-base] pristine worktree を origin/{base} へ {behind} コミット fast-forward しました({before} -> {after})。他 hook の SessionStart advisory(pr-gate の base 追従など)は並列実行の都合でこの更新前の状態を見ている場合があります。");
    println!(
        "{}",
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": ctx}})
    );
    Some(())
}

fn main() {
    let _ = run();
}
