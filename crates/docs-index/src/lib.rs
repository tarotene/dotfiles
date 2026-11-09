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

/// ADR の見出しが宣言する、ほかの ADR との関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Supersedes,
    Amends,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Supersedes => "置換",
            Kind::Amends => "改訂",
        }
    }
}

/// 索引に載る 1 つの文書。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// `docs/` からの相対パス(例: `adr/0001-x.md`)。
    pub rel: String,
    /// H1 の文字列。
    pub title: String,
    /// この ADR が宣言する、ほかの ADR への関係(`- Supersedes:` と `- Amends:`)。ADR 以外は空。
    pub relations: Vec<(Kind, u64)>,
}

impl Doc {
    pub fn new(rel: &str, title: &str) -> Doc {
        Doc {
            rel: rel.to_string(),
            title: title.to_string(),
            relations: Vec::new(),
        }
    }
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

/// ファイル名の番号(`0033-x.md` なら 33)。番号で始まらなければ `None`。
fn file_number(rel: &str) -> Option<u64> {
    adr_number(rel).parse().ok()
}

/// ADR の見出し(最初の `## ` より前)の `- Supersedes:` と `- Amends:` から、対象の ADR 番号を読む。
/// 値は次の `- ` の行か空行までの続きの行を含み、`ADR-<数字>` の並びを拾う(重複と自分自身は除く)。
/// 値が `なし` で始まるものと、ADR の番号を含まないもの(skill のパスなど)は、関係を持たない。
pub fn adr_relations(md: &str, own: Option<u64>) -> Vec<(Kind, u64)> {
    let head = md.split("\n## ").next().unwrap_or(md);
    let lines: Vec<&str> = head.lines().collect();
    let mut out: Vec<(Kind, u64)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let kind = if lines[i].starts_with("- Supersedes:") {
            Some((Kind::Supersedes, "- Supersedes:"))
        } else if lines[i].starts_with("- Amends:") {
            Some((Kind::Amends, "- Amends:"))
        } else {
            None
        };
        let Some((kind, prefix)) = kind else {
            i += 1;
            continue;
        };
        let mut value = lines[i][prefix.len()..].trim().to_string();
        i += 1;
        while i < lines.len()
            && !lines[i].trim().is_empty()
            && !lines[i].starts_with("- ")
            && !lines[i].starts_with('#')
        {
            value.push(' ');
            value.push_str(lines[i].trim());
            i += 1;
        }
        if value.starts_with("なし") {
            continue;
        }
        let mut rest = value.as_str();
        while let Some(at) = rest.find("ADR-") {
            rest = &rest[at + 4..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = digits.parse::<u64>() {
                if Some(n) != own && !out.contains(&(kind, n)) {
                    out.push((kind, n));
                }
            }
        }
    }
    out
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
                Some(title) => {
                    let relations = if dir == "adr" {
                        adr_relations(&md, file_number(&rel))
                    } else {
                        Vec::new()
                    };
                    out.push(Doc {
                        rel,
                        title,
                        relations,
                    })
                }
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
    problems.extend(check_relations(&out["adr"]));
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

/// 並べ替えと比較に使う番号。仮番号(0)は、どの番号より後に出る ADR として扱う。
fn effective_number(rel: &str) -> u64 {
    match file_number(rel) {
        Some(0) | None => u64::MAX,
        Some(n) => n,
    }
}

/// ADR の関係の検査: 対象の ADR が実在し、宣言する側の番号が対象より大きい
/// (後から出た決定が、前の決定を置き換える・改める)。
pub fn check_relations(adrs: &[Doc]) -> Vec<String> {
    let mut problems = Vec::new();
    for d in adrs {
        for (kind, target) in &d.relations {
            let exists = adrs.iter().any(|t| file_number(&t.rel) == Some(*target));
            if !exists {
                problems.push(format!(
                    "docs/{}: {} の対象 ADR-{target} が docs/adr/ に無い",
                    d.rel,
                    kind.label()
                ));
            } else if effective_number(&d.rel) <= *target {
                problems.push(format!(
                    "docs/{}: {} の対象 ADR-{target} より大きい番号でない(後から出た決定が前の決定を改める)",
                    d.rel,
                    kind.label()
                ));
            }
        }
    }
    problems
}

/// `target` を置き換える・改める ADR を、索引の行の後ろに付ける文字列。無ければ空。
/// 古い ADR は accepted 後に書き換えない(immutable)ので、後方のリンクは、新しい側の宣言から導く。
fn backlink_suffix(adrs: &[Doc], target: u64) -> String {
    let mut groups: Vec<String> = Vec::new();
    for kind in [Kind::Supersedes, Kind::Amends] {
        let mut sources: Vec<&Doc> = adrs
            .iter()
            .filter(|s| s.relations.contains(&(kind, target)))
            .collect();
        sources.sort_by_key(|s| adr_sort_key(&s.rel));
        if !sources.is_empty() {
            let names: Vec<String> = sources
                .iter()
                .map(|s| format!("ADR-{}", adr_number(&s.rel)))
                .collect();
            groups.push(format!("{} {}", kind.label(), names.join(", ")));
        }
    }
    if groups.is_empty() {
        String::new()
    } else {
        format!(" (後続: {})", groups.join("; "))
    }
}

/// 節 1 つ分の行。最後の行にも改行を付ける。
pub fn render_block(id: &str, docs: &[Doc]) -> String {
    let mut docs: Vec<&Doc> = docs.iter().collect();
    let mut out = String::new();
    if id == "adr" {
        docs.sort_by_key(|d| adr_sort_key(&d.rel));
        let all: Vec<Doc> = docs.iter().map(|d| (*d).clone()).collect();
        for d in docs {
            let suffix = file_number(&d.rel)
                .map(|n| backlink_suffix(&all, n))
                .unwrap_or_default();
            out.push_str(&format!(
                "- [ADR-{}]({}) — {}{}\n",
                adr_number(&d.rel),
                d.rel,
                adr_title(&d.title),
                suffix
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
        Doc::new(rel, title)
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
        // 仮番号(0 が 4 つ)は、起草中の参照を禁じる adr-number-check に当たらないよう、実行時に組み立てる。
        let draft = "0".repeat(4);
        let docs = vec![
            doc(
                &format!("adr/{draft}-new-decision.md"),
                &format!("ADR-{draft} — new"),
            ),
            doc("adr/872-pipeline-map.md", "ADR-872 — map"),
            doc("adr/0033-nav.md", "ADR-0033 — nav"),
            doc("adr/1400-layers.md", "ADR-1400 — layers"),
        ];
        let out = render_block("adr", &docs);
        let order: Vec<&str> = out.lines().map(|l| l.split(']').next().unwrap()).collect();
        assert_eq!(
            order,
            [
                "- [ADR-0033",
                "- [ADR-872",
                "- [ADR-1400",
                &format!("- [ADR-{draft}")
            ]
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

    #[test]
    fn adr_relations_read_the_header_only_and_skip_none_self_and_non_adr_targets() {
        let md = "# ADR-0033 — x\n\n- Status: Accepted\n\
- Amends: ADR-0016(Decision 5 の適用範囲を広げる。\n  ADR-0017 も同じ)。ADR-0016 を二度書いても 1 つ\n\
- Supersedes: なし。ADR-0001 は破らない\n- Issue: #1\n\n## Context\n- Amends: ADR-9999(本文の中は読まない)\n";
        assert_eq!(
            adr_relations(md, Some(33)),
            vec![(Kind::Amends, 16), (Kind::Amends, 17)]
        );
        let skill = "# ADR-0027 — x\n- Supersedes: `config/agents/skills/stacked-pr/SKILL.md` §1 を改める\n- Amends: ADR-0027 自身は除く\n\n## Context\n";
        assert_eq!(adr_relations(skill, Some(27)), vec![]);
    }

    fn rel(kind: Kind, target: u64) -> (Kind, u64) {
        (kind, target)
    }

    #[test]
    fn back_links_are_derived_from_the_declaring_side_and_grouped_by_kind() {
        let mut new = doc("adr/0033-nav.md", "ADR-0033 — nav");
        new.relations = vec![rel(Kind::Amends, 16)];
        let mut newer = doc("adr/1000-later.md", "ADR-1000 — later");
        newer.relations = vec![rel(Kind::Supersedes, 16), rel(Kind::Amends, 33)];
        let old = doc("adr/0016-canon.md", "ADR-0016 — canon");
        let out = render_block("adr", &[newer, new, old]);
        assert!(
            out.contains(
                "- [ADR-0016](adr/0016-canon.md) — canon (後続: 置換 ADR-1000; 改訂 ADR-0033)\n"
            ),
            "{out}"
        );
        assert!(
            out.contains("- [ADR-0033](adr/0033-nav.md) — nav (後続: 改訂 ADR-1000)\n"),
            "{out}"
        );
        // 何にも宣言されない ADR には何も付かない。
        assert!(
            out.contains("- [ADR-1000](adr/1000-later.md) — later\n"),
            "{out}"
        );
    }

    #[test]
    fn relations_to_a_missing_or_not_earlier_adr_fail() {
        let mut a = doc("adr/0010-a.md", "ADR-0010 — a");
        a.relations = vec![rel(Kind::Amends, 99)];
        let mut b = doc("adr/0020-b.md", "ADR-0020 — b");
        b.relations = vec![rel(Kind::Supersedes, 30)];
        let c = doc("adr/0030-c.md", "ADR-0030 — c");
        let problems = check_relations(&[a, b, c]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("ADR-99") && problems[0].contains("無い"),
            "{problems:?}"
        );
        assert!(
            problems[1].contains("ADR-30") && problems[1].contains("大きい番号"),
            "{problems:?}"
        );
        // 仮番号(0)は最後に出る決定として扱うので、どの対象より後になる。
        let draft = format!("adr/{}-new.md", "0".repeat(4));
        let mut d = doc(&draft, "draft");
        d.relations = vec![rel(Kind::Amends, 10)];
        assert!(check_relations(&[d, doc("adr/0010-a.md", "a")]).is_empty());
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
