//! git の読み取り系ヘルパ(クラスタ B・F)。どれもネットワークに出ない。

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// 静かな `git -C <dir> <args>`: stdin は閉じ、stderr は捨てる。起動できなければ `None`。
///
/// 各 hook/CLI が同じ形のラッパを private に持っていた(`git_ok` / `git_out` の約 10 複写)。
/// 違いは戻り値の形(成否だけか、stdout か)と末尾の改行の扱いだけなので、核をここに置く。
pub fn run(dir: impl AsRef<Path>, args: &[&str]) -> Option<Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir.as_ref())
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
}

/// 成功時の stdout のバイト列(加工なし)。失敗・起動不能は `None`。
pub fn raw(dir: impl AsRef<Path>, args: &[&str]) -> Option<Vec<u8>> {
    run(dir, args)
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
}

/// bash の `$(git -C <dir> …)`: 成功時の stdout(UTF-8 として読めない部分は U+FFFD)から
/// 末尾の改行をすべて落とす。空出力は `Some("")`、失敗・起動不能は `None`。
pub fn out(dir: impl AsRef<Path>, args: &[&str]) -> Option<String> {
    let bytes = raw(dir, args)?;
    Some(
        String::from_utf8_lossy(&bytes)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// `git -C <dir> <args>` が成功したか(stdout も捨てる)。
pub fn ok(dir: impl AsRef<Path>, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir.as_ref())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// 空でない 1 つの値を返す系(`rev-parse` 等)用: UTF-8 として読めない出力は `None`、
/// 末尾の `\n` / `\r` を落とし、空なら `None`。
fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let s = String::from_utf8(raw(dir, args)?).ok()?;
    let s = s.trim_end_matches(['\n', '\r']).to_string();
    (!s.is_empty()).then_some(s)
}

/// クラスタ B: `refs/remotes/origin/HEAD` から default branch を読み、
/// `origin/` を剥がす。未設定なら `None` — 呼び出し側は判定できないことを
/// 断定に変えない(ネットワーク照会へのフォールバックは呼び出し側の責務)。
///
/// 吸収元(同一式 4 箇所 + 変種 1 箇所):
/// - `config/claude/hooks/pr-gate.sh:404`
/// - `config/claude/hooks/plan-fresh-gate.sh:88`(「3 箇所とも揃えること」
///   とコメントしているが実数は 5 箇所だった)
/// - `config/claude/hooks/worktree-fresh-base.sh:49`
/// - `scripts/git-checkout-freshness:43`
/// - 変種 `config/claude/hooks/stack-base-guard.sh:88`(失敗時に `gh repo view` へ)
pub fn default_branch(repo: &Path) -> Option<String> {
    let r = git_stdout(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )?;
    Some(r.strip_prefix("origin/").unwrap_or(&r).to_string())
}

/// 現在の worktree の作業ツリー先頭(`git rev-parse --show-toplevel`)。
/// `git_common_dir` と異なり worktree ごとに異なるパスを返す — ADR-543
/// (`crates/new-tool-guard`)の session ledger キーはこちらを使う。
pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    git_stdout(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// クラスタ F: 絶対パスの `--git-common-dir`(worktree 間で共有される .git)。
///
/// 吸収元: `pr-gate.sh:191` / `worktree-fresh-base.sh:42` /
/// `scripts/git-audit-worktrees:39` / `scripts/git-prune-worktrees:242`。
pub fn git_common_dir(repo: &Path) -> Option<PathBuf> {
    git_stdout(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(PathBuf::from)
}

/// `remote.origin.url` を `owner/repo` に解析する(ローカルの git config のみ)。
pub fn origin_nwo(repo: &Path) -> Option<String> {
    parse_github_nwo(&git_stdout(
        repo,
        &["config", "--get", "remote.origin.url"],
    )?)
}

/// GitHub の remote URL(https / ssh / scp 形式)から `owner/repo` を取り出す。
pub fn parse_github_nwo(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        // `-c core.hooksPath=`(空文字)でこの一時 repo に対する git hooks を
        // 常に無効化する(#428): home-manager がホストの ~/.gitconfig に
        // configure する protected-branch guard(config/git/hooks/pre-commit)
        // が `core.hooksPath` 経由で効いている環境だと、`main` への直接
        // commit がこの一時 repo でもブロックされ、テストが spurious に
        // 落ちる。CI は home-manager 由来の hook が無いホームで走るため
        // 再現しないが、ローカルでは常に効かせておく。
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("-c")
            .arg("core.hooksPath=")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        git(
            d.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        d
    }

    #[test]
    fn default_branch_unset_is_none() {
        let d = repo();
        assert_eq!(default_branch(d.path()), None);
    }

    #[test]
    fn default_branch_strips_origin() {
        let d = repo();
        git(
            d.path(),
            &["update-ref", "refs/remotes/origin/trunk", "HEAD"],
        );
        git(
            d.path(),
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );
        assert_eq!(default_branch(d.path()).as_deref(), Some("trunk"));
    }

    #[test]
    fn toplevel_is_worktree_root() {
        let d = repo();
        assert_eq!(toplevel(d.path()), Some(d.path().to_path_buf()));
        assert_eq!(toplevel(Path::new("/")), None);
    }

    #[test]
    fn common_dir_is_absolute() {
        let d = repo();
        let c = git_common_dir(d.path()).unwrap();
        assert!(c.is_absolute());
        assert!(c.ends_with(".git"));
        assert_eq!(git_common_dir(Path::new("/")), None);
    }

    #[test]
    fn origin_nwo_from_config() {
        let d = repo();
        assert_eq!(origin_nwo(d.path()), None);
        git(
            d.path(),
            &["remote", "add", "origin", "git@github.com:octo/hello.git"],
        );
        assert_eq!(origin_nwo(d.path()).as_deref(), Some("octo/hello"));
    }

    #[test]
    fn parse_nwo_forms() {
        for u in [
            "https://github.com/o/r",
            "https://github.com/o/r.git",
            "https://github.com/o/r/",
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
        ] {
            assert_eq!(parse_github_nwo(u).as_deref(), Some("o/r"), "{u}");
        }
        for u in [
            "https://gitlab.com/o/r",
            "https://github.com/o",
            "https://github.com/o/r/x",
            "/local/path",
        ] {
            assert_eq!(parse_github_nwo(u), None, "{u}");
        }
    }

    #[test]
    fn quiet_helpers() {
        let d = repo();
        assert_eq!(
            out(d.path(), &["branch", "--show-current"]).as_deref(),
            Some("main")
        );
        assert_eq!(
            out(d.path(), &["status", "--porcelain"]).as_deref(),
            Some("")
        );
        assert_eq!(out(d.path(), &["no-such-subcommand"]), None);
        assert!(ok(d.path(), &["rev-parse", "--is-inside-work-tree"]));
        assert!(!ok(d.path(), &["rev-parse", "no-such-ref"]));
        assert_eq!(
            raw(d.path(), &["branch", "--show-current"]),
            Some(b"main\n".to_vec())
        );
        assert!(run(Path::new("/nonexistent-dir-for-test"), &["status"])
            .is_some_and(|o| !o.status.success()));
    }
}
