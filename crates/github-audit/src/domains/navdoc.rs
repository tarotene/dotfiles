//! nav-doc 検査(ADR-0033、charters ドメインの一部)。
//!
//! README / CONTRIBUTING / AGENTS.md / CLAUDE.md がディレクトリツリーや
//! 手書きのファイル一覧を持たないこと(repo-charter SKILL.md §4a)。転記
//! (他ファイルの内容の写し)は意味比較が要るので github-audit-triage の
//! LLM ノードに委ね、ここでは決定論的に判定できる 2 つだけを見る。

use crate::jq;

/// 節の一意な path 風トークン数がこれ以上なら「手書きの一覧」(実例での
/// 校正: 13 行のディレクトリ構造ブロックが 11、散文の ADR 索引が 5、正当な
/// 1-2 個の言及が 1-2)。
pub const NAV_DOC_PATH_THRESHOLD: usize = 4;

/// path 区切りが無くてもファイル参照に見える拡張子(素の `profile.yml` 等)。
const NAV_DOC_PATH_EXTENSIONS: [&str; 15] = [
    "md", "yml", "yaml", "typ", "toml", "json", "sh", "nix", "rs", "py", "ts", "js", "lock", "tsv",
    "bib",
];

/// fenced block 内の手描きツリーを示す罫線(├ └ │)。
fn is_tree_char(c: char) -> bool {
    matches!(c, '\u{251C}' | '\u{2514}' | '\u{2502}')
}

fn has_known_extension(tok: &str) -> bool {
    NAV_DOC_PATH_EXTENSIONS.iter().any(|ext| {
        tok.len() > ext.len() && tok.ends_with(ext) && {
            let dot = tok.len() - ext.len() - 1;
            tok.as_bytes()[dot] == b'.'
        }
    })
}

/// `^#{1,6}[[:space:]](.*)$` に一致すれば見出しテキスト(キャプチャ)。
pub(crate) fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    let c = rest.chars().next()?;
    if !jq::is_space(c) {
        return None;
    }
    Some(&rest[c.len_utf8()..])
}

/// bash: `nav_doc_path_tokens`。節本文から path 風トークン(一意)を拾う:
/// インラインコード `...` の中身、または fenced 行の最初の語のうち、空白を
/// 含まず、http(s):// で始まらず、`/` を含むか既知の拡張子で終わるもの。
/// 末尾の `/` は 1 つ落としてから一意化する。
pub fn nav_doc_path_tokens(body: &str) -> Vec<String> {
    let mut toks: Vec<String> = Vec::new();
    let mut infence = false;
    for line in body.split('\n') {
        if line.starts_with("```") {
            infence = !infence;
            continue;
        }
        if infence {
            // awk の split(/[[:space:]]+/) は先頭の空白で空の第 1 要素を作る。
            let first = line.split(jq::is_space).next().unwrap_or("");
            if !first.is_empty() {
                toks.push(first.to_string());
            }
            continue;
        }
        let mut s = line;
        while let Some(start) = s.find('`') {
            let rest = &s[start + 1..];
            match rest.find('`') {
                None => break,
                Some(0) => s = rest,
                Some(end) => {
                    let tok = &rest[..end];
                    if !tok.chars().any(jq::is_space) {
                        toks.push(tok.to_string());
                    }
                    s = &rest[end + 1..];
                }
            }
        }
    }
    let kept: Vec<String> = toks
        .into_iter()
        .filter(|t| !(t.starts_with("http://") || t.starts_with("https://")))
        .filter(|t| t.contains('/') || has_known_extension(t))
        .map(|t| t.strip_suffix('/').map(str::to_string).unwrap_or(t))
        .collect();
    jq::unique_strings(kept)
}

struct Marker {
    check: &'static str,
    start: usize,
    /// `end < start` は「何も守らない」(i64 で負も表す)。
    end: i64,
    malformed: bool,
}

const MARKER_PREFIX: &str = "<!-- nav-doc-exempt:";

fn is_marker(t: &str) -> bool {
    t.len() >= MARKER_PREFIX.len() + 3 && t.starts_with(MARKER_PREFIX) && t.ends_with("-->")
}

/// マーカー行なら `Some(検査名)`。検査名か理由が欠ける(malformed)なら
/// `Some(None)`、マーカー行でなければ `None`。
fn parse_marker(line: &str) -> Option<Option<&'static str>> {
    let t = jq::trim_space(line);
    if !is_marker(t) {
        return None;
    }
    let content = jq::trim_space(&t[MARKER_PREFIX.len()..t.len() - 3]);
    let Some((c, r)) = content.split_once(" — ") else {
        return Some(None);
    };
    let c = c.trim_end_matches(jq::is_space);
    let r = r.trim_start_matches(jq::is_space);
    Some(match c {
        "path-inventory" if !r.is_empty() => Some("path-inventory"),
        "tree-fence" if !r.is_empty() => Some("tree-fence"),
        _ => None,
    })
}

/// bash: `nav_doc_scan`。`file` はラベル(例 "README.md")。戻り値は finding
/// トークン: `nav-doc-tree-fence:<file>` / `nav-doc-path-inventory:<file>:
/// <heading>:<n>` / `nav-doc-exempt-unused:<file>:<check>` /
/// `nav-doc-exempt-malformed:<file>`(この順)。
///
/// `<!-- nav-doc-exempt: <check> — <reason> -->` は直後のブロック(次行が
/// fence を開くならその fence 全体、そうでなければ次の空行か見出しまでの
/// 連続行)を、その検査 1 つからだけ除外する。守るブロックが除外無しでも
/// drift しないなら unused(ESLint の reportUnusedDisableDirectives /
/// Ruff の RUF100 と同じ考え方)。
pub fn nav_doc_scan(file: &str, text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let n = lines.len();

    // 行ごとの節番号(節 0 は最初の見出しより前)と各節の見出し。fence 内の
    // '#' 行(シェルのコメント等)は見出しではない。
    let mut section_idx = vec![0usize; n];
    let mut section_heading: Vec<&str> = vec!["(intro)"];
    let mut cur = 0usize;
    let mut infence_hdr = false;
    for (k, line) in lines.iter().enumerate() {
        if line.starts_with("```") {
            infence_hdr = !infence_hdr;
        } else if !infence_hdr {
            if let Some(h) = heading_text(line) {
                cur += 1;
                section_heading.push(h);
            }
        }
        section_idx[k] = cur;
    }
    let n_sections = cur + 1;

    let mut markers: Vec<Marker> = Vec::new();
    for (k, line) in lines.iter().enumerate() {
        let Some(check) = parse_marker(line) else {
            continue;
        };
        let start = k + 1;
        let (end, malformed) = match check {
            Some(_) => {
                if start < n && lines[start].starts_with("```") {
                    let mut end = start;
                    for (j, l) in lines.iter().enumerate().skip(start + 1) {
                        if l.starts_with("```") {
                            end = j;
                            break;
                        }
                    }
                    (end as i64, false)
                } else {
                    let mut j = start;
                    while j < n {
                        let bl = lines[j];
                        if bl.is_empty() || heading_text(bl).is_some() {
                            break;
                        }
                        j += 1;
                    }
                    (j as i64 - 1, false)
                }
            }
            None => (start as i64 - 1, true),
        };
        markers.push(Marker {
            check: check.unwrap_or(""),
            start,
            end,
            malformed,
        });
    }

    // 有効なマーカーの守る範囲を検査種別ごとに空行にする。
    let mut lines_path: Vec<&str> = lines.clone();
    let mut lines_tree: Vec<&str> = lines.clone();
    for m in markers.iter().filter(|m| !m.malformed) {
        if m.end < m.start as i64 {
            continue;
        }
        for idx in m.start..=(m.end as usize) {
            if m.check == "path-inventory" {
                lines_path[idx] = "";
            } else {
                lines_tree[idx] = "";
            }
        }
    }

    let mut findings: Vec<String> = Vec::new();

    // tree-fence: 除外後の fence に罫線があれば手描きツリー(ファイル単位)。
    let mut infence = false;
    for l in &lines_tree {
        if l.starts_with("```") {
            infence = !infence;
            continue;
        }
        if infence && l.chars().any(is_tree_char) {
            findings.push(format!("nav-doc-tree-fence:{file}"));
            break;
        }
    }

    // path-inventory: 節ごとの一意トークン数を、除外後(drift の判定)と
    // 除外前(unused の判定の基準)の両方で数える。
    let mut section_raw_count = vec![0usize; n_sections];
    for (si, raw_count) in section_raw_count.iter_mut().enumerate() {
        let mut body_exempted = String::new();
        let mut body_raw = String::new();
        for k in (0..n).filter(|k| section_idx[*k] == si) {
            body_exempted.push_str(lines_path[k]);
            body_exempted.push('\n');
            body_raw.push_str(lines[k]);
            body_raw.push('\n');
        }
        let count_exempted = nav_doc_path_tokens(&body_exempted).len();
        if count_exempted >= NAV_DOC_PATH_THRESHOLD {
            findings.push(format!(
                "nav-doc-path-inventory:{file}:{}:{count_exempted}",
                section_heading[si]
            ));
        }
        *raw_count = nav_doc_path_tokens(&body_raw).len();
    }

    // マーカーの後始末: malformed(検査名か理由が欠ける)と unused。
    let mut any_malformed = false;
    for m in &markers {
        if m.malformed {
            any_malformed = true;
            continue;
        }
        if m.check == "tree-fence" {
            let has_tree = (m.start as i64..=m.end)
                .filter(|i| *i >= 0 && (*i as usize) < n)
                .any(|i| lines[i as usize].chars().any(is_tree_char));
            if !has_tree {
                findings.push(format!("nav-doc-exempt-unused:{file}:tree-fence"));
            }
        } else {
            let sec = if m.start < n { section_idx[m.start] } else { 0 };
            if section_raw_count[sec] < NAV_DOC_PATH_THRESHOLD {
                findings.push(format!("nav-doc-exempt-unused:{file}:path-inventory"));
            }
        }
    }
    if any_malformed {
        findings.push(format!("nav-doc-exempt-malformed:{file}"));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t = nav_doc_path_tokens(
            "See `a.md`, `b c`, `https://x/y`, `dir/`\n```\n  indented x\nlib/ # c\n```\n",
        );
        assert_eq!(t, vec!["a.md", "dir", "lib"]);
    }

    #[test]
    fn heading() {
        assert_eq!(heading_text("## Scope"), Some("Scope"));
        assert_eq!(heading_text("####### x"), None);
        assert_eq!(heading_text("#x"), None);
    }
}
