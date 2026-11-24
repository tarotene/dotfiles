//! 原則の層の不変条件: `config/web/principles.toml` は、ページの型の要件(`page-types.toml`)が、どの原則から
//! 起こされたかを、閉じた語彙の表で持つ(ADR-1708)。
//!
//! - 表のキーと値は閉じている。知らないキー・語彙の外の `kind`・`scope`・重複した `id` は落ちる。
//! - 原則は、出典(`source`)・原文を読んだ日(`retrieved`)・逐語の引用(`quote`)を持つ。
//!   `kind = "principle"` の `source` は https の URL。`kind = "decision"` の `source` はリポジトリ内の
//!   ファイルで、`quote` はそのファイルに、そのまま含まれる(機械で照合できる)。
//! - すべての要件は、実在する原則の ID を 1 つ以上、`principles` に持つ。
//! - どの要件にも使われない `scope = "general"` の原則は落ちる(死んだ原則を残さない)。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use toml::{Table, Value};

use crate::manifest::Violation;

/// 原則の表のパス(リポジトリの根から)。
pub const PRINCIPLES_PATH: &str = "config/web/principles.toml";
/// 要件の一覧のパス。
pub const PAGE_TYPES_PATH: &str = "config/web/page-types.toml";

/// 原則の種類(`kind`)の語彙。
const KINDS: [&str; 2] = ["principle", "decision"];
/// 原則の範囲(`scope`)の語彙。
const SCOPES: [&str; 2] = ["structure", "general"];
/// 原則が持てるキーの全体(すべて必須)。
const PRINCIPLE_KEYS: [&str; 7] = [
    "id",
    "title",
    "kind",
    "scope",
    "source",
    "retrieved",
    "quote",
];

/// 要件 1 件の、原則への結びつき。`principles` が配列でない・文字列でない要素を含むときは `None`。
pub struct Requirement {
    pub id: String,
    pub principles: Option<Vec<String>>,
}

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

fn string_of<'a>(t: &'a Table, key: &str) -> Option<&'a str> {
    match t.get(key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn is_kebab(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `YYYY-MM-DD` の形(月は 01〜12、日は 01〜31。暦の厳密な検査はしない)。
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// 要件の一覧(`page-types.toml`)から、要件の ID と、結びつく原則の ID を取る。
pub fn requirements(page_types: &str) -> Result<Vec<Requirement>, String> {
    let table: Table = page_types
        .parse()
        .map_err(|e| format!("{PAGE_TYPES_PATH} を TOML として読めない: {e}"))?;
    let Some(Value::Array(types)) = table.get("type") else {
        return Err(format!("{PAGE_TYPES_PATH} に `[[type]]` が無い"));
    };
    let mut out = Vec::new();
    for t in types {
        let Value::Table(t) = t else { continue };
        let Some(Value::Array(reqs)) = t.get("requirement") else {
            continue;
        };
        for r in reqs {
            let Value::Table(r) = r else { continue };
            let Some(id) = string_of(r, "id") else {
                continue;
            };
            let principles = match r.get("principles") {
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            };
            out.push(Requirement {
                id: id.to_string(),
                principles,
            });
        }
    }
    Ok(out)
}

/// 原則の表の文字列を、要件との結びつきに突き合わせて検査する。`read_source` は、リポジトリの根からの
/// 相対パスのファイルの中身を答える(無ければ `None`)。
pub fn check_principles(
    path: &str,
    text: &str,
    requirements: &[Requirement],
    read_source: &dyn Fn(&str) -> Option<String>,
) -> Vec<Violation> {
    let table: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return vec![violation(path, format!("TOML として読めない: {e}"))],
    };
    let mut out = Vec::new();
    for key in table.keys() {
        if key != "principle" {
            out.push(violation(
                path,
                format!("知らないキー `{key}` がある(表の先頭に書けるのは `[[principle]]` だけ)"),
            ));
        }
    }
    let Some(Value::Array(items)) = table.get("principle") else {
        out.push(violation(path, "`[[principle]]` が 1 つも無い".to_string()));
        return out;
    };
    if items.is_empty() {
        out.push(violation(path, "`[[principle]]` が 1 つも無い".to_string()));
    }

    // id → scope(使われているかの検査に使う)
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut general: BTreeSet<String> = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let Value::Table(p) = item else {
            out.push(violation(
                path,
                format!("{} 番目の `principle` が表でない", i + 1),
            ));
            continue;
        };
        let id = string_of(p, "id").unwrap_or("");
        let label = if id.is_empty() {
            format!("{} 番目の principle", i + 1)
        } else {
            format!("principle `{id}`")
        };
        let mut bad = |detail: String| out.push(violation(path, format!("{label}: {detail}")));

        for key in p.keys() {
            if !PRINCIPLE_KEYS.contains(&key.as_str()) {
                bad(format!("知らないキー `{key}` がある"));
            }
        }
        for key in PRINCIPLE_KEYS {
            if string_of(p, key).is_none_or(|s| s.trim().is_empty()) {
                bad(format!("`{key}` が無い、または空(空でない文字列で書く)"));
            }
        }
        if !id.is_empty() {
            if !is_kebab(id) {
                bad("`id` は小文字・数字・`-` だけで、小文字で始める".to_string());
            }
            if !ids.insert(id.to_string()) {
                bad("`id` が重複している".to_string());
            }
        }
        let kind = string_of(p, "kind").unwrap_or("");
        if !kind.is_empty() && !KINDS.contains(&kind) {
            bad(format!(
                "`kind = \"{kind}\"` は語彙の外(使えるのは {KINDS:?})"
            ));
        }
        let scope = string_of(p, "scope").unwrap_or("");
        if !scope.is_empty() && !SCOPES.contains(&scope) {
            bad(format!(
                "`scope = \"{scope}\"` は語彙の外(使えるのは {SCOPES:?})"
            ));
        }
        if scope == "general" && !id.is_empty() {
            general.insert(id.to_string());
        }
        if let Some(date) = string_of(p, "retrieved") {
            if !date.is_empty() && !is_date(date) {
                bad(format!("`retrieved = \"{date}\"` は YYYY-MM-DD の形でない"));
            }
        }
        let source = string_of(p, "source").unwrap_or("");
        let quote = string_of(p, "quote").unwrap_or("");
        match kind {
            "principle" if !source.is_empty() && !source.starts_with("https://") => {
                bad(format!(
                    "`kind = \"principle\"` の `source` は https の URL で書く(`{source}`)"
                ));
            }
            "decision" if !source.is_empty() => match read_source(source) {
                None => bad(format!(
                    "`kind = \"decision\"` の `source` `{source}` が、リポジトリ内に実在しない"
                )),
                Some(body) if !quote.is_empty() && !body.contains(quote) => bad(format!(
                    "`quote` が `{source}` に、そのまま含まれない(逐語で書く)"
                )),
                Some(_) => {}
            },
            _ => {}
        }
    }

    // 要件 → 原則
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for r in requirements {
        match &r.principles {
            Some(ps) if !ps.is_empty() => {
                for p in ps {
                    if ids.contains(p) {
                        used.insert(p.as_str());
                    } else {
                        out.push(violation(
                            PAGE_TYPES_PATH,
                            format!("要件 `{}`: `principles` の `{p}` が `{path}` に実在しない", r.id),
                        ));
                    }
                }
            }
            _ => out.push(violation(
                PAGE_TYPES_PATH,
                format!(
                    "要件 `{}`: `principles` が無い(原則の ID の、空でない配列で書く。どの原則にも結びつかない要件は置けない)",
                    r.id
                ),
            )),
        }
    }
    for id in &general {
        if !used.contains(id.as_str()) {
            out.push(violation(
                path,
                format!("principle `{id}`: どの要件にも使われていない(`scope = \"general\"` の原則は、要件が指す)"),
            ));
        }
    }
    out
}

/// ツリーの原則の表を、要件の一覧と、決定の出典のファイルに突き合わせて検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let text = read(PRINCIPLES_PATH)?;
    let reqs = requirements(&read(PAGE_TYPES_PATH)?)?;
    Ok(check_principles(PRINCIPLES_PATH, &text, &reqs, &|rel| {
        fs::read_to_string(root.join(rel)).ok()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[[principle]]
id = "wcag-1-4-1"
title = "色の使用"
kind = "principle"
scope = "general"
source = "https://www.w3.org/TR/WCAG22/#use-of-color"
retrieved = "2026-11-25"
quote = "Color is not used as the only visual means"

[[principle]]
id = "adr-x"
title = "決定"
kind = "decision"
scope = "structure"
source = "docs/adr/x.md"
retrieved = "2026-11-25"
quote = "禁止する"
"#;

    const PAGE_TYPES: &str = r#"
[[type]]
id = "common"
title = "共通"

[[type.requirement]]
id = "C5"
text = "色だけで伝えない"
principles = ["wcag-1-4-1", "adr-x"]
check = "visual"
"#;

    fn reqs(text: &str) -> Vec<Requirement> {
        requirements(text).expect("page-types")
    }

    fn source(rel: &str) -> Option<String> {
        (rel == "docs/adr/x.md").then(|| "第三者への読み込みを禁止する。".to_string())
    }

    fn details(principles: &str, page_types: &str) -> Vec<String> {
        check_principles(PRINCIPLES_PATH, principles, &reqs(page_types), &source)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn assert_fails(principles: &str, page_types: &str, needle: &str) {
        let d = details(principles, page_types);
        assert!(
            d.iter().any(|m| m.contains(needle)),
            "「{needle}」を含む違反が出るはず: {d:?}"
        );
    }

    #[test]
    fn a_consistent_table_passes() {
        assert_eq!(details(GOOD, PAGE_TYPES), Vec::<String>::new());
    }

    #[test]
    fn unknown_keys_and_vocabulary_are_rejected() {
        assert_fails(
            &format!("bogus = 1\n{GOOD}"),
            PAGE_TYPES,
            "知らないキー `bogus`",
        );
        assert_fails(
            &GOOD.replace("kind = \"principle\"", "kind = \"opinion\""),
            PAGE_TYPES,
            "`kind = \"opinion\"` は語彙の外",
        );
        assert_fails(
            &GOOD.replace("scope = \"general\"", "scope = \"misc\""),
            PAGE_TYPES,
            "`scope = \"misc\"` は語彙の外",
        );
        assert_fails(
            &GOOD.replace("title = \"色の使用\"", "title = \"色の使用\"\nextra = 1"),
            PAGE_TYPES,
            "知らないキー `extra`",
        );
    }

    #[test]
    fn every_key_is_required_and_ids_are_unique() {
        assert_fails(
            &GOOD.replace(
                "quote = \"Color is not used as the only visual means\"\n",
                "",
            ),
            PAGE_TYPES,
            "`quote` が無い",
        );
        assert_fails(
            &GOOD.replace("id = \"adr-x\"", "id = \"wcag-1-4-1\""),
            PAGE_TYPES,
            "`id` が重複している",
        );
        assert_fails(
            &GOOD.replace("id = \"adr-x\"", "id = \"ADR_X\""),
            PAGE_TYPES,
            "`id` は小文字・数字・`-` だけ",
        );
        assert_fails(
            &GOOD.replace("retrieved = \"2026-11-25\"", "retrieved = \"25 Nov 2026\""),
            PAGE_TYPES,
            "YYYY-MM-DD の形でない",
        );
    }

    #[test]
    fn sources_are_checked_by_kind() {
        assert_fails(
            &GOOD.replace("https://www.w3.org/TR/WCAG22/#use-of-color", "docs/x.md"),
            PAGE_TYPES,
            "https の URL",
        );
        assert_fails(
            &GOOD.replace("docs/adr/x.md", "docs/adr/missing.md"),
            PAGE_TYPES,
            "リポジトリ内に実在しない",
        );
        assert_fails(
            &GOOD.replace("quote = \"禁止する\"", "quote = \"許可する\""),
            PAGE_TYPES,
            "そのまま含まれない",
        );
    }

    #[test]
    fn requirements_must_point_at_existing_principles() {
        assert_fails(
            GOOD,
            &PAGE_TYPES.replace("[\"wcag-1-4-1\", \"adr-x\"]", "[\"wcag-1-4-1\", \"nope\"]"),
            "`nope` が",
        );
        assert_fails(
            GOOD,
            &PAGE_TYPES.replace("principles = [\"wcag-1-4-1\", \"adr-x\"]\n", ""),
            "要件 `C5`: `principles` が無い",
        );
        assert_fails(
            GOOD,
            &PAGE_TYPES.replace("[\"wcag-1-4-1\", \"adr-x\"]", "[]"),
            "要件 `C5`: `principles` が無い",
        );
        assert_fails(
            GOOD,
            &PAGE_TYPES.replace("[\"wcag-1-4-1\", \"adr-x\"]", "[1]"),
            "要件 `C5`: `principles` が無い",
        );
    }

    #[test]
    fn a_general_principle_that_no_requirement_uses_is_dead() {
        assert_fails(
            GOOD,
            &PAGE_TYPES.replace("[\"wcag-1-4-1\", \"adr-x\"]", "[\"adr-x\"]"),
            "principle `wcag-1-4-1`: どの要件にも使われていない",
        );
        // `structure` の原則は、構造パターン(別の表)が使うので、要件に使われなくてよい。
        let only_structure = PAGE_TYPES.replace("[\"wcag-1-4-1\", \"adr-x\"]", "[\"wcag-1-4-1\"]");
        assert_eq!(details(GOOD, &only_structure), Vec::<String>::new());
    }

    #[test]
    fn an_empty_or_unreadable_table_is_rejected() {
        assert_fails("", PAGE_TYPES, "`[[principle]]` が 1 つも無い");
        assert_fails("not = [toml", PAGE_TYPES, "TOML として読めない");
    }
}
