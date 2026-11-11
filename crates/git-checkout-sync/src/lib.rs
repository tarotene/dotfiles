//! git-checkout-sync — herdr が worktree を fork する元の *親* checkout を
//! origin/<base> へ fast-forward し続ける(旧 scripts/git-checkout-freshness、
//! 設計根拠は docs/agents/git-checkout-sync.md)。
//!
//! 安全条件(すべて AND、path ごとに判定。fetch は最後の方): 存在する
//! git work tree / 現在 branch が非空 / 現在 branch が default branch /
//! `status --porcelain` が空 / ahead == 0 / behind > 0。満たしたときだけ
//! `merge --ff-only --quiet origin/<base>`(reset --hard は使わない)。
//!
//! fast-forward のあと、追跡ファイルが 1 つも無くなった旧ディレクトリ(ファイルを別の場所へ移した
//! PR の残骸。ビルド成果物・`.env` などの無視されたファイル、submodule の作業コピー)は、削除せず、
//! 状態ディレクトリへ丸ごと退避する(`quarantine_leftovers`)。残骸があると `status --porcelain`
//! が空でなくなり、以後の同期が毎回止まり、`main-checkout-guard` も通常の変更と数える(#1505)。
//! `.gitignore` は秘密のファイルも無視するので、追跡ファイルの無さは「捨ててよい」の根拠にならない。
//!
//! 縮退: 各 path は独立に fail-open。skip 理由は stderr に出すだけで、
//! usage エラー以外では常に exit 0。

use git_repository::ok as git_ok;
use git_repository::out as git_out;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::{Command, Stdio};
use std::time::Duration;

/// fetch の打ち切り秒数(旧版の `timeout 15`)。
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

pub use process_control::status_with_group_timeout as run_with_timeout;

fn skip(project: &str, why: &str) -> bool {
    eprintln!("SKIP {project}: {why}");
    false
}

/// 1 repo を処理する。true = 更新済み or 何もすることなし、false = skip/失敗(致命ではない)。
pub fn process_one(project: &str) -> bool {
    process_one_with(project, FETCH_TIMEOUT)
}

pub fn process_one_with(project: &str, fetch_timeout: Duration) -> bool {
    if project.is_empty() || !Path::new(project).is_dir() {
        return skip(project, "no such directory");
    }
    if !git_ok(project, &["rev-parse", "--is-inside-work-tree"]) {
        return skip(project, "not a git work tree");
    }

    let branch = git_out(project, &["branch", "--show-current"]).unwrap_or_default();
    if branch.is_empty() {
        return skip(project, "detached HEAD or mid-rebase");
    }

    let Some(base) = git_repository::default_branch(Path::new(project)) else {
        return skip(project, "origin/HEAD not set");
    };
    if branch != base {
        return skip(
            project,
            &format!("on '{branch}', not the default branch '{base}'"),
        );
    }

    if !git_out(project, &["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty()
    {
        return skip(project, "working tree not clean");
    }

    if !git_repository::fetch_inherited_stdout(Path::new(project), &base, fetch_timeout) {
        return skip(project, "fetch failed or timed out");
    }
    let origin_ref = format!("origin/{base}");
    if !git_ok(project, &["rev-parse", "--verify", "-q", &origin_ref]) {
        return skip(
            project,
            &format!("origin/{base} not resolvable after fetch"),
        );
    }

    let range = format!("HEAD...{origin_ref}");
    let Some(ab) = git_out(project, &["rev-list", "--left-right", "--count", &range]) else {
        return skip(project, "rev-list failed");
    };
    let ahead = ab.split('\t').next().unwrap_or("");
    let behind = ab.rsplit('\t').next().unwrap_or("");
    if ahead != "0" {
        return skip(
            project,
            &format!("{ahead} local commit(s) not on origin/{base}"),
        );
    }
    let behind_n = behind
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| behind.parse::<u64>().ok())
        .flatten()
        .unwrap_or(0);
    if behind.is_empty() || behind_n == 0 {
        eprintln!("OK   {project}: already current with origin/{base}");
        return true;
    }

    let before = git_out(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let before_full = git_out(project, &["rev-parse", "HEAD"]).unwrap_or_default();
    if !git_ok(project, &["merge", "--ff-only", "--quiet", &origin_ref]) {
        return skip(
            project,
            "fast-forward merge failed (history moved under us?)",
        );
    }
    let after = git_out(project, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    eprintln!("OK   {project}: fast-forwarded {before} -> {after} ({behind} commit(s))");
    let after_full = git_out(project, &["rev-parse", "HEAD"]).unwrap_or_default();
    quarantine_leftovers(project, &before_full, &after_full);
    true
}

/// 退避先の親。`GIT_CHECKOUT_SYNC_LEFTOVERS_DIR`(試験用)か、
/// `${XDG_STATE_HOME:-$HOME/.local/state}/claude/git-checkout-sync/leftovers`(sandbox の書き込み許可の中)。
fn leftovers_root() -> Option<PathBuf> {
    process_env::var_nonempty_path("GIT_CHECKOUT_SYNC_LEFTOVERS_DIR").or_else(|| {
        Some(
            process_env::Xdg::State
                .resolve()?
                .join("claude")
                .join("git-checkout-sync")
                .join("leftovers"),
        )
    })
}

/// `before..after` で消えたパスの親ディレクトリのうち、追跡ファイルが 1 つも残っていないもの
/// (入れ子の内側は、外側を動かせば一緒に動くので数えない)。パスは repo の root からの相対。
fn leftover_dirs(project: &str, before: &str, after: &str) -> Vec<String> {
    let Some(deleted) = git_out(
        project,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=D",
            before,
            after,
        ],
    ) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = Vec::new();
    for path in deleted.lines().filter(|l| !l.is_empty()) {
        let mut cur = path;
        while let Some((parent, _)) = cur.rsplit_once('/') {
            if !dirs.iter().any(|d| d == parent) {
                dirs.push(parent.to_string());
            }
            cur = parent;
        }
    }
    // 外側から先に決める。外側を選んだら、その内側は選ばない。
    dirs.sort_by_key(|d| d.matches('/').count());
    let mut chosen: Vec<String> = Vec::new();
    for dir in dirs {
        if chosen.iter().any(|c| dir.starts_with(&format!("{c}/"))) {
            continue;
        }
        let on_disk = Path::new(project).join(&dir);
        let is_real_dir = std::fs::symlink_metadata(&on_disk)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        if !is_real_dir {
            continue;
        }
        let tracked = git_out(project, &["ls-files", "--", &dir]).unwrap_or_default();
        if tracked.is_empty() {
            chosen.push(dir);
        }
    }
    chosen
}

/// 追跡ファイルが 1 つも無くなった旧ディレクトリを、削除せず、退避先へ丸ごと移す。退避先は
/// `<leftovers_root>/<repo 名>-<UTC 時刻>/<元の相対パス>`。移せなかったときは、そのまま残して報告する
/// (データを失う操作は持たない)。
fn quarantine_leftovers(project: &str, before: &str, after: &str) {
    if before.is_empty() || after.is_empty() {
        return;
    }
    let dirs = leftover_dirs(project, before, after);
    if dirs.is_empty() {
        return;
    }
    let Some(root) = leftovers_root() else {
        eprintln!(
            "LEFT {project}: {} dir(s) have no tracked files, but no state dir to move them to",
            dirs.len()
        );
        return;
    };
    let name = Path::new(project)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");
    let batch = root.join(format!(
        "{name}-{}",
        civil_time::utc_timestamp(civil_time::now_unix())
    ));
    for dir in dirs {
        let from = Path::new(project).join(&dir);
        let to = batch.join(&dir);
        let moved = to
            .parent()
            .map(std::fs::create_dir_all)
            .unwrap_or(Ok(()))
            .and_then(|()| std::fs::rename(&from, &to));
        match moved {
            Ok(()) => eprintln!(
                "MOVED {project}: {dir}/ has no tracked files (the PR moved them); moved intact to {}",
                to.display()
            ),
            Err(e) => eprintln!(
                "LEFT {project}: {dir}/ has no tracked files but could not be moved ({e}); left in place. Move or remove it by hand"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn timeout_kills_slow_command() {
        let t = Instant::now();
        let ok = run_with_timeout(
            Command::new("sleep")
                .arg("10")
                .stdin(Stdio::null())
                .stdout(Stdio::null()),
            Duration::from_millis(300),
        );
        assert!(!ok);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn fast_success_and_failure() {
        assert!(run_with_timeout(
            &mut Command::new("true"),
            Duration::from_secs(5)
        ));
        assert!(!run_with_timeout(
            &mut Command::new("false"),
            Duration::from_secs(5)
        ));
        assert!(!run_with_timeout(
            &mut Command::new("/nonexistent/cmd"),
            Duration::from_secs(5)
        ));
    }
}
