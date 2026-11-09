//! 暦の計算の不変条件: unix 秒と暦日の変換は `civil-time` が持つ。
//!
//! Howard Hinnant の `days_from_civil` / `civil_from_days` の定数(`719468` と `146097`)が、8 つの crate に手で
//! 複写されていた(月日の型が `i64` と `u32` で違う複写も混ざり、`agent-session` の複写は 1 つだけ pub だった)。
//! この 2 つの定数を、`civil-time` の外の `crates/*/src` に書かない。例外は無い。
//!
//! 検査は、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を読まない。テストの
//! `tests/` の独立な実装(暦の逆変換を手で書いて突き合わせる)は、対象にしない。

use std::path::Path;

use crate::hook_io::{code_lines, read_sources, Source};
use crate::manifest::Violation;

/// 暦の変換の定数(数字の区切り `_` を除いた形)。
const CONSTANTS: [&str; 2] = ["719468", "146097"];

fn violation(path: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: path.to_string(),
        detail: detail.into(),
    }
}

/// 持ち主(`civil-time`)と、この検査自身(定数を持つ)は、検査しない。
fn is_owner(path: &str) -> bool {
    path.starts_with("crates/civil-time/") || path.starts_with("crates/repo-structure-check/")
}

/// 行の中の数のかたまり(数字と `_` の連なり)が、定数と一致するか。`1_460_970` のような別の数には当たらない。
fn has_constant(line: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_digit() || c == '_'))
        .map(|t| t.replace('_', ""))
        .any(|t| CONSTANTS.contains(&t.as_str()))
}

fn constant_hit(text: &str) -> Option<usize> {
    code_lines(text)
        .find(|(_, l)| has_constant(l))
        .map(|(n, _)| n)
}

/// 暦の変換の定数が、`civil-time` のほかに無いことを検査する。
pub fn check_constants(sources: &[Source]) -> Vec<Violation> {
    sources
        .iter()
        .filter(|s| !is_owner(&s.path))
        .filter_map(|s| {
            constant_hit(&s.text).map(|n| {
                violation(
                    &s.path,
                    format!(
                        "{n} 行目に暦の変換の定数(719468 か 146097)がある。unix 秒と暦日の変換は civil_time を呼ぶ"
                    ),
                )
            })
        })
        .collect()
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    Ok(check_constants(&read_sources(root)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(path: &str, text: &str) -> Source {
        Source {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn civil_time_may_hold_the_constants() {
        let v = vec![src(
            "crates/civil-time/src/lib.rs",
            "era * 146_097 + doe - 719_468\n",
        )];
        assert_eq!(check_constants(&v), vec![]);
    }

    #[test]
    fn comments_and_the_test_section_are_not_read() {
        let text = "// 719_468 は 1970-01-01 の補正\n\
                    fn f() {}\n\
                    #[cfg(test)]\n\
                    mod tests { const Z: i64 = 719_468; }\n";
        assert_eq!(check_constants(&[src("crates/x/src/lib.rs", text)]), vec![]);
    }

    #[test]
    fn a_copy_is_a_violation_in_every_spelling() {
        for text in [
            "let z = days + 719_468;\n",
            "let z = days + 719468;\n",
            "era * 146_097 + doe\n",
            "era * 146097 + doe\n",
        ] {
            let got = check_constants(&[src("crates/x/src/lib.rs", text)]);
            assert_eq!(got.len(), 1, "{text}: {got:?}");
            assert!(got[0].detail.contains("1 行目"), "{got:?}");
        }
    }

    #[test]
    fn a_longer_number_that_merely_contains_a_constant_is_not_a_violation() {
        for text in [
            "let n = 1_460_970;\n",
            "let n = 71_946_800;\n",
            "let id = 3146097;\n",
        ] {
            let got = check_constants(&[src("crates/x/src/lib.rs", text)]);
            assert_eq!(got, vec![], "{text}");
        }
    }
}
