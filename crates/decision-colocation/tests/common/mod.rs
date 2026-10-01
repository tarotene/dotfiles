//! 実際の一時 git repo を作るテスト補助(check.rs / guard.rs / cli.rs で共有)。
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        // 利用者の global 設定(commit 署名・hook 等)を遮断する。
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

pub fn commit_all(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            msg,
        ],
    );
}

pub fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

/// bash selftest の共通 base 状態: 1 本の着地済み ADR + 既存の非文書ファイル。
pub struct Repo {
    pub dir: tempfile::TempDir,
    pub base_sha: String,
}

impl Repo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        write(
            p,
            "docs/adr/0001-base.md",
            "# ADR-0001 — base\n\n## Context\n",
        );
        write(
            p,
            "config/claude/hooks/existing-gate.sh",
            "#!/usr/bin/env bash\ntrue\n",
        );
        commit_all(p, "base");
        let base_sha = git(p, &["rev-parse", "HEAD"]);
        Repo { dir, base_sha }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    pub fn reset_to_base(&self) {
        git(self.dir.path(), &["reset", "-q", "--hard", &self.base_sha]);
        git(self.dir.path(), &["clean", "-fdq"]);
    }

    pub fn write(&self, rel: &str, content: &str) {
        write(self.dir.path(), rel, content);
    }

    pub fn commit(&self, msg: &str) {
        commit_all(self.dir.path(), msg);
    }
}
