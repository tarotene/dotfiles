//! `owner/repo` と default branch の解決(gh 投稿系 guard の共通部分)。
//!
//! 吸収元: stack-base-guard.sh / pr-title-guard.sh / decision-colocation-guard.sh
//! の `owner_repo()`(issue-index.sh / pr-gate.sh と「同一式の複製」と各所の
//! コメントが書いていたもの)と、stack-base-guard.sh / decision-colocation-
//! guard.sh の `default_branch()`(origin/HEAD → `gh repo view` の順)。
//!
//! `hook_io::git::origin_nwo` とは別物: あちらは `remote.origin.url` だけを
//! 見る。こちらは bash と同じく `git remote -v` の最初の github.com 行を見る
//! (origin 以外の remote 名でも拾う)。

use std::path::Path;
use std::process::{Command, Stdio};

fn stdout_of(cmd: &mut Command) -> Option<String> {
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `owner_repo()`: `git -C <project> remote -v` の最初の github.com 行の
/// URL を `owner/repo` にする。解決できなければ `None`。
pub fn owner_repo(project: &Path) -> Option<String> {
    // bash は `git … | awk …` のパイプで、git の失敗は無視して awk の出力だけを
    // 見る。git が失敗すれば出力は空なので同じ結果になる。
    let out = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["remote", "-v"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_owner_repo_url(remote_v_github_url(&text)?)
}

/// `awk '/github\.com/{print $2; exit}'`: `github.com` を含む最初の行の
/// 第 2 フィールド。空なら `None`。
pub fn remote_v_github_url(remote_v: &str) -> Option<&str> {
    let line = remote_v.lines().find(|l| l.contains("github.com"))?;
    line.split([' ', '\t']).filter(|f| !f.is_empty()).nth(1)
}

/// `sed -nE 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p'`。
///
/// repo 名にドットを含む URL(`o/foo.bar`)は一致しない — bash と同じ。
pub fn parse_owner_repo_url(url: &str) -> Option<String> {
    // `.*` は最長一致なので、最後の `github.com` から順に試す。
    let mut idx: Vec<usize> = url.match_indices("github.com").map(|(i, _)| i).collect();
    idx.reverse();
    idx.into_iter().find_map(|at| {
        let rest = &url[at + "github.com".len()..];
        let sep = rest.len() - rest.trim_start_matches([':', '/']).len();
        if sep == 0 {
            return None;
        }
        let rest = &rest[sep..];
        let (owner, rest) = rest.split_once('/')?;
        if owner.is_empty() {
            return None;
        }
        let end = rest.find(['/', '.']).unwrap_or(rest.len());
        let (repo, tail) = rest.split_at(end);
        if repo.is_empty() || !matches!(tail, "" | "/" | ".git" | ".git/") {
            return None;
        }
        Some(format!("{owner}/{repo}"))
    })
}

/// `default_branch()`(stack-base-guard.sh 版): `origin/HEAD` の symref を
/// ローカルで読み、無ければ `gh repo view -R <nwo>` に問い合わせる
/// (ローカルで解決できればネットワーク往復を増やさない)。空なら `None`。
pub fn default_branch_or_gh(project: &Path, nwo: &str) -> Option<String> {
    if let Some(b) = hook_io::git::default_branch(project) {
        return Some(b);
    }
    let out = stdout_of(Command::new("gh").args([
        "repo",
        "view",
        "-R",
        nwo,
        "--json",
        "defaultBranchRef",
        "-q",
        ".defaultBranchRef.name // empty",
    ]))?;
    let b = out.trim_end_matches('\n');
    (!b.is_empty()).then(|| b.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_urls_like_sed() {
        let p = parse_owner_repo_url;
        assert_eq!(p("git@github.com:o/r.git").as_deref(), Some("o/r"));
        assert_eq!(p("https://github.com/o/r").as_deref(), Some("o/r"));
        assert_eq!(p("https://github.com/o/r/").as_deref(), Some("o/r"));
        assert_eq!(
            p("ssh://git@github.com/o.x/r.git/").as_deref(),
            Some("o.x/r")
        );
        assert_eq!(p("https://github.com/o/foo.bar"), None);
        assert_eq!(p("https://github.com/o/r/tree"), None);
        assert_eq!(p("https://gitlab.com/o/r"), None);
        assert_eq!(p("github.com"), None);
    }

    #[test]
    fn remote_v_first_github_line() {
        let s = "up\thttps://example.com/x (fetch)\norigin\tgit@github.com:o/r.git (fetch)\norigin\tgit@github.com:o/r.git (push)\n";
        assert_eq!(remote_v_github_url(s), Some("git@github.com:o/r.git"));
        assert_eq!(remote_v_github_url("none\n"), None);
    }
}
