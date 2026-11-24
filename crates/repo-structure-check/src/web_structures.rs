//! 構造の規則の不変条件: `config/web/structures.toml` は、課題の性質の軸と、構造パターン(軸の値から候補を出す)を、
//! 閉じた語彙で持つ。リポジトリ内のすべての HTML は、自分の構造と軸を宣言する(ADR-1711)。
//!
//! 表(`structures.toml`):
//! - キーと値は閉じている。軸の値・パターンの `fits`・`measure`・`principles` は、語彙と実在に突き合わせる。
//! - `default = true` のパターンは、高々 1 つ。
//! - `scope = "structure"` の原則は、どれかのパターンか、どれかの要件が指す(死んだ原則を残さない)。
//!
//! 各 HTML(`<head>` の `<meta>`):
//! - `page-structure` を宣言する(語彙にある構造)。無ければ落ちる。ページを足すとき、宣言を忘れられない。
//! - `page-axes`(`軸=値;軸=値`)は、語彙にある軸と値だけ。宣言した構造の `fits` に挙げた軸は、すべて宣言する。
//! - 軸の値が、宣言した構造の `fits` の外にあるとき、または、既定の構造に合うのに別の構造を選ぶときは、
//!   `page-structure-deviation` に理由を書く。
//! - 属性は、`name="..."`・`content="..."` の二重引用符の形だけを読む。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use toml::{Table, Value};

use crate::manifest::Violation;
use crate::web_assets;

/// 構造の表のパス(リポジトリの根から)。
pub const STRUCTURES_PATH: &str = "config/web/structures.toml";
/// 原則の表のパス。
pub const PRINCIPLES_PATH: &str = "config/web/principles.toml";
/// 要件の一覧のパス。
pub const PAGE_TYPES_PATH: &str = "config/web/page-types.toml";

const PATTERN_KEYS: [&str; 7] = [
    "id",
    "title",
    "default",
    "measure",
    "fits",
    "principles",
    "note",
];
const MEASURES: [&str; 2] = ["viewport-fit", "none"];

/// 検査を通った構造の表。
#[derive(Debug, Default)]
pub struct Structures {
    /// 軸の id → 許す値。
    pub axes: BTreeMap<String, Vec<String>>,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug)]
pub struct Pattern {
    pub id: String,
    pub default: bool,
    /// 軸の id → このパターンが許す値(空の表 = 何でも許す)。
    pub fits: BTreeMap<String, Vec<String>>,
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

fn string_array(v: &Value) -> Option<Vec<String>> {
    let Value::Array(items) = v else { return None };
    items
        .iter()
        .map(|x| match x {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// 原則の表から、原則の id → scope を取る(形の検査は `web_principles` が行う)。
pub fn principle_scopes(principles: &str) -> Result<BTreeMap<String, String>, String> {
    let table: Table = principles
        .parse()
        .map_err(|e| format!("{PRINCIPLES_PATH} を TOML として読めない: {e}"))?;
    let mut out = BTreeMap::new();
    if let Some(Value::Array(items)) = table.get("principle") {
        for p in items {
            if let Value::Table(p) = p {
                if let (Some(id), Some(scope)) = (string_of(p, "id"), string_of(p, "scope")) {
                    out.insert(id.to_string(), scope.to_string());
                }
            }
        }
    }
    Ok(out)
}

/// 要件が指す原則の id の集合。
pub fn requirement_principles(page_types: &str) -> Result<BTreeSet<String>, String> {
    Ok(crate::web_principles::requirements(page_types)?
        .into_iter()
        .filter_map(|r| r.principles)
        .flatten()
        .collect())
}

/// 構造の表を検査する。表が読めて語彙が定まれば、`Structures` も返す(ページの宣言の検査に使う)。
pub fn check_structures(
    path: &str,
    text: &str,
    principles: &BTreeMap<String, String>,
    used_by_requirements: &BTreeSet<String>,
) -> (Option<Structures>, Vec<Violation>) {
    let table: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            return (
                None,
                vec![violation(path, format!("TOML として読めない: {e}"))],
            )
        }
    };
    let mut out = Vec::new();
    for key in table.keys() {
        if key != "axis" && key != "pattern" {
            out.push(violation(
                path,
                format!("知らないキー `{key}` がある(表の先頭に書けるのは `[[axis]]` と `[[pattern]]` だけ)"),
            ));
        }
    }
    let mut result = Structures::default();

    // 軸
    match table.get("axis") {
        Some(Value::Array(axes)) if !axes.is_empty() => {
            for (i, a) in axes.iter().enumerate() {
                let Value::Table(a) = a else {
                    out.push(violation(
                        path,
                        format!("{} 番目の `axis` が表でない", i + 1),
                    ));
                    continue;
                };
                let id = string_of(a, "id").unwrap_or("");
                let label = if id.is_empty() {
                    format!("{} 番目の axis", i + 1)
                } else {
                    format!("axis `{id}`")
                };
                let mut bad = |d: String| out.push(violation(path, format!("{label}: {d}")));
                for key in a.keys() {
                    if !["id", "title", "values"].contains(&key.as_str()) {
                        bad(format!("知らないキー `{key}` がある"));
                    }
                }
                if !is_kebab(id) {
                    bad("`id` は小文字・数字・`-` だけで、小文字で始める".to_string());
                }
                if string_of(a, "title").is_none_or(|t| t.trim().is_empty()) {
                    bad("`title` が無い、または空".to_string());
                }
                let values = a.get("values").and_then(string_array).unwrap_or_default();
                if values.len() < 2 {
                    bad("`values` は、2 つ以上の文字列の配列で書く".to_string());
                }
                let mut seen = BTreeSet::new();
                for v in &values {
                    if !is_kebab(v) {
                        bad(format!(
                            "値 `{v}` は小文字・数字・`-` だけで、小文字で始める"
                        ));
                    }
                    if !seen.insert(v.clone()) {
                        bad(format!("値 `{v}` が重複している"));
                    }
                }
                if !id.is_empty() && result.axes.insert(id.to_string(), values).is_some() {
                    bad("`id` が重複している".to_string());
                }
            }
        }
        _ => out.push(violation(path, "`[[axis]]` が 1 つも無い".to_string())),
    }

    // パターン
    let mut used_principles: BTreeSet<String> = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut defaults = 0;
    match table.get("pattern") {
        Some(Value::Array(patterns)) if !patterns.is_empty() => {
            for (i, p) in patterns.iter().enumerate() {
                let Value::Table(p) = p else {
                    out.push(violation(
                        path,
                        format!("{} 番目の `pattern` が表でない", i + 1),
                    ));
                    continue;
                };
                let id = string_of(p, "id").unwrap_or("");
                let label = if id.is_empty() {
                    format!("{} 番目の pattern", i + 1)
                } else {
                    format!("pattern `{id}`")
                };
                let mut bad = |d: String| out.push(violation(path, format!("{label}: {d}")));
                for key in p.keys() {
                    if !PATTERN_KEYS.contains(&key.as_str()) {
                        bad(format!("知らないキー `{key}` がある"));
                    }
                }
                if !is_kebab(id) {
                    bad("`id` は小文字・数字・`-` だけで、小文字で始める".to_string());
                } else if !ids.insert(id.to_string()) {
                    bad("`id` が重複している".to_string());
                }
                if string_of(p, "title").is_none_or(|t| t.trim().is_empty()) {
                    bad("`title` が無い、または空".to_string());
                }
                if let Some(m) = p.get("measure") {
                    match m {
                        Value::String(s) if MEASURES.contains(&s.as_str()) => {}
                        _ => bad(format!("`measure` は {MEASURES:?} のどれか")),
                    }
                }
                let default = match p.get("default") {
                    None => false,
                    Some(Value::Boolean(b)) => *b,
                    Some(_) => {
                        bad("`default` は真偽値で書く".to_string());
                        false
                    }
                };
                if default {
                    defaults += 1;
                }
                let mut fits = BTreeMap::new();
                match p.get("fits") {
                    Some(Value::Table(f)) => {
                        for (axis, v) in f {
                            let Some(allowed) = result.axes.get(axis) else {
                                bad(format!("`fits` の軸 `{axis}` が、`[[axis]]` に無い"));
                                continue;
                            };
                            match string_array(v) {
                                Some(vals) if !vals.is_empty() => {
                                    for x in &vals {
                                        if !allowed.contains(x) {
                                            bad(format!("`fits` の `{axis}` の値 `{x}` が、軸の語彙 {allowed:?} に無い"));
                                        }
                                    }
                                    fits.insert(axis.clone(), vals);
                                }
                                _ => bad(format!(
                                    "`fits` の `{axis}` は、空でない文字列の配列で書く"
                                )),
                            }
                        }
                    }
                    _ => bad("`fits` が無い(何でも許すときは空の表 `{}` で書く)".to_string()),
                }
                if let Some(v) = p.get("principles") {
                    match string_array(v) {
                        Some(ps) => {
                            for x in ps {
                                if principles.contains_key(&x) {
                                    used_principles.insert(x);
                                } else {
                                    bad(format!(
                                        "`principles` の `{x}` が `{PRINCIPLES_PATH}` に実在しない"
                                    ));
                                }
                            }
                        }
                        None => bad("`principles` は、文字列の配列で書く".to_string()),
                    }
                }
                if !id.is_empty() {
                    result.patterns.push(Pattern {
                        id: id.to_string(),
                        default,
                        fits,
                    });
                }
            }
        }
        _ => out.push(violation(path, "`[[pattern]]` が 1 つも無い".to_string())),
    }
    if defaults > 1 {
        out.push(violation(
            path,
            "`default = true` のパターンは、高々 1 つ".to_string(),
        ));
    }

    // 構造に効く原則は、どれかが指す
    for (id, scope) in principles {
        if scope == "structure"
            && !used_principles.contains(id)
            && !used_by_requirements.contains(id)
        {
            out.push(violation(
                path,
                format!("principle `{id}`(`scope = \"structure\"`): どのパターンも、どの要件も指していない"),
            ));
        }
    }
    (Some(result), out)
}

/// `<meta name="<name>" content="...">` の `content` を取る。
fn meta_content(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(html.len(), |j| start + j);
        let tag = &html[start..end];
        if attr(tag, "name").as_deref() == Some(name) {
            return attr(tag, "content");
        }
        from = end.max(start + 5);
    }
    None
}

/// タグの文字列から、二重引用符の属性の値を取る(前に空白がある属性名だけ)。
fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&needle) {
        let at = from + i;
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let vstart = at + needle.len();
        if before_ok {
            let vend = tag[vstart..].find('"')? + vstart;
            return Some(tag[vstart..vend].to_string());
        }
        from = vstart;
    }
    None
}

/// 各 HTML の構造の宣言を、表に突き合わせて検査する。`files` は (リポジトリの根からのパス, 中身)。
pub fn check_declarations(table: &Structures, files: &[(String, String)]) -> Vec<Violation> {
    let mut out = Vec::new();
    let default = table.patterns.iter().find(|p| p.default);
    for (path, html) in files {
        let lower = path.to_ascii_lowercase();
        if !(lower.ends_with(".html") || lower.ends_with(".htm")) {
            continue;
        }
        let mut bad = |d: String| out.push(violation(path, d));
        let Some(structure) = meta_content(html, "page-structure") else {
            bad("`<meta name=\"page-structure\" content=\"...\">` が無い(ページの構造を宣言する。語彙は config/web/structures.toml)".to_string());
            continue;
        };
        let Some(pattern) = table.patterns.iter().find(|p| p.id == structure) else {
            let ids: Vec<&str> = table.patterns.iter().map(|p| p.id.as_str()).collect();
            bad(format!(
                "宣言した構造 `{structure}` が、構造の語彙 {ids:?} に無い"
            ));
            continue;
        };
        // 軸
        let mut declared: BTreeMap<String, String> = BTreeMap::new();
        for part in meta_content(html, "page-axes")
            .unwrap_or_default()
            .split(';')
        {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((k, v)) = part.split_once('=') else {
                bad(format!("`page-axes` の `{part}` は `軸=値` の形でない"));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match table.axes.get(k) {
                None => bad(format!("`page-axes` の軸 `{k}` が、軸の語彙に無い")),
                Some(allowed) if !allowed.iter().any(|x| x == v) => {
                    bad(format!(
                        "`page-axes` の `{k}={v}`: 値が、軸の語彙 {allowed:?} に無い"
                    ));
                }
                Some(_) => {
                    if declared.insert(k.to_string(), v.to_string()).is_some() {
                        bad(format!("`page-axes` の軸 `{k}` が重複している"));
                    }
                }
            }
        }
        for axis in pattern.fits.keys() {
            if !declared.contains_key(axis) {
                bad(format!("構造 `{structure}` を宣言したページは、`page-axes` に軸 `{axis}` を宣言する(整合を確かめるため)"));
            }
        }
        let has_deviation =
            meta_content(html, "page-structure-deviation").is_some_and(|d| !d.trim().is_empty());
        let own_ok = pattern
            .fits
            .iter()
            .all(|(k, allowed)| declared.get(k).is_none_or(|v| allowed.contains(v)));
        if !own_ok && !has_deviation {
            bad(format!("軸の値が、構造 `{structure}` の許す値の外にある。外れる理由を `<meta name=\"page-structure-deviation\">` に書く"));
        }
        if let Some(d) = default {
            let fully = !d.fits.is_empty() && d.fits.keys().all(|k| declared.contains_key(k));
            let fits = d
                .fits
                .iter()
                .all(|(k, allowed)| declared.get(k).is_some_and(|v| allowed.contains(v)));
            if d.id != pattern.id && fully && fits && !has_deviation {
                bad(format!("軸の値が既定の構造 `{}` に合うのに、`{structure}` を選んでいる。既定から外れる理由を `<meta name=\"page-structure-deviation\">` に書く", d.id));
            }
        }
    }
    out
}

/// ツリーの構造の表と、すべての HTML の宣言を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let text = read(STRUCTURES_PATH)?;
    let principles = principle_scopes(&read(PRINCIPLES_PATH)?)?;
    let used = requirement_principles(&read(PAGE_TYPES_PATH)?)?;
    let (table, mut out) = check_structures(STRUCTURES_PATH, &text, &principles, &used);
    if let Some(table) = table {
        let files: Vec<(String, String)> = web_assets::read_files(root)?
            .into_iter()
            .map(|f| (f.path, f.text))
            .collect();
        out.extend(check_declarations(&table, &files));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[[axis]]
id = "independence"
title = "独立性"
values = ["independent", "interdependent"]

[[axis]]
id = "comparison"
title = "比較"
values = ["none", "needed"]

[[pattern]]
id = "step"
title = "ステップ"
default = true
measure = "viewport-fit"
fits = { independence = ["independent"], comparison = ["none"] }
principles = ["p-struct"]

[[pattern]]
id = "single-column"
title = "縦 1 枚"
fits = {}

[[pattern]]
id = "document"
title = "読み物"
measure = "none"
fits = {}
"#;

    fn principles() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("p-struct".to_string(), "structure".to_string()),
            ("p-general".to_string(), "general".to_string()),
        ])
    }

    fn table(text: &str) -> (Option<Structures>, Vec<String>) {
        let (t, v) = check_structures(STRUCTURES_PATH, text, &principles(), &BTreeSet::new());
        (t, v.iter().map(ToString::to_string).collect())
    }

    fn assert_table_fails(text: &str, needle: &str) {
        let (_, d) = table(text);
        assert!(
            d.iter().any(|m| m.contains(needle)),
            "「{needle}」を含む違反が出るはず: {d:?}"
        );
    }

    fn page(structure: &str, axes: &str, deviation: &str) -> String {
        let mut h = format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"page-structure\" content=\"{structure}\">");
        if !axes.is_empty() {
            h.push_str(&format!("<meta name=\"page-axes\" content=\"{axes}\">"));
        }
        if !deviation.is_empty() {
            h.push_str(&format!(
                "<meta name=\"page-structure-deviation\" content=\"{deviation}\">"
            ));
        }
        h.push_str("</head><body></body></html>");
        h
    }

    fn pages(files: &[(&str, String)]) -> Vec<String> {
        let (t, v) = table(GOOD);
        assert!(v.is_empty(), "{v:?}");
        let files: Vec<(String, String)> = files
            .iter()
            .map(|(p, h)| (p.to_string(), h.clone()))
            .collect();
        check_declarations(&t.expect("table"), &files)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn assert_page_fails(html: String, needle: &str) {
        let d = pages(&[("x.html", html)]);
        assert!(
            d.iter().any(|m| m.contains(needle)),
            "「{needle}」を含む違反が出るはず: {d:?}"
        );
    }

    #[test]
    fn a_consistent_table_passes() {
        let (t, d) = table(GOOD);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(t.expect("table").patterns.len(), 3);
    }

    #[test]
    fn the_table_is_closed() {
        assert_table_fails(&format!("bogus = 1\n{GOOD}"), "知らないキー `bogus`");
        assert_table_fails(
            &GOOD.replace("measure = \"none\"", "measure = \"magic\""),
            "`measure` は",
        );
        assert_table_fails(
            &GOOD.replace("title = \"縦 1 枚\"", "title = \"縦 1 枚\"\nextra = 1"),
            "知らないキー `extra`",
        );
        assert_table_fails(
            &GOOD.replace("values = [\"none\", \"needed\"]", "values = [\"none\"]"),
            "2 つ以上",
        );
        assert_table_fails(
            &GOOD.replace("id = \"single-column\"", "id = \"step\""),
            "`id` が重複している",
        );
        assert_table_fails(
            &GOOD.replace(
                "fits = {}\n\n[[pattern]]\nid = \"document\"",
                "fits = {}\ndefault = true\n\n[[pattern]]\nid = \"document\"",
            ),
            "高々 1 つ",
        );
    }

    #[test]
    fn fits_and_principles_must_exist() {
        assert_table_fails(
            &GOOD.replace("independence = [\"independent\"]", "nope = [\"x\"]"),
            "軸 `nope` が",
        );
        assert_table_fails(
            &GOOD.replace(
                "independence = [\"independent\"]",
                "independence = [\"weird\"]",
            ),
            "値 `weird`",
        );
        assert_table_fails(
            &GOOD.replace("[\"p-struct\"]", "[\"missing\"]"),
            "`missing` が",
        );
    }

    #[test]
    fn a_structure_principle_nobody_points_at_is_dead() {
        assert_table_fails(
            &GOOD.replace("principles = [\"p-struct\"]\n", ""),
            "principle `p-struct`",
        );
        let (_, v) = check_structures(
            STRUCTURES_PATH,
            &GOOD.replace("principles = [\"p-struct\"]\n", ""),
            &principles(),
            &BTreeSet::from(["p-struct".to_string()]),
        );
        assert!(
            v.is_empty(),
            "要件が指すなら、パターンが指さなくてよい: {v:?}"
        );
    }

    #[test]
    fn every_page_must_declare_a_known_structure() {
        assert_page_fails("<html><head></head></html>".to_string(), "page-structure");
        assert_page_fails(page("carousel", "", ""), "語彙");
        assert!(pages(&[("a.html", page("document", "", ""))]).is_empty());
        assert!(
            pages(&[("a.css", "body{}".to_string())]).is_empty(),
            "HTML 以外は対象外"
        );
    }

    #[test]
    fn axes_are_checked_against_the_vocabulary_and_the_pattern() {
        assert_page_fails(
            page("step", "independence=independent", ""),
            "軸 `comparison` を宣言する",
        );
        assert_page_fails(
            page(
                "step",
                "independence=independent;comparison=none;nope=x",
                "",
            ),
            "軸 `nope`",
        );
        assert_page_fails(
            page("step", "independence=independent;comparison=weird", ""),
            "`comparison=weird`",
        );
        assert_page_fails(
            page(
                "step",
                "independence=independent;independence=independent;comparison=none",
                "",
            ),
            "重複",
        );
        assert_page_fails(page("step", "independence", ""), "`軸=値` の形でない");
        assert!(pages(&[(
            "a.html",
            page("step", "independence=independent;comparison=none", "")
        )])
        .is_empty());
    }

    #[test]
    fn leaving_the_fit_or_the_default_needs_a_reason() {
        // step を宣言したが、比較が要る: 外れる理由が要る。
        assert_page_fails(
            page("step", "independence=independent;comparison=needed", ""),
            "許す値の外",
        );
        assert!(pages(&[(
            "a.html",
            page(
                "step",
                "independence=independent;comparison=needed",
                "比較が要る"
            )
        )])
        .is_empty());
        // 軸の値が既定(step)に合うのに、縦 1 枚を選ぶ: 理由が要る。
        assert_page_fails(
            page(
                "single-column",
                "independence=independent;comparison=none",
                "",
            ),
            "既定の構造 `step`",
        );
        assert!(pages(&[(
            "a.html",
            page(
                "single-column",
                "independence=independent;comparison=none",
                "全件を見渡す面"
            )
        )])
        .is_empty());
        // 軸の値が既定に合わない縦 1 枚は、理由が要らない。
        assert!(pages(&[(
            "a.html",
            page(
                "single-column",
                "independence=interdependent;comparison=needed",
                ""
            )
        )])
        .is_empty());
        // 軸を宣言しない縦 1 枚は、既定との整合を確かめられないので、理由を要求しない。
        assert!(pages(&[("a.html", page("single-column", "", ""))]).is_empty());
    }

    #[test]
    fn meta_attributes_are_read_literally() {
        let html = "<meta name=\"page-structure\" content=\"step\"><meta data-name=\"page-axes\" content=\"x\">";
        assert_eq!(
            meta_content(html, "page-structure").as_deref(),
            Some("step")
        );
        assert_eq!(
            meta_content(html, "page-axes"),
            None,
            "`data-name` は name でない"
        );
        assert_eq!(
            meta_content("<META NAME=\"a\" CONTENT=\"b\">", "a"),
            None,
            "属性名は小文字だけを読む"
        );
    }
}
