//! 触れた決定の一覧(`docs/agents/agent-mode.md` の第 2 段の入力)。
//!
//! 決定の記録を読まずに整理したセッション(agent-mode)の PR に、「どの決定の執行点に触れたか」を機械的に残す。
//! 既存の検査(`check::run_check`)は、足された ADR の執行点が実在するかだけを見て、**旧 ADR の執行点が消えた・
//! 変わったこと**は、どの検査も見ない。この一覧がその穴を埋め、通常のセッションが、覆す決定ごとに新しい ADR
//! (見出しの `- Supersedes:` で宣言する。ADR-0008 の Amendment)を同じブランチへ足す入力になる。
//!
//! 一覧は 2 つ:
//! 1. **触れた決定**: base 時点の `docs/adr/` の各 ADR の執行点(`## 執行点` と、Amendment の `### 執行点`)のうち、
//!    base..HEAD で変更・削除・改名(移動元)されたもの。ADR は base 時点の本文を読む(この PR が ADR を書き換えても、
//!    変わる前の決定を見るため)。
//! 2. **消した・変えた検査**: テストと CI の設定のうち、削除・変更されたもの。ADR を持たないリポジトリでは、こちらだけが
//!    手がかりになる([`is_check_path`])。

use std::path::Path;

use crate::check::{
    extract_paths, is_adr_exec_heading, is_amendment_exec_heading, is_any_heading, is_new_adr_path,
};

/// 変わり方。追加(`A`)は、触れた決定ではないので持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Deleted,
    /// 改名・移動(移動元のパス)。
    Renamed,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Modified => "変更",
            Kind::Deleted => "削除",
            Kind::Renamed => "改名・移動",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: Kind,
    pub path: String,
}

/// `git diff --name-status -M` の出力を読む。追加・コピーは読み飛ばし、改名は移動元を `Renamed` で返す。
pub fn parse_name_status(text: &str) -> Vec<Change> {
    text.lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let status = cols.next()?;
            let first = cols.next()?;
            let kind = match status.chars().next()? {
                'M' | 'T' => Kind::Modified,
                'D' => Kind::Deleted,
                'R' => Kind::Renamed,
                _ => return None,
            };
            Some(Change {
                kind,
                path: first.to_string(),
            })
        })
        .collect()
}

/// ADR の本文から、執行点のパスを集める(`## 執行点` と `### 執行点` の箇条書き。重複は 1 つにする)。
pub fn exec_points(adr_text: &str) -> Vec<String> {
    let mut body = String::new();
    let mut collecting = false;
    for line in adr_text.split_terminator('\n') {
        if is_adr_exec_heading(line) || is_amendment_exec_heading(line) {
            collecting = true;
            continue;
        }
        if collecting && is_any_heading(line) {
            collecting = false;
        }
        if collecting {
            body.push_str(line);
            body.push('\n');
        }
    }
    let mut paths = extract_paths(&body);
    let mut seen = std::collections::BTreeSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    paths
}

/// 1 つの決定(ADR)と、そのうち触れられた執行点。
#[derive(Debug, PartialEq, Eq)]
pub struct Touch {
    pub adr: String,
    pub hits: Vec<Change>,
}

/// `adrs`: `(ADR のパス, base 時点の本文)`。触れた執行点を持つ ADR だけを、パスの順で返す。
pub fn touched_adrs(adrs: &[(String, String)], changes: &[Change]) -> Vec<Touch> {
    let mut out: Vec<Touch> = adrs
        .iter()
        .filter_map(|(adr, text)| {
            let points = exec_points(text);
            let hits: Vec<Change> = changes
                .iter()
                .filter(|c| points.contains(&c.path))
                .cloned()
                .collect();
            (!hits.is_empty()).then(|| Touch {
                adr: adr.clone(),
                hits,
            })
        })
        .collect();
    out.sort_by(|a, b| a.adr.cmp(&b.adr));
    out
}

/// 検査(テスト・CI の設定)のパスか。閉じた規則: `.github/workflows/` 配下、`tests`・`test`・`__tests__` の
/// ディレクトリ配下、ファイル名が `test_*`・`*_test.*`・`*.test.*`・`*.spec.*`。
pub fn is_check_path(path: &str) -> bool {
    if path.starts_with(".github/workflows/") {
        return true;
    }
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or("");
    if parts
        .iter()
        .any(|c| matches!(*c, "tests" | "test" | "__tests__"))
    {
        return true;
    }
    name.starts_with("test_")
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// 削除・変更・改名された検査。
pub fn check_changes(changes: &[Change]) -> Vec<Change> {
    changes
        .iter()
        .filter(|c| is_check_path(&c.path))
        .cloned()
        .collect()
}

/// PR 本文の `## 触れた決定` の節を組む。
pub fn render(base: &str, touches: &[Touch], checks: &[Change]) -> String {
    let mut out = String::from("## 触れた決定\n\n");
    out.push_str(&format!(
        "ADR の `## 執行点`(変更前の本文)のうち、この変更(`{base}..HEAD`)で変わった・消えたもの:\n\n"
    ));
    if touches.is_empty() {
        out.push_str("- なし\n");
    }
    for t in touches {
        out.push_str(&format!("- {}\n", t.adr));
        for h in &t.hits {
            out.push_str(&format!("  - {} ({})\n", h.path, h.kind.label()));
        }
    }
    out.push_str("\n消した・変えた検査(テスト・CI の設定):\n\n");
    if checks.is_empty() {
        out.push_str("- なし\n");
    }
    for c in checks {
        out.push_str(&format!("- {} ({})\n", c.path, c.kind.label()));
    }
    out
}

/// base 時点の `docs/adr/` の ADR(パスと本文)。読めない・無いときは空。
fn adrs_at(repo: &Path, base: &str) -> Vec<(String, String)> {
    let listing = git_repository::out(
        repo,
        &["ls-tree", "-r", "--name-only", base, "--", "docs/adr"],
    )
    .unwrap_or_default();
    listing
        .lines()
        .filter(|p| is_new_adr_path(p))
        .filter_map(|p| {
            let spec = format!("{base}:{p}");
            let text = git_repository::out(repo, &["show", &spec])?;
            Some((p.to_string(), text))
        })
        .collect()
}

/// `base..HEAD` の触れた決定の節(Markdown)。`base` が解決できないときは `Err`(黙って空にしない)。
pub fn run(repo: &Path, base: &str) -> Result<String, String> {
    let raw = git_repository::raw(
        repo,
        &["diff", "--name-status", "-M", base, "HEAD", "--", "."],
    )
    .ok_or_else(|| format!("`{base}` と HEAD の差を取れません(base を解決できない)"))?;
    let changes = parse_name_status(&String::from_utf8_lossy(&raw));
    let touches = touched_adrs(&adrs_at(repo, base), &changes);
    Ok(render(base, &touches, &check_changes(&changes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn change(kind: Kind, path: &str) -> Change {
        Change {
            kind,
            path: path.into(),
        }
    }

    #[test]
    fn name_status_keeps_modified_deleted_renamed_and_skips_added() {
        let text = "M\tsrc/a.rs\nD\tsrc/b.rs\nA\tsrc/new.rs\nR100\tsrc/old.rs\tsrc/moved.rs\nT\tsrc/t\nC90\tx\ty\n";
        assert_eq!(
            parse_name_status(text),
            vec![
                change(Kind::Modified, "src/a.rs"),
                change(Kind::Deleted, "src/b.rs"),
                change(Kind::Renamed, "src/old.rs"),
                change(Kind::Modified, "src/t"),
            ]
        );
    }

    #[test]
    fn exec_points_read_the_adr_section_and_amendment_sections_once() {
        let text = "# ADR-1 — x\n\n## Decision\n\n- not/a/point.rs\n\n## 執行点\n\n- crates/a/src/lib.rs\n- `config/b.toml` — 説明\n\n## Amendment (2026-01-01)\n\n### 執行点\n\n- crates/a/src/lib.rs\n- crates/c/src/main.rs\n\n## 他\n\n- after.rs\n";
        assert_eq!(
            exec_points(text),
            vec![
                "crates/a/src/lib.rs",
                "config/b.toml",
                "crates/c/src/main.rs"
            ]
        );
    }

    #[test]
    fn only_adrs_whose_execution_points_changed_are_listed_in_path_order() {
        let adrs = vec![
            (
                "docs/adr/0200-b.md".to_string(),
                "## 執行点\n\n- src/a.rs\n".to_string(),
            ),
            (
                "docs/adr/0100-a.md".to_string(),
                "## 執行点\n\n- src/a.rs\n- src/b.rs\n".to_string(),
            ),
            (
                "docs/adr/0300-c.md".to_string(),
                "## 執行点\n\n- src/untouched.rs\n".to_string(),
            ),
        ];
        let changes = vec![
            change(Kind::Modified, "src/a.rs"),
            change(Kind::Deleted, "src/b.rs"),
        ];
        let got = touched_adrs(&adrs, &changes);
        assert_eq!(
            got.iter().map(|t| t.adr.as_str()).collect::<Vec<_>>(),
            ["docs/adr/0100-a.md", "docs/adr/0200-b.md"]
        );
        assert_eq!(got[0].hits.len(), 2);
        assert_eq!(got[1].hits, vec![change(Kind::Modified, "src/a.rs")]);
    }

    #[test]
    fn check_paths_follow_the_closed_rule() {
        for yes in [
            ".github/workflows/ci.yml",
            "crates/x/tests/cli.rs",
            "tests/a.py",
            "src/__tests__/a.ts",
            "pkg/test_util.py",
            "pkg/util_test.go",
            "web/a.test.ts",
            "web/a.spec.ts",
        ] {
            assert!(is_check_path(yes), "{yes}");
        }
        for no in [
            "src/lib.rs",
            "docs/testing.md",
            "src/latest.rs",
            "README.md",
        ] {
            assert!(!is_check_path(no), "{no}");
        }
    }

    #[test]
    fn render_says_none_for_empty_sections_and_lists_hits_under_their_adr() {
        let empty = render("origin/main", &[], &[]);
        assert_eq!(empty.matches("- なし").count(), 2, "{empty}");
        let t = vec![Touch {
            adr: "docs/adr/0100-a.md".into(),
            hits: vec![change(Kind::Deleted, "src/b.rs")],
        }];
        let c = vec![change(Kind::Modified, ".github/workflows/ci.yml")];
        let s = render("origin/main", &t, &c);
        assert!(s.starts_with("## 触れた決定\n"), "{s}");
        assert!(
            s.contains("- docs/adr/0100-a.md\n  - src/b.rs (削除)\n"),
            "{s}"
        );
        assert!(s.contains("- .github/workflows/ci.yml (変更)\n"), "{s}");
    }

    fn git(repo: &Path, args: &[&str]) {
        let st = Command::new("git")
            .current_dir(repo)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn run_reads_the_adr_as_of_base_and_reports_deleted_and_changed_points() {
        let d = tempfile::tempdir().unwrap();
        let repo = d.path();
        git(repo, &["init", "-q"]);
        std::fs::create_dir_all(repo.join("docs/adr")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("tests")).unwrap();
        std::fs::write(
            repo.join("docs/adr/0100-x.md"),
            "# ADR-0100 — x\n\n## 執行点\n\n- src/a.rs\n- src/b.rs\n",
        )
        .unwrap();
        for f in ["src/a.rs", "src/b.rs", "tests/t.rs"] {
            std::fs::write(repo.join(f), "// v1\n").unwrap();
        }
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "base"]);
        // 第 1 段の変更: a を変え、b とテストを消し、ADR 自身も書き換える(base 時点の本文が読まれること)。
        std::fs::write(repo.join("src/a.rs"), "// v2\n").unwrap();
        std::fs::remove_file(repo.join("src/b.rs")).unwrap();
        std::fs::remove_file(repo.join("tests/t.rs")).unwrap();
        std::fs::write(
            repo.join("docs/adr/0100-x.md"),
            "# ADR-0100 — x\n\n## 執行点\n\n",
        )
        .unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "refactor"]);

        let s = run(repo, "HEAD~1").unwrap();
        assert!(
            s.contains("- docs/adr/0100-x.md\n  - src/a.rs (変更)\n  - src/b.rs (削除)\n"),
            "{s}"
        );
        assert!(s.contains("- tests/t.rs (削除)\n"), "{s}");
    }

    #[test]
    fn an_unresolvable_base_is_an_error_not_an_empty_list() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        assert!(run(d.path(), "no-such-ref").is_err());
    }
}
