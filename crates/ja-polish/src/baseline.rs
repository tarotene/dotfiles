//! 文書の「基点」(書き直しの対象を、追加・変更した段落に絞るための、変更前の内容)。
//!
//! git の作業ツリーの中なら、origin の既定ブランチとの merge-base にある同じパスの内容。`git mv` で改名してきたファイルは、
//! 元のパスの内容(改名の検出は git の `-M`)。中身を変えずに移しただけのファイルは、全段落が「新しい」と数えられない。
//! 基点が無い(git の外、origin が無い)ときは `None` で、文書全体が対象になる。新規に作ったファイルは、スナップショット(`snapshot.rs`)の空の基点で、同じく文書全体が対象。
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

fn toplevel(dir: &Path) -> Option<PathBuf> {
    Some(PathBuf::from(
        git(dir, &["rev-parse", "--show-toplevel"])?.trim(),
    ))
}

/// origin の既定ブランチとの merge-base のコミット。
fn merge_base(top: &Path) -> Option<String> {
    let remote = git(
        top,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "origin/main".to_string());
    Some(
        git(top, &["merge-base", "HEAD", &remote])?
            .trim()
            .to_string(),
    )
}

/// `rel`(リポジトリのルートからの相対パス)が、merge-base から改名されてきたファイルなら、merge-base での元のパス。
/// 改名の検出は git に任せる(`-M`。既定の類似度)。`git mv` しただけのファイルは、元のパスの内容が基点になる。
fn renamed_from(top: &Path, base: &str, rel: &str) -> Option<String> {
    let out = git(
        top,
        &["diff", "-M", "--name-status", "-z", "--diff-filter=R", base],
    )?;
    // `-z` の name-status は、`R<類似度>\0<元>\0<先>\0` の並び。
    let mut fields = out.split('\0').filter(|s| !s.is_empty());
    while let Some(status) = fields.next() {
        if status.starts_with('R') {
            let (old, new) = (fields.next()?, fields.next()?);
            if new == rel {
                return Some(old.to_string());
            }
        }
    }
    None
}

/// `path` の git 上の基点の内容。改名されてきたファイルは、元のパスの内容。
pub fn git_baseline(path: &Path) -> Option<String> {
    let abs = path.canonicalize().ok()?;
    let top = toplevel(abs.parent()?)?;
    let rel = abs.strip_prefix(&top).ok()?.to_string_lossy().into_owned();
    let base = merge_base(&top)?;
    git(&top, &["show", &format!("{base}:{rel}")]).or_else(|| {
        let old = renamed_from(&top, &base, &rel)?;
        git(&top, &["show", &format!("{base}:{old}")])
    })
}

/// `path` の基点。git の merge-base にあればその内容、無ければ書き込み前に残したスナップショット
/// (git の外の文書。[`crate::snapshot`])。どちらも無ければ `None`(文書全体が対象)。
pub fn baseline_for(path: &Path) -> Option<String> {
    git_baseline(path).or_else(|| crate::snapshot::load(path))
}

/// `dir` の git の作業ツリーで、merge-base から変わった(追加・変更・改名の)ファイルと、
/// 未追跡のファイル。パスは絶対パス。git の外なら空。
pub fn git_changed_files(dir: &Path) -> Vec<PathBuf> {
    let Some(top) = toplevel(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    if let Some(base) = merge_base(&top) {
        if let Some(out) = git(
            &top,
            &["diff", "--name-only", "-z", "--diff-filter=AMR", &base],
        ) {
            names.extend(out.split('\0').filter(|s| !s.is_empty()).map(String::from));
        }
    }
    if let Some(out) = git(&top, &["ls-files", "--others", "--exclude-standard", "-z"]) {
        names.extend(out.split('\0').filter(|s| !s.is_empty()).map(String::from));
    }
    names.sort();
    names.dedup();
    names.into_iter().map(|n| top.join(n)).collect()
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

        let changed = git_changed_files(root);
        let names: Vec<String> = changed
            .iter()
            .map(|p| {
                p.strip_prefix(root.canonicalize().unwrap())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec!["a.md", "new.md"],
            "変更した追跡ファイルと未追跡のファイル"
        );

        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("b.md"), "x\n").unwrap();
        assert_eq!(git_baseline(&outside.path().join("b.md")), None);
        assert_eq!(git_baseline(&root.join("missing.md")), None);
    }

    #[test]
    fn a_renamed_file_takes_the_content_of_its_old_path_as_the_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sh(root, &["init", "-q", "-b", "main"]);
        sh(root, &["config", "user.email", "t@example.com"]);
        sh(root, &["config", "user.name", "t"]);
        sh(root, &["config", "commit.gpgsign", "false"]);
        let body = "移すだけの文書の、一つ目の段落です。内容は十分に長くしておきます。\n\n二つ目の段落です。これも、改名の検出に足りる長さにします。\n\n三つ目の段落です。\n";
        std::fs::create_dir_all(root.join("old")).unwrap();
        std::fs::write(root.join("old/a.md"), body).unwrap();
        std::fs::write(root.join("keep.md"), "動かさない文書です。\n").unwrap();
        sh(root, &["add", "-A"]);
        sh(root, &["commit", "-q", "-m", "base"]);
        sh(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

        // git mv だけ(中身は変えない): 基点は元のパスの内容と同じ。
        std::fs::create_dir_all(root.join("new")).unwrap();
        sh(root, &["mv", "old/a.md", "new/a.md"]);
        assert_eq!(git_baseline(&root.join("new/a.md")).as_deref(), Some(body));

        // 移して、1 段落だけ足した: 基点は元の内容。足した段落だけが「新しい」。
        let edited = format!("{body}\n足した段落です。\n");
        std::fs::write(root.join("new/a.md"), &edited).unwrap();
        assert_eq!(git_baseline(&root.join("new/a.md")).as_deref(), Some(body));
        let pending = crate::polish::pending(
            &edited,
            git_baseline(&root.join("new/a.md")).as_deref(),
            &Default::default(),
        );
        assert_eq!(pending.len(), 1);

        // 動かしていないファイルは、これまでどおり同じパスの内容。
        assert_eq!(
            git_baseline(&root.join("keep.md")).as_deref(),
            Some("動かさない文書です。\n")
        );
        // 改名でなく、ただ新しく作ったファイルは、基点なし。
        std::fs::write(root.join("fresh.md"), "まったく新しい文書です。\n").unwrap();
        assert_eq!(git_baseline(&root.join("fresh.md")), None);
    }
}
