//! overrides.tsv と閉語彙 .tsv の読み込み(bash 版 read_overrides /
//! read_tsv_col1 / read_closed_set_json / is_exempt)。
//!
//! どれも「ファイルが無い = 空」(エラーにしない)。コメント行・空行
//! (`grep -Ev '^[[:space:]]*(#|$)'`)は落とす。

use crate::jq;
use crate::model::Domain;
use std::path::Path;

fn data_lines(path: &Path) -> Vec<String> {
    if !path.is_file() {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .split('\n')
        .filter(|l| {
            // 末尾改行の後ろの空文字列もここで落ちる(空行扱い)。
            let t = l.trim_start_matches(jq::is_space);
            !(t.is_empty() || t.starts_with('#'))
        })
        .map(str::to_string)
        .collect()
}

/// overrides.tsv の 1 行(`<repo>\t<domain-or-*>\t<directive>`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub repo: String,
    pub domain: String,
    pub directive: String,
}

/// bash: `read_overrides`。
pub fn read_overrides(path: &Path) -> Vec<Override> {
    data_lines(path)
        .into_iter()
        .map(|l| {
            let mut f = l.split('\t');
            Override {
                repo: f.next().unwrap_or("").to_string(),
                domain: f.next().unwrap_or("").to_string(),
                directive: f.next().unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// bash: `read_tsv_col1`(1 列目、タブが無ければ行全体)。
pub fn read_tsv_col1(path: &Path) -> Vec<String> {
    data_lines(path)
        .into_iter()
        .map(|l| l.split('\t').next().unwrap_or("").to_string())
        .collect()
}

/// bash: `read_closed_set_json`。PUBLIC 版と .local.tsv(PRIVATE)の和集合を
/// `LC_ALL=C sort -u` して空要素を落とす。
pub fn read_closed_set(repo_file: &Path, local_file: Option<&Path>) -> Vec<String> {
    let mut v = read_tsv_col1(repo_file);
    if let Some(l) = local_file {
        v.extend(read_tsv_col1(l));
    }
    jq::unique_strings(v.into_iter().filter(|s| !s.is_empty()).collect())
}

/// bash: `is_exempt`。`.github` は naming / charters から恒久除外(GitHub の
/// 予約名で、宣言しうるクラスが無い)。
pub fn is_exempt(repo: &str, domain: Domain, overrides: &[Override]) -> bool {
    if repo == ".github" && matches!(domain, Domain::Naming | Domain::Charters) {
        return true;
    }
    overrides.iter().any(|o| {
        o.repo == repo
            && (o.domain == domain.as_str() || o.domain == "*")
            && o.directive == "exempt"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_set_union_sorted() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.tsv");
        let b = d.path().join("b.tsv");
        std::fs::write(&a, "# c\nzeta\tx\n\n  \nalpha\n").unwrap();
        std::fs::write(&b, "alpha\nbeta\t1\n").unwrap();
        assert_eq!(read_closed_set(&a, Some(&b)), vec!["alpha", "beta", "zeta"]);
        assert!(read_closed_set(&d.path().join("none"), None).is_empty());
    }

    #[test]
    fn exempt_rules() {
        let o = vec![Override {
            repo: "x".into(),
            domain: "*".into(),
            directive: "exempt".into(),
        }];
        assert!(is_exempt("x", Domain::Settings, &o));
        assert!(!is_exempt("y", Domain::Settings, &o));
        assert!(is_exempt(".github", Domain::Naming, &[]));
        assert!(!is_exempt(".github", Domain::Settings, &[]));
    }
}
