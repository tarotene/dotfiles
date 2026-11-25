//! 部品の不変条件: 共有の部品 `config/web/components.css` は、正本 `config/web/variables.css` の役割と尺度
//! だけを参照し、操作部の大きさと置き方の裁定(ADR-1687)を守る。
//!
//! - 色を 16 進で直書きしない(面のテーマが役割を上書きしても、部品が追従するため)。
//! - `var(--名前)` の名前は、すべて `variables.css` に宣言がある(部品が、正本に無い値を作らない)。
//! - 必須の部品(`.btn-main`・`.btn-sub`・`.btn-link`・`.actions`・`.choice`・`.status`・`.alert-ng`・`.empty`・
//!   `.skip-link`・`.table-cards`・`:focus-visible`)がある。
//! - `.btn` は `min-height: var(--target)`(操作部の大きさ)を持つ。
//! - `.actions` は、幅の広い画面で `row-reverse` + `space-between`(主な操作を先に書き、PC で右端に置く)。
//! - `@media (prefers-color-scheme)` を書かない(明暗は変数の側が持つ)。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::manifest::Violation;
use crate::web_variables::{declarations, strip_comments};

/// 部品のパス(リポジトリの根から)。
pub const COMPONENTS_PATH: &str = "config/web/components.css";

/// 必須の部品の綴り。
const REQUIRED: [&str; 13] = [
    ".steps",
    ".readout",
    ":focus-visible",
    ".skip-link",
    ".btn-main",
    ".btn-sub",
    ".btn-link",
    ".actions",
    ".choice",
    ".status",
    ".alert-ng",
    ".empty",
    ".table-cards",
];

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

/// 16 進の色(`#rgb`・`#rgba`・`#rrggbb`・`#rrggbbaa`)の直書きがある行番号。
pub(crate) fn hex_color_lines(stripped: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, line) in stripped.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        // 宣言の値だけを読む(最初の `:` より後)。セレクタの `#add` のような id を、色と取り違えない。
        let mut k = chars.iter().position(|c| *c == ':').unwrap_or(chars.len());
        while k < chars.len() {
            if chars[k] == '#' {
                let mut n = 0;
                while k + 1 + n < chars.len() && chars[k + 1 + n].is_ascii_hexdigit() {
                    n += 1;
                }
                let next = chars.get(k + 1 + n);
                let ends = next.is_none_or(|c| !c.is_alphanumeric() && *c != '-' && *c != '_');
                if [3, 4, 6, 8].contains(&n) && ends {
                    out.push(i + 1);
                    break;
                }
            }
            k += 1;
        }
    }
    out
}

/// `var(--名前` の名前を、出てきた順に取る。
pub(crate) fn var_references(stripped: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in stripped.lines().enumerate() {
        let mut rest = line;
        while let Some(p) = rest.find("var(") {
            let after = &rest[p + 4..];
            let name: String = after
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if name.starts_with("--") {
                out.push((i + 1, name));
            }
            rest = after;
        }
    }
    out
}

/// `var(--marker)` を持つのに `var(--text)` を持たない規則(宣言のかたまり `{…}`)の、開き括弧の行番号。
/// 安全黄の `--marker` は、明るい背景とのコントラストが 3:1 に届かないので、単独で「いま」の印にしてはならず、
/// 同じ規則の中で `--text` の輪郭と対にする(`variables.css` の冒頭の注)。
pub(crate) fn marker_without_text_lines(stripped: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut line = 1;
    let mut open: Option<(usize, usize)> = None; // (開き括弧の位置, その行番号)
    for (i, c) in stripped.char_indices() {
        match c {
            '\n' => line += 1,
            '{' => open = Some((i + 1, line)),
            '}' => {
                if let Some((start, open_line)) = open.take() {
                    let block = &stripped[start..i];
                    if block.contains("var(--marker)") && !block.contains("var(--text)") {
                        out.push(open_line);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 部品の文字列を検査する。`declared` は、正本が宣言する変数名。
pub fn check_components(path: &str, css: &str, declared: &BTreeSet<String>) -> Vec<Violation> {
    let mut out = Vec::new();
    let stripped = strip_comments(css);

    for line in marker_without_text_lines(&stripped) {
        out.push(violation(
            path,
            format!("{line} 行目: `var(--marker)` を `var(--text)` の輪郭と対にしていない(安全黄は単独で 3:1 に届かない)"),
        ));
    }

    for line in hex_color_lines(&stripped) {
        out.push(violation(
            path,
            format!("{line} 行目: 色を 16 進で直書きしている。正本の役割(`var(--…)`)を使う"),
        ));
    }
    for (line, name) in var_references(&stripped) {
        if !declared.contains(&name) {
            out.push(violation(
                path,
                format!("{line} 行目: `{name}` は正本 `config/web/variables.css` に宣言が無い"),
            ));
        }
    }
    for needle in REQUIRED {
        if !stripped.contains(needle) {
            out.push(violation(path, format!("必須の部品 `{needle}` が無い")));
        }
    }
    let flat: String = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    if !flat.contains(".btn { min-height: var(--target);") {
        out.push(violation(
            path,
            "`.btn` が `min-height: var(--target);` を先頭に持たない(操作部の大きさ)".to_string(),
        ));
    }
    if !(flat.contains("flex-direction: row-reverse;")
        && flat.contains("justify-content: space-between;"))
    {
        out.push(violation(
            path,
            "`.actions` が、幅の広い画面で `row-reverse` と `space-between`(主な操作を右端、副の操作を左端)を持たない".to_string(),
        ));
    }
    if flat.contains("prefers-color-scheme") {
        out.push(violation(
            path,
            "`@media (prefers-color-scheme)` を書いている。明暗は正本の変数が持つ".to_string(),
        ));
    }
    out
}

/// ツリーの部品を、正本の宣言と突き合わせて検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let variables = read(crate::web_variables::VARIABLES_PATH)?;
    let declared: BTreeSet<String> = declarations(&strip_comments(&variables))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let css = read(COMPONENTS_PATH)?;
    Ok(check_components(COMPONENTS_PATH, &css, &declared))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> (String, BTreeSet<String>) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        let css = fs::read_to_string(root.join(COMPONENTS_PATH)).expect("read components");
        let vars = fs::read_to_string(root.join(crate::web_variables::VARIABLES_PATH))
            .expect("read variables");
        let declared = declarations(&strip_comments(&vars))
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        (css, declared)
    }

    fn details(vs: &[Violation]) -> String {
        vs.iter()
            .map(|v| v.detail.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_real_components_pass() {
        let (css, declared) = real();
        let vs = check_components(COMPONENTS_PATH, &css, &declared);
        assert!(vs.is_empty(), "{}", details(&vs));
    }

    #[test]
    fn a_raw_hex_color_is_reported() {
        let (css, declared) = real();
        let bad = css.replace("color: var(--text);", "color: #1a1a1a;");
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("16 進で直書き"), "{d}");
    }

    #[test]
    fn an_id_selector_is_not_a_hex_color() {
        let (css, declared) = real();
        let ok = format!(
            "{css}\n#add {{ color: var(--text); }}\n#bad-beef-x {{ color: var(--text); }}\n"
        );
        assert!(check_components(COMPONENTS_PATH, &ok, &declared).is_empty());
    }

    #[test]
    fn an_undeclared_variable_is_reported() {
        let (css, declared) = real();
        let bad = css.replace("var(--on-accent)", "var(--on-brand)");
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("`--on-brand` は正本"), "{d}");
    }

    #[test]
    fn a_missing_component_is_reported() {
        let (css, declared) = real();
        let bad = css.replace(".skip-link", ".jump-link");
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("必須の部品 `.skip-link`"), "{d}");
    }

    #[test]
    fn a_button_without_the_target_size_is_reported() {
        let (css, declared) = real();
        let bad = css.replace(
            ".btn {\n  min-height: var(--target);",
            ".btn {\n  min-height: 1rem;",
        );
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("操作部の大きさ"), "{d}");
    }

    #[test]
    fn the_placement_decision_is_required() {
        let (css, declared) = real();
        let bad = css.replace("row-reverse", "row");
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("row-reverse"), "{d}");
    }

    #[test]
    fn a_marker_without_a_text_outline_is_reported() {
        let (css, declared) = real();
        let bad = format!("{css}\n.lonely {{\n  border: 4px solid var(--marker);\n}}\n");
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("`var(--marker)` を `var(--text)`"), "{d}");
        // 輪郭と対なら通る。
        let ok = format!(
            "{css}\n.paired {{\n  border: 4px solid var(--marker);\n  outline: 1px solid var(--text);\n}}\n"
        );
        assert!(check_components(COMPONENTS_PATH, &ok, &declared).is_empty());
    }

    #[test]
    fn the_media_query_for_the_color_scheme_is_reported() {
        let (css, declared) = real();
        let bad = format!(
            "{css}\n@media (prefers-color-scheme: dark) {{ body {{ color: var(--text); }} }}\n"
        );
        let d = details(&check_components(COMPONENTS_PATH, &bad, &declared));
        assert!(d.contains("prefers-color-scheme"), "{d}");
    }
}
