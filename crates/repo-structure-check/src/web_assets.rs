//! Web の資産の不変条件: このリポジトリが持つ HTML・CSS は、第三者から書体・スタイル・スクリプトを読み込まない。
//!
//! 表示のたびに第三者(Google Fonts など)へ要求が飛ぶと、開いた事実と IP アドレスがその第三者に渡る。
//! 書体は機体のものを既定にし、自分のサイトから配信できる面だけが Web フォントを選ぶ(ADR-1677)。
//! スクリプトも同じ(ADR-0000 D4。ADR-1677 D6 の文言には含まれていなかったが、要件 C9 が最初から含めていた)。
//! 外部のスクリプトが要るときは、同梱して自分のサイトから配り、`sha256` を pin する。
//!
//! 読む対象は、追跡されうる `*.html`・`*.htm`・`*.css`(`.` で始まる名前、`target/`、`node_modules/` を除く)。
//! 次の形を違反とする。コメントの中も読む(コメントに書いた参照を、あとで外し忘れる形を避けるため)。
//!
//! - 書体の配信元のホスト(`fonts.googleapis.com`・`fonts.gstatic.com`)への参照
//! - `<link>` の `rel` が `stylesheet`・`preconnect`・`dns-prefetch`・`preload`・`modulepreload` で、`href` が外部(`http://`・`https://`・`//`)
//! - `@import` の先が外部
//! - `@font-face` の中の `url(...)` が外部
//! - `<script>` の `src` が外部
//! - スクリプトの `import("…")`・`from "…"` の先が外部
//!
//! 例外の一覧は持たない。範囲はこのリポジトリの内側だけで、ほかのリポジトリの面は検査しない
//! (配り方は別の裁定。`docs/agents/repo-structure-check.md`)。

use std::fs;
use std::path::Path;

use crate::manifest::Violation;

/// 検査する HTML・CSS のファイル 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFile {
    /// リポジトリの根からの相対パス(`/` 区切り)。
    pub path: String,
    pub text: String,
}

/// 書体を配る第三者のホスト。
const FONT_HOSTS: [&str; 2] = ["fonts.googleapis.com", "fonts.gstatic.com"];

/// 外部の資源を読む `<link rel>` の値。
const LOADING_RELS: [&str; 5] = [
    "stylesheet",
    "preconnect",
    "dns-prefetch",
    "preload",
    "modulepreload",
];

fn violation(subject: &str, line: usize, detail: &str) -> Violation {
    Violation {
        subject: subject.to_string(),
        detail: format!("{line} 行目: {detail}"),
    }
}

/// `url` が外部(スキーム付き、またはスキーム相対)か。
fn is_remote(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("//")
}

/// `text` の先頭から数えた、バイト位置 `pos` の行(1 から)。
fn line_of(text: &str, pos: usize) -> usize {
    text[..pos].matches('\n').count() + 1
}

/// タグの文字列から、属性 `name` の値を取る(引用符つきと引用符なし)。属性名は大文字小文字を区別しない。
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        let boundary = lower[..start]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || c == '"' || c == '\'');
        let rest = tag[end..].trim_start();
        if boundary && rest.starts_with('=') {
            let value = rest[1..].trim_start();
            return Some(match value.chars().next() {
                Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or("").to_string(),
                _ => value
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
                    .to_string(),
            });
        }
        from = end;
    }
    None
}

/// `<link ...>` のタグごとに、外部から資源を読む形を探す。
fn link_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("<link") {
        let start = from + pos;
        let end = file.text[start..]
            .find('>')
            .map_or(file.text.len(), |e| start + e + 1);
        let tag = &file.text[start..end];
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        let loads = rel.split_whitespace().any(|r| LOADING_RELS.contains(&r));
        if loads {
            if let Some(href) = attr(tag, "href") {
                if is_remote(&href) {
                    let line = line_of(&file.text, start);
                    out.push((
                        line,
                        violation(
                            &file.path,
                            line,
                            &format!("`<link rel=\"{rel}\">` が外部 `{href}` を読む。書体・スタイルは自分のサイトから配信するか、機体のものを使う"),
                        ),
                    ));
                }
            }
        }
        from = end;
    }
}

/// `@import` の先が外部のものを探す。
fn import_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("@import") {
        let start = from + pos;
        let after = start + "@import".len();
        let stmt_end = file.text[after..]
            .find(';')
            .map_or(file.text.len(), |e| after + e);
        let stmt = file.text[after..stmt_end]
            .trim_start()
            .trim_start_matches("url(")
            .trim_start_matches(['"', '\'']);
        if is_remote(stmt) {
            let line = line_of(&file.text, start);
            out.push((
                line,
                violation(
                    &file.path,
                    line,
                    "`@import` が外部のスタイルを読む。書体・スタイルは自分のサイトから配信するか、機体のものを使う",
                ),
            ));
        }
        from = stmt_end;
    }
}

/// `@font-face` の中の `url(...)` が外部のものを探す。
fn font_face_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("@font-face") {
        let start = from + pos;
        let block_end = file.text[start..]
            .find('}')
            .map_or(file.text.len(), |e| start + e);
        let block = &file.text[start..block_end];
        let block_lower = &lower[start..block_end];
        let mut u_from = 0;
        while let Some(u) = block_lower[u_from..].find("url(") {
            let arg_start = u_from + u + "url(".len();
            let arg = block[arg_start..]
                .trim_start()
                .trim_start_matches(['"', '\'']);
            if is_remote(arg) {
                let line = line_of(&file.text, start + arg_start);
                out.push((
                    line,
                    violation(
                        &file.path,
                        line,
                        "`@font-face` の `url(...)` が外部の書体を読む。書体は自分のサイトから配信するか、機体のものを使う",
                    ),
                ));
            }
            u_from = arg_start;
        }
        from = block_end;
    }
}

/// `<script ...>` のタグごとに、`src` が外部のものを探す。
fn script_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("<script") {
        let start = from + pos;
        let end = file.text[start..]
            .find('>')
            .map_or(file.text.len(), |e| start + e + 1);
        // `<script` の直後が空白・`>`・`/` でなければ、別のタグ(`<scripted>` など)。
        let is_script = lower[start + "<script".len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/');
        if is_script {
            if let Some(src) = attr(&file.text[start..end], "src") {
                if is_remote(&src) {
                    let line = line_of(&file.text, start);
                    out.push((
                        line,
                        violation(
                            &file.path,
                            line,
                            &format!("`<script src>` が外部 `{src}` を読む。スクリプトは自分のサイトから配信する(外部のものは、同梱して sha256 を pin する)"),
                        ),
                    ));
                }
            }
        }
        from = end;
    }
}

/// スクリプトの `import("…")`・`from "…"` の先が外部のものを探す。
fn script_import_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    for needle in ["import(", " from "] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(needle) {
            let start = from + pos;
            let after = start + needle.len();
            let rest = file.text[after..].trim_start();
            let quoted = rest.starts_with(['"', '\'', '`']);
            if quoted && is_remote(rest.trim_start_matches(['"', '\'', '`'])) {
                let line = line_of(&file.text, start);
                out.push((
                    line,
                    violation(
                        &file.path,
                        line,
                        "スクリプトの `import`・`from` が外部のモジュールを読む。スクリプトは自分のサイトから配信する(外部のものは、同梱して sha256 を pin する)",
                    ),
                ));
            }
            from = after;
        }
    }
}

/// 書体の配信元のホストへの参照を探す(上の形に当たらない書き方の取りこぼしを拾う)。
fn host_violations(file: &WebFile, out: &mut Vec<(usize, Violation)>) {
    let lower = file.text.to_ascii_lowercase();
    for host in FONT_HOSTS {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(host) {
            let start = from + pos;
            let line = line_of(&file.text, start);
            out.push((
                line,
                violation(
                    &file.path,
                    line,
                    &format!("書体の配信元 `{host}` を参照している。書体は自分のサイトから配信するか、機体のものを使う"),
                ),
            ));
            from = start + host.len();
        }
    }
}

/// Web の資産の不変条件を検査する。
pub fn check_web_assets(files: &[WebFile]) -> Vec<Violation> {
    let mut out = Vec::new();
    for f in files {
        // 具体的な形を先に出し、同じ行の host の参照は重ねて出さない。
        let mut found: Vec<(usize, Violation)> = Vec::new();
        link_violations(f, &mut found);
        import_violations(f, &mut found);
        font_face_violations(f, &mut found);
        script_violations(f, &mut found);
        script_import_violations(f, &mut found);
        let mut hosts: Vec<(usize, Violation)> = Vec::new();
        host_violations(f, &mut hosts);
        for (line, v) in hosts {
            if !found.iter().any(|(l, _)| *l == line) {
                found.push((line, v));
            }
        }
        found.sort_by_key(|(line, _)| *line);
        out.extend(found.into_iter().map(|(_, v)| v));
    }
    out
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<WebFile>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{} を読めない: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            collect(&p, root, out)?;
        } else if p.is_file() && is_web_file(name) {
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let text =
                fs::read_to_string(&p).map_err(|e| format!("{} を読めない: {e}", p.display()))?;
            out.push(WebFile { path: rel, text });
        }
    }
    Ok(())
}

fn is_web_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".html", ".htm", ".css"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// ツリーの `*.html`・`*.htm`・`*.css` を全部読む。
pub fn read_files(root: &Path) -> Result<Vec<WebFile>, String> {
    let mut out = Vec::new();
    collect(root, root, &mut out)?;
    Ok(out)
}

/// リポジトリのツリー全体を検査する。
pub fn check_tree(root: &Path) -> Result<Vec<Violation>, String> {
    Ok(check_web_assets(&read_files(root)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> WebFile {
        WebFile {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    fn details(files: &[WebFile]) -> Vec<String> {
        check_web_assets(files)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_page_with_no_external_loads_is_clean() {
        let page = file(
            "a.html",
            "<!doctype html><html><head><link rel=\"stylesheet\" href=\"site.css\">\
             <link rel=\"icon\" href=\"data:,\"><link rel=\"canonical\" href=\"https://example.com/\">\
             <style>:root{--font-body:\"Noto Sans JP\",system-ui,sans-serif}\
             @font-face{font-family:x;src:url(\"fonts/x.woff2\")}\
             .a{background:url(https://example.com/a.png)}</style></head></html>",
        );
        assert_eq!(details(&[page]), Vec::<String>::new());
    }

    #[test]
    fn a_remote_stylesheet_link_is_reported_with_its_line() {
        let page = file(
            "a.html",
            "<head>\n<link rel=\"stylesheet\" href=\"https://cdn.example.com/x.css\">\n</head>",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("a.html: 2 行目:") && v[0].contains("https://cdn.example.com/x.css"),
            "{v:?}"
        );
    }

    #[test]
    fn preconnect_and_scheme_relative_and_unquoted_forms_are_reported() {
        let page = file(
            "a.html",
            "<link rel=preconnect href=//cdn.example.com>\n\
             <LINK HREF='http://a.example.com/x.css' REL='Stylesheet'>\n\
             <link href=\"https://a.example.com/x\" rel=\"alternate stylesheet\">",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 3, "{v:?}");
    }

    #[test]
    fn the_google_fonts_hosts_are_reported_once_per_line() {
        let page = file(
            "a.html",
            "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n\
             <link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n\
             <link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=X\">",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 3, "{v:?}");
    }

    #[test]
    fn a_host_in_a_form_the_other_rules_miss_is_still_reported() {
        let page = file(
            "a.css",
            ".a { font-family: x; }\n/* see fonts.gstatic.com */\n",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("a.css: 2 行目:") && v[0].contains("fonts.gstatic.com"),
            "{v:?}"
        );
    }

    #[test]
    fn a_remote_import_is_reported_in_each_syntax() {
        let css = file(
            "a.css",
            "@import url(\"https://cdn.example.com/a.css\");\n\
             @import 'https://cdn.example.com/b.css';\n\
             @import url(//cdn.example.com/c.css);\n\
             @import \"local.css\";\n\
             @import url(local2.css);\n",
        );
        let v = details(&[css]);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().all(|m| m.contains("`@import`")), "{v:?}");
    }

    #[test]
    fn a_remote_url_inside_font_face_is_reported() {
        let css = file(
            "a.css",
            "@font-face {\n  font-family: X;\n  src: local(X), url(https://cdn.example.com/x.woff2) format(\"woff2\");\n}\n\
             @font-face { font-family: Y; src: url(y.woff2); }\n",
        );
        let v = details(&[css]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("a.css: 3 行目:") && v[0].contains("`@font-face`"),
            "{v:?}"
        );
    }

    #[test]
    fn files_are_reported_in_the_order_given_and_lines_in_order() {
        let a = file(
            "a.html",
            "x\n<link rel=stylesheet href=https://e.example/a.css>\n",
        );
        let b = file("b.css", "@import 'https://e.example/b.css';\n");
        let v = details(&[a, b]);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(
            v[0].starts_with("a.html:") && v[1].starts_with("b.css:"),
            "{v:?}"
        );
    }

    #[test]
    fn only_html_and_css_files_are_read() {
        assert!(is_web_file("a.html") && is_web_file("A.CSS") && is_web_file("b.htm"));
        assert!(!is_web_file("a.md") && !is_web_file("a.rs") && !is_web_file("html"));
    }

    #[test]
    fn a_remote_script_src_is_reported_in_each_syntax() {
        let page = file(
            "a.html",
            "<script src=\"https://cdn.example.com/a.js\"></script>\n\
             <SCRIPT type=module SRC=//cdn.example.com/b.js></SCRIPT>\n\
             <script\n  src='http://cdn.example.com/c.js'></script>",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().all(|m| m.contains("`<script src>`")), "{v:?}");
        assert!(v[0].starts_with("a.html: 1 行目:"), "{v:?}");
    }

    #[test]
    fn a_local_script_and_an_inline_script_are_clean() {
        let page = file(
            "a.html",
            "<script src=\"behavior.js\"></script>\n<script src=\"/behavior.js\"></script>\n\
             <script>const a = 1; // https://example.com/ in a comment is not a load\n</script>\n\
             <scripted src=\"https://cdn.example.com/x.js\"></scripted>",
        );
        assert_eq!(details(&[page]), Vec::<String>::new());
    }

    #[test]
    fn a_remote_modulepreload_is_reported() {
        let page = file(
            "a.html",
            "<link rel=\"modulepreload\" href=\"https://cdn.example.com/a.js\">",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("modulepreload"), "{v:?}");
    }

    #[test]
    fn a_remote_dynamic_import_and_a_remote_from_are_reported() {
        let page = file(
            "a.html",
            "<script type=module>\n\
             const m = await import(\"https://cdn.example.com/a.js\");\n\
             import x from 'https://cdn.example.com/b.js';\n\
             const ok = await import(\"./local.js\");\n\
             // it came from https://example.com/ (prose, no quote)\n\
             </script>",
        );
        let v = details(&[page]);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(
            v[0].starts_with("a.html: 2 行目:") && v[1].starts_with("a.html: 3 行目:"),
            "{v:?}"
        );
    }
}
