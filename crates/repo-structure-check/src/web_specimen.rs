//! 見本ページの不変条件: `config/web/specimen.html` は、正本(`variables.css`・`components.css`)を実際に読み込み、
//! 役割のすべてと部品のすべてを 1 枚に並べる。見本は正本を複写せず、値を持たない(ADR-0000)。
//!
//! - 3 つの CSS(`variables.css`・`components.css`・`specimen.css`)を、相対パスで読み込む。
//! - インラインの `style` 属性・`<style>`・`<script>` を持たない(位置と大きさは CSS のクラスで与える)。
//! - 第三者の URL(`http://`・`https://`)を持たない。
//! - 役割の集合(`REQUIRED_ROLES`)のすべてを `data-role` の見本に並べ、`specimen.css` が `var(--役割)` で塗る。
//! - `components.css` のセレクタが使うクラスのすべてを、見本の HTML が使う(部品を足して見本に載せ忘れない)。
//! - `specimen.css` は、色を 16 進で直書きせず、`var()` の名前はすべて正本に宣言がある。
//! - `lang`・viewport・`title`・`main`・`h1` を持つ。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::manifest::Violation;
use crate::web_components::{hex_color_lines, var_references, COMPONENTS_PATH};
use crate::web_variables::{declarations, strip_comments, REQUIRED_ROLES, VARIABLES_PATH};

/// 見本のパス(リポジトリの根から)。
pub const HTML_PATH: &str = "config/web/specimen.html";
/// 見本の配置の CSS。
pub const CSS_PATH: &str = "config/web/specimen.css";

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// セレクタ(`{` の前の文)に出てくるクラス名。`@media` などの at-rule は読まない。
fn selector_classes(stripped: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut buf = String::new();
    for c in stripped.chars() {
        match c {
            '{' => {
                let prelude = buf.trim();
                if !prelude.starts_with('@') {
                    let chars: Vec<char> = prelude.chars().collect();
                    let mut i = 0;
                    while i < chars.len() {
                        if chars[i] == '.' {
                            let name: String = chars[i + 1..]
                                .iter()
                                .take_while(|c| c.is_alphanumeric() || **c == '-' || **c == '_')
                                .collect();
                            if !name.is_empty() {
                                i += name.chars().count();
                                out.insert(name);
                            }
                        }
                        i += 1;
                    }
                }
                buf.clear();
            }
            '}' | ';' => buf.clear(),
            c => buf.push(c),
        }
    }
    out
}

/// HTML の `class="…"` に出てくるクラス名。
fn html_classes(html: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = html;
    while let Some(p) = rest.find("class=\"") {
        let after = &rest[p + 7..];
        let Some(end) = after.find('"') else { break };
        out.extend(after[..end].split_whitespace().map(str::to_string));
        rest = &after[end..];
    }
    out
}

/// 見本の文字列を検査する。`variables`・`components` は、正本の文字列。
pub fn check_specimen(html: &str, css: &str, variables: &str, components: &str) -> Vec<Violation> {
    let mut out = Vec::new();

    for href in ["variables.css", "components.css", "specimen.css"] {
        if !html.contains(&format!("href=\"{href}\"")) {
            out.push(violation(
                HTML_PATH,
                format!("`{href}` を相対パスで読み込んでいない(見本は正本を実際に読み込んで描く)"),
            ));
        }
    }
    for (needle, why) in [
        (" style=", "インラインの `style` 属性"),
        ("<style", "`<style>` 要素"),
        ("<script", "`<script>` 要素"),
    ] {
        if html.contains(needle) {
            out.push(violation(
                HTML_PATH,
                format!("{why}を持つ(位置と大きさは、`specimen.css` のクラスで与える)"),
            ));
        }
    }
    for (path, text) in [(HTML_PATH, html), (CSS_PATH, css)] {
        if text.contains("http://") || text.contains("https://") {
            out.push(violation(
                path,
                "第三者の URL(`http://`・`https://`)を持つ(ADR-1677 D6)".to_string(),
            ));
        }
    }
    for (needle, why) in [
        ("lang=\"ja\"", "`lang=\"ja\"`"),
        ("name=\"viewport\"", "viewport"),
        ("<title>", "`<title>`"),
        ("<main", "`<main>`"),
        ("<h1", "`<h1>`"),
    ] {
        if !html.contains(needle) {
            out.push(violation(HTML_PATH, format!("{why}が無い")));
        }
    }

    let stripped_css = strip_comments(css);
    let flat_css = squash(&stripped_css);
    for role in REQUIRED_ROLES {
        let short = role.trim_start_matches("--");
        if !html.contains(&format!("data-role=\"{short}\"")) {
            out.push(violation(
                HTML_PATH,
                format!("役割 `{role}` の見本(`data-role=\"{short}\"`)が無い"),
            ));
        }
        let rule = format!(".swatch[data-role=\"{short}\"] .chip {{ background: var({role}); }}");
        if !flat_css.contains(&rule) {
            out.push(violation(
                CSS_PATH,
                format!("役割 `{role}` を `var({role})` で塗る規則が無い"),
            ));
        }
    }

    let declared: BTreeSet<String> = declarations(&strip_comments(variables))
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for line in hex_color_lines(&stripped_css) {
        out.push(violation(
            CSS_PATH,
            format!("{line} 行目: 色を 16 進で直書きしている。正本の役割(`var(--…)`)を使う"),
        ));
    }
    for (line, name) in var_references(&stripped_css) {
        if !declared.contains(&name) {
            out.push(violation(
                CSS_PATH,
                format!("{line} 行目: `{name}` は正本 `{VARIABLES_PATH}` に宣言が無い"),
            ));
        }
    }

    let used = html_classes(html);
    for class in selector_classes(&strip_comments(components)) {
        if !used.contains(&class) {
            out.push(violation(
                HTML_PATH,
                format!("`{COMPONENTS_PATH}` の部品 `.{class}` が、見本に載っていない"),
            ));
        }
    }
    out
}

/// ツリーの見本を、正本と突き合わせて検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    Ok(check_specimen(
        &read(HTML_PATH)?,
        &read(CSS_PATH)?,
        &read(VARIABLES_PATH)?,
        &read(COMPONENTS_PATH)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> (String, String, String, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let read = |rel: &str| {
            fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel} を読めない: {e}"))
        };
        (
            read(HTML_PATH),
            read(CSS_PATH),
            read(VARIABLES_PATH),
            read(COMPONENTS_PATH),
        )
    }

    fn details(vs: &[Violation]) -> String {
        vs.iter()
            .map(|v| v.detail.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_real_specimen_passes() {
        let (h, c, v, k) = real();
        let vs = check_specimen(&h, &c, &v, &k);
        assert!(vs.is_empty(), "{}", details(&vs));
    }

    #[test]
    fn the_class_scanner_reads_selectors_not_declarations() {
        let css = "@media (min-width: 40rem) { .actions { margin: 0.5rem; } } .btn:hover, .btn-main .x { color: red; }";
        let got: Vec<String> = selector_classes(css).into_iter().collect();
        assert_eq!(got, vec!["actions", "btn", "btn-main", "x"]);
    }

    #[test]
    fn a_component_missing_from_the_specimen_is_reported() {
        let (h, c, v, k) = real();
        let bad = h.replace("alert-warn", "alert-x");
        let d = details(&check_specimen(&bad, &c, &v, &k));
        assert!(d.contains("`.alert-warn` が、見本に載っていない"), "{d}");
    }

    #[test]
    fn a_role_missing_from_the_specimen_is_reported() {
        let (h, c, v, k) = real();
        let bad = h.replace("data-role=\"warn-bg\"", "data-role=\"x\"");
        let d = details(&check_specimen(&bad, &c, &v, &k));
        assert!(d.contains("`--warn-bg` の見本"), "{d}");
        let bad_css = c.replace(
            ".swatch[data-role=\"ng\"] .chip {\n  background: var(--ng);\n}",
            "",
        );
        let d = details(&check_specimen(&h, &bad_css, &v, &k));
        assert!(d.contains("`--ng` を `var(--ng)` で塗る規則が無い"), "{d}");
    }

    #[test]
    fn inline_style_script_and_third_party_urls_are_reported() {
        let (h, c, v, k) = real();
        let bad = h.replace(
            "<main id=\"main\">",
            "<main id=\"main\" style=\"color: red\">",
        );
        assert!(details(&check_specimen(&bad, &c, &v, &k)).contains("`style` 属性"));
        let bad = h.replace("</head>", "<script>1</script></head>");
        assert!(details(&check_specimen(&bad, &c, &v, &k)).contains("`<script>`"));
        let bad = h.replace(
            "</head>",
            "<link rel=\"stylesheet\" href=\"https://x.example/a.css\"></head>",
        );
        assert!(details(&check_specimen(&bad, &c, &v, &k)).contains("第三者の URL"));
    }

    #[test]
    fn a_specimen_that_does_not_load_the_canon_is_reported() {
        let (h, c, v, k) = real();
        let bad = h.replace("href=\"components.css\"", "href=\"copy.css\"");
        let d = details(&check_specimen(&bad, &c, &v, &k));
        assert!(
            d.contains("`components.css` を相対パスで読み込んでいない"),
            "{d}"
        );
    }

    #[test]
    fn a_hex_color_and_an_undeclared_variable_in_the_specimen_css_are_reported() {
        let (h, c, v, k) = real();
        let bad = format!("{c}\n.x {{\n  color: #ff0000;\n  margin: var(--nope);\n}}\n");
        let d = details(&check_specimen(&h, &bad, &v, &k));
        assert!(d.contains("16 進で直書き"), "{d}");
        assert!(d.contains("`--nope` は正本"), "{d}");
    }
}
