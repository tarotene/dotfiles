//! plan-fresh-gate — ExitPlanMode 直前に origin/<base> の進行を検出し、
//! プランが参照しているファイルと交差するときだけ deny する
//! (旧 `config/claude/hooks/plan-fresh-gate.sh`、ADR-0024 Stage 4b #412)。
//!
//! 設計と根拠: docs/claude/plan-fresh-gate.md
//!
//! 判定は 2 段:
//!   1) 移動: worktree-fresh-base と同じ pristine 5 条件(branch 非空・
//!      branch != base・clean・ahead==0・behind>0)を満たすときだけ
//!      `git merge --ff-only` で追従する。
//!   2) deny 判定: 移動の可否とは独立に、常に fetch し、origin/<base> の
//!      進行分(from..to)が変更したファイルと、プラン本文が参照している
//!      ファイル(フルパス部分一致 or basename 部分一致)が交差するかを判定する。
//!
//! 収束保証: deny した時点の origin/<base> の SHA をセッション単位の台帳
//! (`hook_io::SessionLedger`、`<GATE_DIR>/<sid>.denied_sha`)に記録し、次回は
//! その SHA からの増分だけを見る。同じ SHA への再 ExitPlanMode は無条件 allow。
//! 台帳は追記式なので「最後のレコード」を記録 SHA として読む(bash 版は上書き
//! していたが、読む値は同じ)。
//!
//! 交差判定はファイル数に上限を設けない(表示件数だけ 50 件に丸める)。
//! パターン集合の照合は bash 版の `grep -F -o -f` と同じ leftmost-longest・
//! 非重複の走査(Aho-Corasick)で 1 回だけ行う。
//!
//! 縮退: git 不在、fetch 失敗、origin/HEAD 未設定、プラン本文が取得できない
//! 等はすべて fail-open。

use aho_corasick::{AhoCorasick, MatchKind};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

/// deny メッセージに並べる交差ファイルの上限(判定には上限を設けない)。
pub const MAX_DENY_DISPLAY: usize = 50;

/// `git -C dir <args>` の stdout(末尾改行除去)。失敗は `None`。
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
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
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    while s.ends_with('\n') {
        s.pop();
    }
    Some(s)
}

/// `git -C dir <args>` が成功したか。
pub fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 変更ファイル(改行区切り、リポ相対パス)のうち、プラン本文に
/// フルパスか basename が部分文字列として現れるものを、変更ファイルの順で返す。
///
/// bash 版と同じく、パターン集合(各パスと basename)を `grep -F -o` と同じ
/// leftmost-longest・非重複で走査し、拾えたパターンだけを「現れた」とみなす
/// (長いパターンに飲まれて重なった短いパターンは拾われない)。
pub fn intersect_files(changed: &str, plan: &str) -> Vec<String> {
    let changed = lines(changed);
    if changed.is_empty() {
        return Vec::new();
    }
    let patterns: BTreeSet<&str> = changed
        .iter()
        .flat_map(|p| [*p, basename(p)])
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() {
        return Vec::new();
    }
    let patterns: Vec<&str> = patterns.into_iter().collect();
    let ac = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(&patterns)
        .expect("valid patterns");
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    // grep は行単位で走査する(パターンは改行を含まないので行を跨いだ一致は無い)。
    for line in lines(plan) {
        for m in ac.find_iter(line) {
            seen.insert(patterns[m.pattern().as_usize()]);
        }
    }
    changed
        .iter()
        .filter(|p| !p.is_empty() && (seen.contains(*p) || seen.contains(basename(p))))
        .map(|p| p.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_path_and_basename() {
        let changed = "config/foo.nix\ndeep/nested/unique-name.txt\nother.txt\n";
        assert_eq!(
            intersect_files(changed, "config/foo.nix と unique-name.txt を直す"),
            vec!["config/foo.nix", "deep/nested/unique-name.txt"]
        );
        assert!(intersect_files(changed, "無関係").is_empty());
        assert!(intersect_files("", "x").is_empty());
    }

    #[test]
    fn overlapping_short_pattern_is_swallowed_like_grep() {
        // grep -F -o は "xbar.nix" を 1 件として消費するので、重なる
        // "bar.nix"(a/bar.nix の basename)は拾われない。
        let changed = "a/bar.nix\nxbar.nix\n";
        assert_eq!(intersect_files(changed, "xbar.nix"), vec!["xbar.nix"]);
    }
}
