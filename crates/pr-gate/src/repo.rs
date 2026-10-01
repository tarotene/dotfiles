//! git だけで求まる材料(ネットワークに出るのは [`added_files`] の軽い fetch と
//! SessionStart の fetch だけ)。

use crate::trim_nl;
use regex::Regex;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

/// `git -C <dir> <args>` の (成功したか, stdout)。起動できなければ `None`。
pub fn git(dir: &Path, args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// 成功時だけ stdout(末尾改行除去)を返す。bash の `x="$(git …)" || x=""` の成功側。
pub fn git_ok(dir: &Path, args: &[&str]) -> Option<String> {
    match git(dir, args)? {
        (true, out) => Some(trim_nl(&out).to_string()),
        _ => None,
    }
}

/// 成功したか(出力は捨てる)。
pub fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    matches!(git(dir, args), Some((true, _)))
}

static NWO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$").expect("NWO_RE")
});

/// `git remote -v` の出力から "owner/repo" を取り出す(bash の `owner_repo()`)。
///
/// bash 版: `awk '/github\.com/{print $2; exit}'` →
/// `sed -nE 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p'`。
/// `crates/issue-index` の `owner_repo` とは repo 名にドットを許さない点が違う
/// (bash 版のコメントは「同一式の複製」と書いていたが実際には異なっていた)。
/// ここは pr-gate.sh の式に合わせる。
pub fn owner_repo_from_remote_v(remote_v: &str) -> Option<String> {
    let line = remote_v.split('\n').find(|l| l.contains("github.com"))?;
    let url = line.split_whitespace().nth(1)?;
    let c = NWO_RE.captures(url)?;
    Some(format!("{}/{}", &c[1], &c[2]))
}

pub fn owner_repo(project: &Path) -> Option<String> {
    let (_, out) = git(project, &["remote", "-v"])?;
    owner_repo_from_remote_v(&out)
}

/// allowlist ファイルに nwo がちょうど 1 行として載っているか
/// (`grep -vE '^[[:space:]]*(#|$)' | grep -qxF "$nwo"`)。
pub fn allowlisted(allowlist: &Path, nwo: &str) -> bool {
    std::fs::read_to_string(allowlist)
        .map(|s| s.split('\n').any(|l| l == nwo))
        .unwrap_or(false)
}

/// origin/<base> に対する (ahead, behind)。origin/<base> が手元に無い/不明なら
/// ("?", "?")。
pub fn ahead_behind(project: &Path, base: &str) -> (String, String) {
    let unknown = ("?".to_string(), "?".to_string());
    let origin = format!("origin/{base}");
    if !git_succeeds(project, &["rev-parse", "--verify", "-q", &origin]) {
        return unknown;
    }
    let range = format!("HEAD...origin/{base}");
    match git_ok(project, &["rev-list", "--left-right", "--count", &range]) {
        Some(ab) if !ab.is_empty() => {
            // bash: ahead="${ab%%$'\t'*}" / behind="${ab##*$'\t'}"
            let ahead = ab.split('\t').next().unwrap_or("").to_string();
            let behind = ab.rsplit('\t').next().unwrap_or("").to_string();
            (ahead, behind)
        }
        _ => unknown,
    }
}

/// 未コミットのファイル数(`git status --porcelain | wc -l`)。失敗時は "?"。
pub fn uncommitted_count(dir: &Path) -> String {
    match git(dir, &["status", "--porcelain"]) {
        Some((true, out)) => out.matches('\n').count().to_string(),
        _ => "?".to_string(),
    }
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn as_positive(s: &str) -> Option<u64> {
    if !is_number(s) {
        return None;
    }
    s.parse::<u64>().ok().filter(|n| *n > 0)
}

// --- hygiene advisories(事故①古い base / 事故④残骸): PR の有無を問わず計算できる ---
// 以下は API を叩かず git だけで求まる。SessionStart は「PR が無ければ完全沈黙」
// だったため、この worktree 自身が origin/main から複数コミット遅れていても、
// [gone] ブランチが溜まっていても一切表示されなかった。PR の有無に関わらず
// 単独の SessionStart advisory として出す(Stop の advisory は既存どおり
// 「他の block に相乗り、単独では終了を止めない」のままにする — 履歴改変を
// 自動化しない判断は変えない)。

/// stale base 行。base が空、origin/<base> が手元に無い、behind が 0 のいずれか
/// なら空文字(判定できないことを断定に変えない)。
pub fn stale_base_line(project: &Path, base: &str) -> String {
    if base.is_empty() {
        return String::new();
    }
    let (_, behind) = ahead_behind(project, base);
    match as_positive(&behind) {
        Some(_) => format!(
            "base 追従: origin/{base} から {behind} コミット遅れています(git pull --ff-only 等で追従してください)"
        ),
        None => String::new(),
    }
}

fn gone_count(project: &Path) -> usize {
    match git(
        project,
        &["for-each-ref", "--format=%(upstream:track)", "refs/heads"],
    ) {
        Some((true, out)) => out.split('\n').filter(|l| *l == "[gone]").count(),
        _ => 0,
    }
}

/// `[gone]` ブランチ本数の行。0 なら空文字。
pub fn gone_branches_line(project: &Path) -> String {
    match gone_count(project) {
        0 => String::new(),
        n => format!("残骸ブランチ: [gone] が {n} 本あります(git prune-branches で確認・削除)"),
    }
}

/// `[gone]` かつ未変更の worktree 数の行。0 なら空文字。dirty な worktree は本物の
/// 作業中の可能性があるので数えない(false positive を出さない側に倒す)。
pub fn stale_worktrees_line(project: &Path) -> String {
    let list = match git(project, &["worktree", "list", "--porcelain"]) {
        Some((_, out)) => out,
        None => String::new(),
    };
    let mut n = 0usize;
    for wt in list
        .split('\n')
        .filter_map(|l| l.strip_prefix("worktree "))
        .filter(|w| !w.is_empty())
    {
        let wt = Path::new(wt);
        let br = git_ok(wt, &["branch", "--show-current"]).unwrap_or_default();
        if br.is_empty() {
            continue;
        }
        let r = format!("refs/heads/{br}");
        let track = git_ok(project, &["for-each-ref", "--format=%(upstream:track)", &r])
            .unwrap_or_default();
        if track != "[gone]" {
            continue;
        }
        if uncommitted_count(wt) != "0" {
            continue;
        }
        n += 1;
    }
    match n {
        0 => String::new(),
        n => format!(
            "残骸 worktree: [gone] かつ未変更の worktree が {n} 個あります(git prune-worktrees で確認・削除)"
        ),
    }
}

// --- G_unpushed(PR 未作成時の push 忘れ): 事故③のうち G_push が届かない領域 -----
// PR を作るべきかには踏み込まず、「積んだコミットが 1 個もリモートに無い」事実
// だけを見る。解消は git push 1 回(push.autoSetupRemote が upstream を自動で
// 張るので --set-upstream の指定は要らない) — 履歴改変は伴わない。

/// upstream(無ければ origin/`<branch>`、それも無ければ origin/`<default>`)に対して
/// 未 push のコミット数。比較先が決まらなければ "?"。
pub fn unpushed_count(project: &Path, branch: &str) -> String {
    let mut upstream = git_ok(
        project,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .unwrap_or_default();
    if upstream.is_empty() {
        let ob = format!("origin/{branch}");
        if git_succeeds(project, &["rev-parse", "--verify", "-q", &ob]) {
            upstream = ob;
        } else if let Some(d) = hook_io::git::default_branch(project) {
            let od = format!("origin/{d}");
            if git_succeeds(project, &["rev-parse", "--verify", "-q", &od]) {
                upstream = od;
            }
        }
    }
    if upstream.is_empty() {
        return "?".to_string();
    }
    let range = format!("{upstream}..HEAD");
    match git_ok(project, &["rev-list", "--count", &range]) {
        Some(n) if !n.is_empty() => n,
        _ => "?".to_string(),
    }
}

/// `head_oid..HEAD` のコミット数(head_oid が手元に無ければ "?")。
pub fn unpushed_since(project: &Path, head_oid: &str) -> String {
    if !git_succeeds(project, &["cat-file", "-e", head_oid]) {
        return "?".to_string();
    }
    let range = format!("{head_oid}..HEAD");
    match git_ok(project, &["rev-list", "--count", &range]) {
        Some(n) if !n.is_empty() => n,
        _ => "?".to_string(),
    }
}

/// base ブランチの tip だけを軽く fetch してから merge-base 起点の追加ファイル
/// (`--diff-filter=A`)一覧を返す。取得できない場合は空。
pub fn added_files(project: &Path, base: &str) -> Vec<String> {
    let _ = git(project, &["fetch", "-q", "origin", base]);
    let range = format!("origin/{base}...HEAD");
    let out = match git(
        project,
        &[
            "diff",
            "--name-status",
            "--diff-filter=A",
            &range,
            "--",
            ".",
        ],
    ) {
        Some((_, out)) => out,
        None => return Vec::new(),
    };
    out.split('\n')
        .filter(|l| !l.is_empty())
        // `cut -f2-`: 最初のタブより後ろ(タブが無い行は行全体)。
        .map(|l| l.split_once('\t').map_or(l, |(_, rest)| rest).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_repo_forms() {
        let rv = "origin\thttps://github.com/example/example.git (fetch)\n";
        assert_eq!(
            owner_repo_from_remote_v(rv).as_deref(),
            Some("example/example")
        );
        assert_eq!(
            owner_repo_from_remote_v("o\tgit@github.com:a/b (fetch)").as_deref(),
            Some("a/b")
        );
        // pr-gate.sh の式は repo 名にドットを許さない。
        assert_eq!(
            owner_repo_from_remote_v("o\thttps://github.com/a/b.c.git (fetch)"),
            None
        );
        assert_eq!(owner_repo_from_remote_v("o\t/local (fetch)"), None);
    }

    #[test]
    fn allowlist_exact_line() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("allow");
        std::fs::write(&f, "# c\n  example/example\nfoo/bar\n").unwrap();
        assert!(allowlisted(&f, "foo/bar"));
        assert!(!allowlisted(&f, "example/example"));
        assert!(!allowlisted(&d.path().join("none"), "foo/bar"));
    }
}
