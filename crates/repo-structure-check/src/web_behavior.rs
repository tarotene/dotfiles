//! 振る舞いの共有部品の不変条件: `config/web/behavior.js` と、それを使う見本の `config/web/specimen.js` は、
//! CSP `style-src 'self'` の下で動き、1 枚だけの HTML の区間 pin に複写でき、失敗を握りつぶさない(ADR-1703 D2)。
//!
//! 次の形を違反とする(コメントの中は読まない。文字列の中は読む)。
//!
//! - 外部の URL(`http://`・`https://`)。第三者から読まない(ADR-1677 D6)。
//! - 実行時の style の注入(`.style`・`cssText`・`setAttribute("style", …)`)。`style-src 'self'` と衝突する。
//! - HTML・コードの注入(`innerHTML`・`outerHTML`・`insertAdjacentHTML`・`document.write`・`eval(`・`new Function`)。
//! - モジュールの構文(行頭の `import`・`export`)。インラインの `<script>` に複写できなくなる。
//! - 失敗を握りつぶす形(`catch {}`・`catch (e) {}`・`.catch(() => {})` など、本体の空の捕捉)。
//! - `behavior.js` は `"use strict"` を持ち、`window.pageBehavior` に `saver`・`keyguard`・`viewstate` を置く。
//!   `specimen.js` は、その 3 つを実際に呼ぶ(動く見本を、共有の部品から外さない)。
//!
//! 限界: 正規表現リテラルの中の引用符・`//` は、文字列やコメントと取り違える(この 2 つのファイルは正規表現リテラルを書かない)。

use std::fs;
use std::path::Path;

use crate::manifest::Violation;

/// 共有の振る舞いのパス(リポジトリの根から)。
pub const BEHAVIOR_PATH: &str = "config/web/behavior.js";
/// 動く見本のパス。
pub const SPECIMEN_JS_PATH: &str = "config/web/specimen.js";

/// 共有の振る舞いが公開する名前。
pub const API: [&str; 3] = ["saver", "keyguard", "viewstate"];

fn violation(path: &str, detail: String) -> Violation {
    Violation {
        subject: path.to_string(),
        detail,
    }
}

/// JavaScript のコメントを空白に置き換える(改行は残して、行番号を保つ)。文字列(`'…'`・`"…"`・`` `…` ``)の中は、そのまま残す。
pub fn strip_js_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '/' if next == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if next == Some('*') => {
                out.push_str("  ");
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < chars.len() {
                    out.push_str("  ");
                    i += 2;
                }
            }
            '\'' | '"' | '`' => {
                out.push(c);
                i += 1;
                while i < chars.len() && chars[i] != c {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i]);
                        i += 1;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn is_ident(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// 空白を除いた文字列 `s` の先頭が、本体の空の関数(`()=>{}`・`(e)=>{}`・`e=>{}`・`function(e){}`・`function f(e){}`)か。
fn starts_with_empty_fn(s: &str) -> bool {
    if let Some(i) = s.find("=>{}") {
        let params = &s[..i];
        let bare = !params.contains(')') && is_ident(params);
        let paren = params
            .strip_prefix('(')
            .and_then(|p| p.strip_suffix(')'))
            .is_some_and(is_ident);
        if bare || paren {
            return true;
        }
    }
    if let Some(rest) = s.strip_prefix("function") {
        let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_' || c == '$');
        if let Some(r) = rest.strip_prefix('(') {
            if let Some(end) = r.find(')') {
                return r[end + 1..].starts_with("{}");
            }
        }
    }
    false
}

/// 失敗を握りつぶす形(本体の空の `catch`、本体の空の関数を渡した `.catch(...)`)があるか。
pub fn swallows_errors(code: &str) -> bool {
    let c: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut rest = c.as_str();
    while let Some(p) = rest.find("catch") {
        let after = &rest[p + "catch".len()..];
        if after.starts_with("{}") {
            return true;
        }
        if let Some(inner) = after.strip_prefix('(') {
            // `catch(e){}`
            if let Some(end) = inner.find(')') {
                if is_ident(&inner[..end]) && inner[end + 1..].starts_with("{}") {
                    return true;
                }
            }
            // `.catch(() => {})`・`.catch(function (e) {})`
            if starts_with_empty_fn(inner) {
                return true;
            }
        }
        rest = after;
    }
    false
}

fn line_of(text: &str, pos: usize) -> usize {
    text[..pos].matches('\n').count() + 1
}

/// `needle` の出てくる行(1 から)を、すべて返す。
fn lines_with(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle)
        .map(|(p, _)| line_of(text, p))
        .collect()
}

/// `.style` の出てくる行。`.stylesheet` のような、続きの名前は数えない。
fn style_property_lines(text: &str) -> Vec<usize> {
    text.match_indices(".style")
        .filter(|(p, _)| {
            !text[p + ".style".len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .map(|(p, _)| line_of(text, p))
        .collect()
}

/// 行頭の `import`・`export`(モジュールの構文)の行。
fn module_syntax_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            [
                "import ", "import{", "import*", "export ", "export{", "export*",
            ]
            .iter()
            .any(|p| t.starts_with(p))
        })
        .map(|(i, _)| i + 1)
        .collect()
}

/// スクリプト 1 つを検査する。`is_behavior` なら、共有の振る舞い本体の条件(`"use strict"`・`pageBehavior` と 3 つの関数)を足す。
pub fn check_script(path: &str, text: &str, is_behavior: bool) -> Vec<Violation> {
    let mut out = Vec::new();
    let code = strip_js_comments(text);

    for needle in ["http://", "https://"] {
        for line in lines_with(&code, needle) {
            out.push(violation(
                path,
                format!(
                    "{line} 行目: 外部の URL(`{needle}`)を持つ。第三者から読まない(ADR-1677 D6)"
                ),
            ));
        }
    }
    for line in style_property_lines(&code) {
        out.push(violation(
            path,
            format!("{line} 行目: `.style` を使っている。実行時の style の注入は `style-src 'self'` と衝突する(クラスと `hidden` 属性で出し入れする)"),
        ));
    }
    for needle in ["cssText", "setAttribute(\"style\"", "setAttribute('style'"] {
        for line in lines_with(&code, needle) {
            out.push(violation(
                path,
                format!("{line} 行目: `{needle}` で style を注入している。`style-src 'self'` と衝突する"),
            ));
        }
    }
    for needle in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "new Function",
    ] {
        for line in lines_with(&code, needle) {
            out.push(violation(
                path,
                format!("{line} 行目: `{needle}` を使っている。HTML・コードを文字列から作らない(`textContent` と要素の組み立てだけ)"),
            ));
        }
    }
    for line in module_syntax_lines(&code) {
        out.push(violation(
            path,
            format!("{line} 行目: モジュールの構文(`import`・`export`)がある。インラインの `<script>` に複写できなくなる"),
        ));
    }
    if swallows_errors(&code) {
        out.push(violation(
            path,
            "失敗を握りつぶす形(本体の空の `catch`、または `.catch(() => {})`)がある。失敗は画面に出す".to_string(),
        ));
    }

    if is_behavior {
        if !code.contains("\"use strict\"") && !code.contains("'use strict'") {
            out.push(violation(path, "`\"use strict\"` が無い".to_string()));
        }
        if !code.contains("root.pageBehavior =") {
            out.push(violation(
                path,
                "`window.pageBehavior` に公開していない(`root.pageBehavior = …`)".to_string(),
            ));
        }
        for name in API {
            if !code.contains(&format!("function {name}(")) {
                out.push(violation(path, format!("関数 `{name}` が無い")));
            }
            if !code.contains(&format!("{name}: {name}")) {
                out.push(violation(
                    path,
                    format!("`pageBehavior` が `{name}` を公開していない(`{name}: {name}`)"),
                ));
            }
        }
    } else {
        for name in API {
            if !code.contains(&format!(".{name}(")) {
                out.push(violation(
                    path,
                    format!("共有の振る舞い `{name}` を呼んでいない(動く見本は、3 つすべてを実際に呼ぶ)"),
                ));
            }
        }
    }
    out
}

/// ツリーの 2 つのスクリプトを検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        let p = root.join(rel);
        fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))
    };
    let mut out = check_script(BEHAVIOR_PATH, &read(BEHAVIOR_PATH)?, true);
    out.extend(check_script(
        SPECIMEN_JS_PATH,
        &read(SPECIMEN_JS_PATH)?,
        false,
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real(rel: &str) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
        fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel} を読めない: {e}"))
    }

    fn details(vs: &[Violation]) -> String {
        vs.iter()
            .map(|v| v.detail.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_real_scripts_pass() {
        let vs = check_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")).expect("read");
        assert!(vs.is_empty(), "{}", details(&vs));
    }

    #[test]
    fn comments_are_blanked_but_strings_are_kept() {
        let src = "a(); // http://x.example\n/* eval( */ b('//not a comment', \"/*x*/\");\n";
        let got = strip_js_comments(src);
        assert!(!got.contains("http://") && !got.contains("eval("), "{got}");
        assert!(
            got.contains("'//not a comment'") && got.contains("\"/*x*/\""),
            "{got}"
        );
        assert_eq!(
            got.matches('\n').count(),
            src.matches('\n').count(),
            "改行を保つ"
        );
    }

    #[test]
    fn an_external_url_is_reported_but_not_in_a_comment() {
        let ok = format!("{}\n// see https://example.com/\n", real(BEHAVIOR_PATH));
        assert!(check_script(BEHAVIOR_PATH, &ok, true).is_empty());
        let bad = format!(
            "{}\nvar u = \"https://cdn.example.com/a.js\";\n",
            real(BEHAVIOR_PATH)
        );
        let d = details(&check_script(BEHAVIOR_PATH, &bad, true));
        assert!(d.contains("外部の URL(`https://`)"), "{d}");
    }

    #[test]
    fn style_injection_is_reported_in_each_form() {
        let base = real(BEHAVIOR_PATH);
        for (snippet, expect) in [
            ("el.style.color = 'red';", "`.style`"),
            ("el.style = 'x';", "`.style`"),
            ("el.style.cssText = 'x';", "`cssText`"),
            ("el.setAttribute('style', 'x');", "setAttribute('style'"),
            ("el.setAttribute(\"style\", 'x');", "setAttribute(\"style\""),
        ] {
            let d = details(&check_script(
                BEHAVIOR_PATH,
                &format!("{base}\n{snippet}\n"),
                true,
            ));
            assert!(d.contains(expect), "{snippet}: {d}");
        }
        // `.stylesheet` のような続きの名前は、style の注入ではない。
        let ok = format!("{base}\nvar n = doc.stylesheets;\n");
        assert!(check_script(BEHAVIOR_PATH, &ok, true).is_empty());
    }

    #[test]
    fn html_and_code_injection_is_reported() {
        let base = real(BEHAVIOR_PATH);
        for snippet in [
            "el.innerHTML = x;",
            "el.outerHTML = x;",
            "el.insertAdjacentHTML('beforeend', x);",
            "document.write(x);",
            "eval(x);",
            "var f = new Function('x');",
        ] {
            let d = details(&check_script(
                BEHAVIOR_PATH,
                &format!("{base}\n{snippet}\n"),
                true,
            ));
            assert!(d.contains("文字列から作らない"), "{snippet}: {d}");
        }
    }

    #[test]
    fn module_syntax_is_reported() {
        let base = real(BEHAVIOR_PATH);
        for snippet in [
            "import x from './x.js';",
            "export const a = 1;",
            "export { a };",
        ] {
            let d = details(&check_script(
                BEHAVIOR_PATH,
                &format!("{snippet}\n{base}"),
                true,
            ));
            assert!(d.contains("モジュールの構文"), "{snippet}: {d}");
        }
        // 文中の単語は、モジュールの構文ではない。
        let ok = format!("{base}\n// import export\nvar important = 1;\n");
        assert!(check_script(BEHAVIOR_PATH, &ok, true).is_empty());
    }

    #[test]
    fn swallowed_errors_are_detected_in_each_form() {
        for bad in [
            "try { a(); } catch { }",
            "try { a(); } catch (e) { }",
            "try { a(); } catch (e) {\n}",
            "p.catch(() => {});",
            "p.catch((e) => { });",
            "p.catch(e => {});",
            "p.catch(function () {});",
            "p.catch(function (e) { });",
            "p.catch(function named(e) {});",
        ] {
            assert!(swallows_errors(bad), "{bad}");
        }
        for ok in [
            "try { a(); } catch (e) { show(e); }",
            "p.catch(function (err) { show(err); });",
            "p.catch((err) => show(err));",
            "p.catch(onFail);",
            "var catcher = 1;",
        ] {
            assert!(!swallows_errors(ok), "{ok}");
        }
    }

    #[test]
    fn the_behavior_file_must_expose_the_three_functions_strictly() {
        let base = real(BEHAVIOR_PATH);
        let no_strict = base.replace("\"use strict\";", "");
        assert!(details(&check_script(BEHAVIOR_PATH, &no_strict, true))
            .contains("`\"use strict\"` が無い"));
        let no_viewstate = base.replace("viewstate: viewstate", "other: viewstate");
        assert!(details(&check_script(BEHAVIOR_PATH, &no_viewstate, true))
            .contains("`pageBehavior` が `viewstate` を公開していない"));
        let no_fn = base.replace("function keyguard(", "function other(");
        assert!(
            details(&check_script(BEHAVIOR_PATH, &no_fn, true)).contains("関数 `keyguard` が無い")
        );
        let no_export = base.replace("root.pageBehavior =", "root.other =");
        assert!(details(&check_script(BEHAVIOR_PATH, &no_export, true))
            .contains("`window.pageBehavior` に公開していない"));
    }

    #[test]
    fn the_specimen_script_must_call_all_three() {
        let base = real(SPECIMEN_JS_PATH);
        assert!(check_script(SPECIMEN_JS_PATH, &base, false).is_empty());
        for name in API {
            let bad = base.replace(&format!(".{name}("), ".other(");
            let d = details(&check_script(SPECIMEN_JS_PATH, &bad, false));
            assert!(
                d.contains(&format!("共有の振る舞い `{name}` を呼んでいない")),
                "{name}: {d}"
            );
        }
    }
}
