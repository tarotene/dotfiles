//! docs-index — `docs/README.md` の索引を、各文書の H1 から導く。
//!
//! 索引の一覧は、人が書き足し続ける記録にしない。`docs/` の文書の置き場(`adr/`・`agents/`・`claude/`・直下)ごとに
//! 1 つの節があり、節の中身は `<!-- BEGIN GENERATED: docs-index <id> -->` と `<!-- END GENERATED -->` の間に
//! 生成する。1 行は、文書へのリンクと、その文書の H1 だけで、手で書いた要約は持たない
//! (要約は文書の側で腐るので、見出しを正本にする)。生成の外の文章は手で書いたまま残る。
//!
//! 文書を足して索引に載せ忘れる、題名を変えて索引が古くなる、はどちらも、`render` を実行するまで
//! `--check`(と `tests/tree.rs`)が落ちる形で検出する。置き場の知らない `docs/<dir>/*.md` と、
//! H1 の無い文書は、索引を作れないので失敗にする。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const BEGIN_PREFIX: &str = "<!-- BEGIN GENERATED: docs-index ";
pub const BEGIN_SUFFIX: &str = " -->";
pub const END_MARKER: &str = "<!-- END GENERATED -->";

/// 節の id、`docs/` の下で読む場所(空は直下)。
pub const BLOCKS: [(&str, &str); 4] = [
    ("adr", "adr"),
    ("root", ""),
    ("agents", "agents"),
    ("claude", "claude"),
];

/// 索引に載る 1 つの文書。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// `docs/` からの相対パス(例: `adr/0001-x.md`)。
    pub rel: String,
    /// H1 の文字列。
    pub title: String,
}

/// Markdown の最初の H1(コードの囲みの中は読まない)。
pub fn first_h1(md: &str) -> Option<String> {
    let mut in_fence = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            if let Some(rest) = line.strip_prefix("# ") {
                let t = rest.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

/// ADR の H1 から `ADR-<番号> — ` の接頭辞を除く。接頭辞の無い H1 はそのまま返す。
pub fn adr_title(h1: &str) -> &str {
    let Some(rest) = h1.strip_prefix("ADR-") else {
        return h1;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return h1;
    }
    let after = rest[digits..].trim_start();
    for sep in ["—", "–", "-", ":"] {
        if let Some(t) = after.strip_prefix(sep) {
            return t.trim_start();
        }
    }
    h1
}

/// `docs/<dir>/` の直下の `*.md` を読む。`README.md` は索引自身なので載せない。
fn read_dir_docs(docs: &Path, dir: &str, problems: &mut Vec<String>) -> Vec<Doc> {
    let path = if dir.is_empty() {
        docs.to_path_buf()
    } else {
        docs.join(dir)
    };
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&path) else {
        return out;
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md") && !(dir.is_empty() && n == "README.md"))
        .collect();
    names.sort();
    for name in names {
        let rel = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        match fs::read_to_string(docs.join(&rel)) {
            Ok(md) => match first_h1(&md) {
                Some(title) => out.push(Doc { rel, title }),
                None => problems.push(format!("docs/{rel}: H1(`# 題名`)が無い")),
            },
            Err(e) => problems.push(format!("docs/{rel}: 読めない: {e}")),
        }
    }
    out
}

/// 索引の節のどれにも当たらない置き場の文書を、失敗として集める
/// (節を足してから文書を置く、の順を強いる)。
fn unknown_places(docs: &Path, problems: &mut Vec<String>) {
    let known: Vec<&str> = BLOCKS
        .iter()
        .map(|(_, d)| *d)
        .filter(|d| !d.is_empty())
        .collect();
    fn has_md(dir: &Path) -> Option<String> {
        for e in fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(found) = has_md(&p) {
                    return Some(found);
                }
            } else if p.extension().is_some_and(|x| x == "md") {
                return Some(p.display().to_string());
            }
        }
        None
    }
    let Ok(rd) = fs::read_dir(docs) else { return };
    let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());
    for e in dirs {
        let name = e.file_name().to_string_lossy().into_owned();
        if known.contains(&name.as_str()) {
            // 入れ子(adr/x/y.md など)は、直下だけを読む索引に載らない。
            if let Ok(rd) = fs::read_dir(e.path()) {
                for sub in rd.flatten().filter(|s| s.path().is_dir()) {
                    if let Some(found) = has_md(&sub.path()) {
                        problems.push(format!(
                            "{found}: docs/{name}/ の入れ子の文書は、索引(直下だけを読む)に載らない"
                        ));
                    }
                }
            }
        } else if let Some(found) = has_md(&e.path()) {
            problems.push(format!(
                "{found}: docs/{name}/ に対応する索引の節が無い(BLOCKS に足して、README にマーカーを置く)"
            ));
        }
    }
}

/// 節ごとの文書。`Err` は索引を作れない理由(H1 の欠落、置き場の知らない文書)。
pub fn scan(docs: &Path) -> Result<BTreeMap<&'static str, Vec<Doc>>, Vec<String>> {
    let mut problems = Vec::new();
    let mut out = BTreeMap::new();
    for (id, dir) in BLOCKS {
        out.insert(id, read_dir_docs(docs, dir, &mut problems));
    }
    unknown_places(docs, &mut problems);
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// ADR のファイル名の番号の文字列(`0001-x.md` なら `0001`)。
fn adr_number(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.split('-').next().unwrap_or(name)
}

/// 並べ替えの鍵。番号の昇順で、`0000`(PR 番号が決まる前の仮番号)は最後に置く。
fn adr_sort_key(rel: &str) -> (u64, String) {
    let n = adr_number(rel).parse::<u64>().unwrap_or(u64::MAX);
    (if n == 0 { u64::MAX } else { n }, rel.to_string())
}

/// 節 1 つ分の行。最後の行にも改行を付ける。
pub fn render_block(id: &str, docs: &[Doc]) -> String {
    let mut docs: Vec<&Doc> = docs.iter().collect();
    let mut out = String::new();
    if id == "adr" {
        docs.sort_by_key(|d| adr_sort_key(&d.rel));
        for d in docs {
            out.push_str(&format!(
                "- [ADR-{}]({}) — {}\n",
                adr_number(&d.rel),
                d.rel,
                adr_title(&d.title)
            ));
        }
    } else {
        docs.sort_by(|a, b| a.rel.cmp(&b.rel));
        for d in docs {
            out.push_str(&format!("- [`{}`]({}) — {}\n", d.rel, d.rel, d.title));
        }
    }
    out
}

/// README の生成の節を、`blocks` から作った行で置き換える。
pub fn apply(
    readme: &str,
    blocks: &BTreeMap<&'static str, Vec<Doc>>,
) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut lines = readme.split_inclusive('\n');
    while let Some(line) = lines.next() {
        out.push_str(line);
        let t = line.trim_end_matches('\n').trim_end();
        let Some(id) = t
            .strip_prefix(BEGIN_PREFIX)
            .and_then(|r| r.strip_suffix(BEGIN_SUFFIX))
        else {
            continue;
        };
        let Some((known, _)) = BLOCKS.iter().find(|(b, _)| *b == id) else {
            problems.push(format!("知らない節 id: {id}"));
            continue;
        };
        if seen.contains(known) {
            problems.push(format!("節 {id} のマーカーが 2 回ある"));
        }
        seen.push(known);
        // 古い中身を END_MARKER まで読み捨てる。
        let mut closed = false;
        for inner in lines.by_ref() {
            if inner.trim_end_matches('\n').trim_end() == END_MARKER {
                out.push_str(&render_block(known, &blocks[known]));
                out.push_str(inner);
                closed = true;
                break;
            }
        }
        if !closed {
            problems.push(format!("節 {id} に {END_MARKER} が無い"));
        }
    }
    for (id, _) in BLOCKS {
        if !seen.contains(&id) {
            problems.push(format!(
                "docs/README.md に節 {id} のマーカー({BEGIN_PREFIX}{id}{BEGIN_SUFFIX} と {END_MARKER})が無い"
            ));
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// `root`(リポジトリの根)の `docs/` から、あるべき `docs/README.md` の全文を作る。
pub fn expected(root: &Path) -> Result<(String, String), Vec<String>> {
    let docs = root.join("docs");
    let readme_path = docs.join("README.md");
    let current = fs::read_to_string(&readme_path)
        .map_err(|e| vec![format!("{}: 読めない: {e}", readme_path.display())])?;
    let blocks = scan(&docs)?;
    let rendered = apply(&current, &blocks)?;
    Ok((current, rendered))
}

/// 現在の内容とあるべき内容の、最初に違う行(1 始まり)と両者の行。
pub fn first_difference(current: &str, expected: &str) -> Option<(usize, String, String)> {
    let mut a = current.lines();
    let mut b = expected.lines();
    let mut n = 0;
    loop {
        n += 1;
        match (a.next(), b.next()) {
            (None, None) => return None,
            (x, y) if x == y => {}
            (x, y) => {
                return Some((
                    n,
                    x.unwrap_or("(行なし)").to_string(),
                    y.unwrap_or("(行なし)").to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rel: &str, title: &str) -> Doc {
        Doc {
            rel: rel.into(),
            title: title.into(),
        }
    }

    #[test]
    fn the_first_h1_skips_fenced_code_and_deeper_headings() {
        let md = "intro\n```\n# not a title\n```\n## sub\n# Real title \n";
        assert_eq!(first_h1(md).as_deref(), Some("Real title"));
        assert_eq!(first_h1("no heading\n## only sub\n"), None);
    }

    #[test]
    fn the_adr_prefix_is_stripped_and_other_titles_are_kept() {
        assert_eq!(adr_title("ADR-0035 — 3 軸の辞書式順序"), "3 軸の辞書式順序");
        assert_eq!(adr_title("ADR-872: pipeline map"), "pipeline map");
        assert_eq!(
            adr_title("hook-router — 検査をまとめる"),
            "hook-router — 検査をまとめる"
        );
        assert_eq!(adr_title("ADR-x — not a number"), "ADR-x — not a number");
    }

    #[test]
    fn adr_lines_are_numeric_with_the_provisional_zero_last() {
        let docs = vec![
            doc("adr/0000-new-decision.md", "ADR-0000 — new"),
            doc("adr/872-pipeline-map.md", "ADR-872 — map"),
            doc("adr/0033-nav.md", "ADR-0033 — nav"),
            doc("adr/1400-layers.md", "ADR-1400 — layers"),
        ];
        let out = render_block("adr", &docs);
        let order: Vec<&str> = out.lines().map(|l| l.split(']').next().unwrap()).collect();
        assert_eq!(
            order,
            ["- [ADR-0033", "- [ADR-872", "- [ADR-1400", "- [ADR-0000"]
        );
        assert!(out.contains("- [ADR-872](adr/872-pipeline-map.md) — map\n"));
    }

    #[test]
    fn other_lines_are_sorted_by_path_and_show_the_h1_as_is() {
        let docs = vec![doc("setup.md", "Setup"), doc("operations.md", "Ops")];
        assert_eq!(
            render_block("root", &docs),
            "- [`operations.md`](operations.md) — Ops\n- [`setup.md`](setup.md) — Setup\n"
        );
    }

    fn blocks(adr: Vec<Doc>) -> BTreeMap<&'static str, Vec<Doc>> {
        let mut m = BTreeMap::new();
        m.insert("adr", adr);
        m.insert("root", vec![]);
        m.insert("agents", vec![]);
        m.insert("claude", vec![]);
        m
    }

    fn readme(body_adr: &str) -> String {
        format!(
            "# index\n\nhand written\n\n{b}adr -->\n{body_adr}{e}\n\n{b}root -->\n{e}\n{b}agents -->\n{e}\n{b}claude -->\n{e}\ntail\n",
            b = "<!-- BEGIN GENERATED: docs-index ",
            e = END_MARKER
        )
    }

    #[test]
    fn apply_replaces_only_the_generated_part() {
        let current = readme("- stale line\n");
        let out = apply(
            &current,
            &blocks(vec![doc("adr/0001-a.md", "ADR-0001 — A")]),
        )
        .unwrap();
        assert!(out.contains("- [ADR-0001](adr/0001-a.md) — A\n"));
        assert!(!out.contains("stale line"));
        assert!(out.starts_with("# index\n\nhand written\n"));
        assert!(out.ends_with("tail\n"));
        // 冪等: 出力にもう一度かけても変わらない。
        assert_eq!(
            apply(&out, &blocks(vec![doc("adr/0001-a.md", "ADR-0001 — A")])).unwrap(),
            out
        );
    }

    #[test]
    fn a_missing_marker_or_a_missing_end_is_a_failure() {
        let no_claude = "# i\n<!-- BEGIN GENERATED: docs-index adr -->\n<!-- END GENERATED -->\n";
        let err = apply(no_claude, &blocks(vec![])).unwrap_err();
        assert!(err.iter().any(|p| p.contains("claude")), "{err:?}");
        let unclosed = "<!-- BEGIN GENERATED: docs-index adr -->\n- x\n";
        let err = apply(unclosed, &blocks(vec![])).unwrap_err();
        assert!(err.iter().any(|p| p.contains("END GENERATED")), "{err:?}");
        let unknown = "<!-- BEGIN GENERATED: docs-index nope -->\n<!-- END GENERATED -->\n";
        assert!(apply(unknown, &blocks(vec![])).is_err());
    }

    #[test]
    fn the_first_difference_names_the_line() {
        assert_eq!(first_difference("a\nb\n", "a\nb\n"), None);
        assert_eq!(
            first_difference("a\nb\n", "a\nc\n"),
            Some((2, "b".into(), "c".into()))
        );
        assert_eq!(
            first_difference("a\n", "a\nb\n"),
            Some((2, "(行なし)".into(), "b".into()))
        );
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn scan_reads_the_four_places_and_ignores_the_index_itself() {
        let t = tempfile::tempdir().unwrap();
        let docs = t.path();
        write(docs, "README.md", "# index\n");
        write(docs, "ops.md", "# Ops\n");
        write(docs, "adr/0001-a.md", "# ADR-0001 — A\n");
        write(docs, "agents/x.md", "# X\n");
        write(docs, "claude/y.md", "# Y\n");
        write(docs, "assets/pic.png", "binary");
        let m = scan(docs).unwrap();
        assert_eq!(m["root"], vec![doc("ops.md", "Ops")]);
        assert_eq!(m["adr"].len() + m["agents"].len() + m["claude"].len(), 3);
    }

    #[test]
    fn a_doc_without_h1_or_in_an_unknown_place_fails_the_scan() {
        let t = tempfile::tempdir().unwrap();
        let docs = t.path();
        write(docs, "no-title.md", "just text\n");
        write(docs, "new-dir/doc.md", "# New\n");
        write(docs, "agents/nested/deep.md", "# Deep\n");
        let err = scan(docs).unwrap_err();
        assert!(
            err.iter()
                .any(|p| p.contains("no-title.md") && p.contains("H1")),
            "{err:?}"
        );
        assert!(
            err.iter()
                .any(|p| p.contains("new-dir") && p.contains("節が無い")),
            "{err:?}"
        );
        assert!(err.iter().any(|p| p.contains("入れ子")), "{err:?}");
    }
}
