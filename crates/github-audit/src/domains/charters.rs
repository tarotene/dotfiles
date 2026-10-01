//! charters ドメイン(ADR-0013 目的/topics + ADR-0016 README スキーマ・
//! allowlist・routing + ADR-0017 CONTRIBUTING スキーマ・CJK + ADR-0033 nav-doc)。
//!
//! どの項目も文字列の有無・一致だけで決まる(LLM を呼ばない)。弱いが存在する
//! litmus は通す — 質の判定は repo-charter スキルの対話の仕事。

use super::navdoc::nav_doc_scan;
use crate::jq;
use crate::model::{blob_text, Finding, RepoGql, TreeEntry};
use regex::Regex;
use std::sync::OnceLock;

/// README の必須見出し(この相対順、ADR-0016)。Background は任意で Install より前。
pub const REQUIRED_README_HEADINGS: [&str; 5] =
    ["Install", "Usage", "Scope", "Development", "License"];
pub const ALLOWED_README_HEADINGS: [&str; 6] = [
    "Background",
    "Install",
    "Usage",
    "Scope",
    "Development",
    "License",
];
/// CONTRIBUTING.md(ADR-0017)。"Issue litmus" は廃止語彙なので許可集合に無い。
pub const REQUIRED_CONTRIBUTING_HEADINGS: [&str; 2] = ["Issues", "Pull requests"];
pub const ALLOWED_CONTRIBUTING_HEADINGS: [&str; 3] = ["Issues", "Pull requests", "Expectations"];
pub const ROOT_ALLOWLIST_EXACT: [&str; 5] = [
    "README.md",
    "CONTRIBUTING.md",
    "CHANGELOG.md",
    "AGENTS.md",
    "CLAUDE.md",
];
/// README/CONTRIBUTING は英語原文 1 本(ADR-0016 Decision 4)なので、CJK が
/// この下限以上あればそれだけで drift(ADR-0017)。
pub const CJK_PRESENCE_THRESHOLD: usize = 5;

/// bash: `extract_purpose`。最初の `# ` 見出しの後の、最初の非空段落。
pub fn extract_purpose(readme: &str) -> String {
    let mut seen_h1 = false;
    let mut started = false;
    let mut para = String::new();
    for line in readme.split('\n') {
        if line.starts_with("# ") && !seen_h1 {
            seen_h1 = true;
            continue;
        }
        if !seen_h1 {
            continue;
        }
        let blank = line.chars().all(jq::is_space);
        if !started && blank {
            continue;
        }
        started = true;
        if blank || line.starts_with('#') {
            break;
        }
        if para.is_empty() {
            para = line.to_string();
        } else {
            para.push(' ');
            para.push_str(line);
        }
    }
    para
}

/// bash: `first_sentence`(`grep -oP '^.*?[.。]'`、無ければ全体)。
pub fn first_sentence(para: &str) -> String {
    let line = para.split('\n').next().unwrap_or("");
    match line.find(['.', '。']) {
        Some(i) => {
            let end = i + line[i..].chars().next().map_or(1, char::len_utf8);
            line[..end].to_string()
        }
        None => para.to_string(),
    }
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]\n]*)\]\([^)\n]*\)").expect("valid regex"))
}

/// bash: `normalize_text`。強調(`**` / `__`)とリンク記法を剥がし、空白を
/// 1 つに潰し、両端を切り、末尾の `.` と `。` を 1 つずつ落とす。
pub fn normalize_text(s: &str) -> String {
    let s = s.replace("**", "").replace("__", "");
    let s = link_re().replace_all(&s, "$1");
    let s = jq::sh_trim(&s);
    // tr -s '[:space:]' ' ' <<<"$s"(here-string の改行も空白になる)
    let mut squeezed = String::with_capacity(s.len() + 1);
    for c in s.chars().chain(['\n']) {
        let c = if jq::is_ascii_space(c) { ' ' } else { c };
        if c == ' ' && squeezed.ends_with(' ') {
            continue;
        }
        squeezed.push(c);
    }
    let t = jq::trim_space(&squeezed);
    let t = t.strip_suffix('.').unwrap_or(t);
    let t = t.strip_suffix('。').unwrap_or(t);
    t.to_string()
}

fn code_span_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`[^`\n]*`").expect("valid regex"))
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^ )\n]+").expect("valid regex"))
}

/// bash: `strip_for_lang`。fenced block・インラインコード・URL を落とす。
pub fn strip_for_lang(text: &str) -> String {
    let mut out = Vec::new();
    let mut infence = false;
    for line in text.split('\n') {
        if line.starts_with("```") {
            infence = !infence;
            continue;
        }
        if infence {
            continue;
        }
        let l = code_span_re().replace_all(line, "");
        out.push(url_re().replace_all(&l, "").into_owned());
    }
    out.join("\n")
}

/// bash: `count_cjk`。文字クラスは bash 版の
/// `[\x{3040}-\x{30FF}\x{4E00}\x{9FFF}\x{3400}-\x{4DBF}]` そのまま —
/// `\x{4E00}\x{9FFF}` は範囲ではなく 2 文字なので、CJK 統合漢字の大半は
/// 数えられない(bash 版の挙動をそのまま移している)。
pub fn count_cjk(text: &str) -> usize {
    text.chars()
        .filter(|c| {
            matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}' | '\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
        })
        .count()
}

/// bash: `is_cjk_present`。
pub fn is_cjk_present(text: &str) -> bool {
    count_cjk(&strip_for_lang(text)) >= CJK_PRESENCE_THRESHOLD
}

/// bash: `doc_headings`。fenced block の外の `## X` の X(文書順)。
pub fn doc_headings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut infence = false;
    for line in text.split('\n') {
        if line.starts_with("```") {
            infence = !infence;
            continue;
        }
        if infence {
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            out.push(h.to_string());
        }
    }
    out
}

/// `IFS=, echo "${arr[*]}"`。bash の echo は引数全体が `-n`/`-e`/`-E` の
/// 組み合わせならオプションとして食う。
fn echo_join(items: &[String]) -> String {
    let s = items.join(",");
    let is_opt =
        s.len() > 1 && s.starts_with('-') && s[1..].chars().all(|c| matches!(c, 'n' | 'e' | 'E'));
    if is_opt {
        String::new()
    } else {
        s
    }
}

/// 見出しスキーマ: (必須が揃い順序どおりか, 許可外の見出し)。
fn heading_schema(text: &str, required: &[&str], allowed: &[&str]) -> (bool, Vec<String>) {
    let headings: Vec<String> = doc_headings(text)
        .into_iter()
        .filter(|h| !h.is_empty())
        .collect();
    let mut stray = Vec::new();
    let mut present_required: Vec<&str> = Vec::new();
    for h in &headings {
        if !allowed.contains(&h.as_str()) {
            stray.push(h.clone());
        }
        for a in required {
            if h == a {
                present_required.push(a);
            }
        }
    }
    // `"${present_required[*]}" == "${REQUIRED[*]}"`(空白連結の文字列比較)
    (present_required.join(" ") == required.join(" "), stray)
}

fn grep_line(text: &str, f: impl Fn(&str) -> bool) -> bool {
    text.split('\n').any(f)
}

/// bash: `judge_charters`。`topic_count` は `repositoryTopics` の件数。
pub fn judge_charters(description: &str, topic_count: usize, gql: &RepoGql) -> Finding {
    let readme = blob_text(&gql.readme);
    let contributing = blob_text(&gql.contributing);
    let claude_md = blob_text(&gql.claude_md);
    let agents_md = blob_text(&gql.agents_md);
    let mut missing: Vec<String> = Vec::new();

    if readme.is_empty() {
        missing.push("readme-missing".into());
    } else {
        let purpose = extract_purpose(&readme);
        if purpose.is_empty() {
            missing.push("no-purpose-paragraph".into());
        } else {
            let sentence = first_sentence(&purpose);
            if description.is_empty() || normalize_text(&sentence) != normalize_text(description) {
                missing.push("purpose-mismatch".into());
            }
        }
        let (ok, stray) =
            heading_schema(&readme, &REQUIRED_README_HEADINGS, &ALLOWED_README_HEADINGS);
        if !ok {
            missing.push("readme-schema-incomplete-or-out-of-order".into());
        }
        if !stray.is_empty() {
            missing.push(format!("readme-stray-heading:{}", echo_join(&stray)));
        }
        if grep_line(&readme, |l| {
            l.strip_prefix("## Issue litmus")
                .is_some_and(|r| r.is_empty() || r.starts_with(jq::is_space))
        }) {
            missing.push("litmus-not-migrated".into());
        }
        if is_cjk_present(&readme) {
            missing.push("readme-cjk-present".into());
        }
        missing.extend(nav_doc_scan("README.md", &readme));
    }

    if contributing.is_empty() {
        missing.push("contributing-missing".into());
    } else {
        let (ok, stray) = heading_schema(
            &contributing,
            &REQUIRED_CONTRIBUTING_HEADINGS,
            &ALLOWED_CONTRIBUTING_HEADINGS,
        );
        if !ok {
            missing.push("contributing-schema-incomplete-or-out-of-order".into());
        }
        if !stray.is_empty() {
            missing.push(format!("contributing-stray-heading:{}", echo_join(&stray)));
        }
        if is_cjk_present(&contributing) {
            missing.push("contributing-cjk-present".into());
        }
        missing.extend(nav_doc_scan("CONTRIBUTING.md", &contributing));
    }

    missing.extend(nav_doc_scan("AGENTS.md", &agents_md));
    missing.extend(nav_doc_scan("CLAUDE.md", &claude_md));

    if topic_count < 1 {
        missing.push("no-topics".into());
    }

    // ルート直下の *.md allowlist(ADR-0016)。
    let root: &[TreeEntry] = gql
        .root_tree
        .as_ref()
        .and_then(|t| t.entries.as_deref())
        .unwrap_or(&[]);
    let stray_root: Vec<String> = root
        .iter()
        .filter(|e| e.kind.as_deref() == Some("blob"))
        .map(|e| e.name.clone().unwrap_or_else(|| "null".into()))
        .filter(|name| !name.is_empty() && name.ends_with(".md"))
        .filter(|name| {
            !ROOT_ALLOWLIST_EXACT.contains(&name.as_str()) && !name.starts_with("LICENSE")
        })
        .collect();
    if !stray_root.is_empty() {
        missing.push(format!(
            "root-doc-not-allowlisted:{}",
            echo_join(&stray_root)
        ));
    }

    // CLAUDE.md の router 検査(CLAUDE.md があるときだけ)。
    if !claude_md.is_empty() {
        let routed = grep_line(&claude_md, |l| {
            l.strip_prefix("@AGENTS.md")
                .is_some_and(|r| r.chars().all(jq::is_space))
        });
        if !routed {
            missing.push("claude-md-not-routed".into());
        } else if agents_md.is_empty() {
            missing.push("claude-md-routed-but-agents-md-missing".into());
        }
    }

    // skills の routing: .claude/skills 直下は ../.agents/skills/<name> への
    // symlink であること。#259: GraphQL の TreeEntry.mode は 10 進(40960)、
    // REST の git/trees は 8 進文字列("120000")で同じ mode を返すので両方受ける。
    if let Some(entries) = gql
        .claude_skills_dir
        .as_ref()
        .and_then(|t| t.entries.as_ref())
    {
        let stray_skills: Vec<String> = entries
            .iter()
            .filter(|e| {
                let mode = match &e.mode {
                    None => String::new(),
                    Some(v) if !jq::truthy(Some(v)) => String::new(),
                    Some(v) => jq::raw(v),
                };
                mode != "120000" && mode != "40960"
            })
            .map(|e| e.name.clone().unwrap_or_else(|| "null".into()))
            .collect();
        if !stray_skills.is_empty() {
            missing.push(format!("skills-not-routed:{}", echo_join(&stray_skills)));
        }
    }

    Finding::ok_or_drifted(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_and_sentence() {
        let p = extract_purpose("# x\n\nFirst. Second\nline\n\n## Install");
        assert_eq!(p, "First. Second line");
        assert_eq!(first_sentence(&p), "First.");
        assert_eq!(first_sentence("日本語。続き"), "日本語。");
        assert_eq!(first_sentence("no period"), "no period");
    }

    #[test]
    fn normalize() {
        assert_eq!(
            normalize_text("A **bold** [link](u)  text."),
            "A bold link text"
        );
        assert_eq!(normalize_text("  x\t y。"), "x y");
    }

    #[test]
    fn cjk_class_matches_bash() {
        // ひらがな 5 文字は数える、漢字(4E01)は数えない。
        assert!(is_cjk_present("あいうえお"));
        assert!(!is_cjk_present("丁丁丁丁丁"));
        assert!(!is_cjk_present("```\nあいうえお\n```"));
    }

    #[test]
    fn echo_option_swallow() {
        assert_eq!(echo_join(&["-n".into()]), "");
        assert_eq!(echo_join(&["-n".into(), "x".into()]), "-n,x");
    }
}
