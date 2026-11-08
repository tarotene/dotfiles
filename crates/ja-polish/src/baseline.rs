//! 文書の「基点」(書き直しの対象を、追加・変更した段落に絞るための、変更前の内容)。
//!
//! git の作業ツリーの中なら、origin の既定ブランチとの merge-base にある同じパスの内容。
//! 基点が無い(git の外、新規のファイル、origin が無い)ときは `None` で、文書全体が対象になる。
//! 一度通した段落は記録で分かるので、基点が取れなくても通し直しで書き換えは起きない。

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `path` の基点の内容。
pub fn git_baseline(path: &Path) -> Option<String> {
    let abs = path.canonicalize().ok()?;
    let dir = abs.parent()?;
    let top = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?.trim());
    let rel = abs.strip_prefix(&top).ok()?;
    let remote = git(
        &top,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "origin/main".to_string());
    let base = git(&top, &["merge-base", "HEAD", &remote])?
        .trim()
        .to_string();
    git(
        &top,
        &["show", &format!("{base}:{}", rel.to_string_lossy())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn baseline_is_the_merge_base_content_and_none_outside_git_or_for_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sh(root, &["init", "-q", "-b", "main"]);
        sh(root, &["config", "user.email", "t@example.com"]);
        sh(root, &["config", "user.name", "t"]);
        sh(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a.md"), "基点の段落です。\n").unwrap();
        sh(root, &["add", "a.md"]);
        sh(root, &["commit", "-q", "-m", "base"]);
        // origin/main を同じコミットに置く(リモート無しで merge-base を試すため)。
        sh(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        std::fs::write(root.join("a.md"), "基点の段落です。\n\n足した段落です。\n").unwrap();
        std::fs::write(root.join("new.md"), "新規のファイルです。\n").unwrap();

        assert_eq!(
            git_baseline(&root.join("a.md")).as_deref(),
            Some("基点の段落です。\n")
        );
        assert_eq!(git_baseline(&root.join("new.md")), None);

        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("b.md"), "x\n").unwrap();
        assert_eq!(git_baseline(&outside.path().join("b.md")), None);
        assert_eq!(git_baseline(&root.join("missing.md")), None);
    }
}
