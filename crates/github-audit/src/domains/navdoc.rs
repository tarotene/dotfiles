//! nav-doc 検査(ADR-0033、charters ドメインの一部)。
//!
//! README / CONTRIBUTING / AGENTS.md / CLAUDE.md がディレクトリツリーや
//! 手書きのファイル一覧を持たないこと(repo-charter SKILL.md §4a)。転記
//! (他ファイルの内容の写し)は意味比較が要るので github-audit-triage の
//! LLM ノードに委ね、ここでは決定論的に判定できる 2 つだけを見る。

use crate::jq;

/// 節の一意な path 風トークン数がこれ以上なら「手書きの一覧」。
///
/// 校正記録:
/// - 初版(ADR-0033): 実例で、13 行のディレクトリ構造ブロックが 11、散文の ADR 索引が 5、
///   正当な 1-2 個の言及が 1-2。節のどこにあっても数えていた。
/// - 2026-10-27(#932): 規約集としての AGENTS.md(人と AI の両方に向けた約 860 行)で 12 節が
///   該当し、最大 53 個。規則が実在のファイルを名指しする散文(「このファイルは取り込み時に
///   必ず直す」)と、手書きの一覧が区別されていなかった。数えるのを「一覧の形をした行」
///   (リスト項目・表の行・fence の行で、行頭か行の大半がパスのもの)に絞った。散文の中の
///   名指しは数えない。閾値の 4 は変えない(一覧の形に絞った上でも、1-3 個の列挙は
///   正当な参照の範囲)。この repo の AGENTS.md は、絞る前は `Three-layer model` 節が 6 で
///   1 件、絞ったあとは 0 件(README.md の `Scope` 節の 4 は一覧でなく散文なので、同じく
///   0 件)。
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

/// 空白を含まず、http(s):// で始まらず、`/` を含むか既知の拡張子で終わるもの。
fn is_path_like(tok: &str) -> bool {
    let is_url = tok.starts_with("http://") || tok.starts_with("https://");
    !tok.chars().any(jq::is_space) && !is_url && (tok.contains('/') || has_known_extension(tok))
}

/// 行のインラインコード `...` の中身(閉じていないものは打ち切る。空は飛ばす)。
fn inline_code_spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut s = line;
    while let Some(start) = s.find('`') {
        let rest = &s[start + 1..];
        match rest.find('`') {
            None => break,
            Some(0) => s = rest,
            Some(end) => {
                spans.push(&rest[..end]);
                s = &rest[end + 1..];
            }
        }
    }
    spans
}

/// リスト項目なら、行頭の記号(`- ` `* ` `+ ` `1. ` `1) `、タスクの `[ ] `)を落とした中身。
fn list_item_content(line: &str) -> Option<&str> {
    let t = line.trim_start_matches([' ', '\t']);
    let rest = ["- ", "* ", "+ "]
        .iter()
        .find_map(|m| t.strip_prefix(m))
        .or_else(|| {
            let digits = t.bytes().take_while(u8::is_ascii_digit).count();
            if (1..=9).contains(&digits) {
                t[digits..]
                    .strip_prefix(". ")
                    .or_else(|| t[digits..].strip_prefix(") "))
            } else {
                None
            }
        })?;
    let rest = rest.trim_start_matches([' ', '\t']);
    Some(
        ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find_map(|m| rest.strip_prefix(m))
            .unwrap_or(rest),
    )
}

/// 表の行なら、行頭の `|` を落とした中身。
fn table_row_content(line: &str) -> Option<&str> {
    line.trim_start_matches([' ', '\t']).strip_prefix('|')
}

/// 先頭の強調記号(`*` `_`)を落とした先頭が、path 風のインラインコードか。
fn starts_with_path_span(content: &str) -> bool {
    let t = content.trim_start_matches([' ', '\t', '*', '_']);
    t.starts_with('`')
        && inline_code_spans(t)
            .first()
            .is_some_and(|s| is_path_like(s))
}

/// 一覧の形をした行(リスト項目・表の行)から、path 風のトークンを拾う。行頭が path のもの
/// (表は最初のセル)か、行の大半(空白を除いた文字の半数超)が path のインラインコードの
/// ものだけが対象。散文の途中の名指しは、行頭でも大半でもないので拾わない(#932)。
fn list_line_tokens(line: &str) -> Vec<String> {
    let (content, head) = if let Some(c) = list_item_content(line) {
        (c, c)
    } else if let Some(c) = table_row_content(line) {
        (c, c.split('|').next().unwrap_or(""))
    } else {
        return Vec::new();
    };
    let paths: Vec<&str> = inline_code_spans(content)
        .into_iter()
        .filter(|t| is_path_like(t))
        .collect();
    if paths.is_empty() {
        return Vec::new();
    }
    let path_chars: usize = paths.iter().map(|t| t.chars().count() + 2).sum();
    let total_chars = content
        .chars()
        .filter(|c| !jq::is_space(*c) && *c != '|')
        .count();
    if starts_with_path_span(head) || path_chars * 2 > total_chars {
        paths.into_iter().map(str::to_string).collect()
    } else {
        Vec::new()
    }
}

/// bash: `nav_doc_path_tokens`(2026-10-27 に #932 で「一覧の形をした行」に絞った)。節本文から
/// path 風トークン(一意)を拾う:
/// - fenced 行の最初の語(行頭の語なので、fence は全行が対象)
/// - リスト項目・表の行のインラインコード(行頭か行の大半が path のものだけ。
///   `list_line_tokens`)
///
/// 散文(リストでも表でもない行)のインラインコードは数えない。どちらも、空白を含まず、
/// http(s):// で始まらず、`/` を含むか既知の拡張子で終わるもの。末尾の `/` は 1 つ落として
/// から一意化する。
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
            if is_path_like(first) {
                toks.push(first.to_string());
            }
            continue;
        }
        toks.extend(list_line_tokens(line));
    }
    let kept: Vec<String> = toks
        .into_iter()
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
    fn tokens_in_list_shaped_lines() {
        let t = nav_doc_path_tokens(
            "- `a.md` — what it is\n* `dir/` and notes\n1. `x/y`\n- [ ] `todo.md`\n| `t/u` | desc |\n```\n  indented x\nlib/ # c\n```\n",
        );
        assert_eq!(t, vec!["a.md", "dir", "lib", "t/u", "todo.md", "x/y"]);
    }

    /// #932: 規則が実在のファイルを名指しする散文は、一覧ではない。
    #[test]
    fn prose_mentions_are_not_counted() {
        let prose = "See `a.md`, `b.yml`, `c.toml`, `d.json` for the old scheme.\n\
                     `p/q` `r/s` `t/u` `v/w` are named here.\n\
                     - Run `cargo test` and then fix `crates/x/src/lib.rs` if it fails, see also `docs/y.md`, `z.toml`.\n\
                     | Layer | Owns | Example |\n|---|---|---|\n| **User environment** | shell, git | see `home/modules/git.nix` for details of what is owned |\n";
        assert!(
            nav_doc_path_tokens(prose).is_empty(),
            "{:?}",
            nav_doc_path_tokens(prose)
        );
    }

    #[test]
    fn list_lines_led_by_a_path_or_mostly_paths_are_counted() {
        // 行頭が path(説明が長くても数える)。
        assert_eq!(
            nav_doc_path_tokens(
                "- **`a/b.md`** — a very long description that goes on and on and on\n"
            ),
            vec!["a/b.md"]
        );
        // 行頭ではないが、行の大半が path。
        assert_eq!(
            nav_doc_path_tokens("- `a/b` `c/d` `e/f`\n- and `g/h` `i/j` `k/l`\n"),
            vec!["a/b", "c/d", "e/f", "g/h", "i/j", "k/l"]
        );
        // 行頭でも大半でもない。
        assert!(
            nav_doc_path_tokens("- the long sentence explains that `a/b` matters here\n")
                .is_empty()
        );
        // 表は最初のセルが path のものを数える。
        assert_eq!(
            nav_doc_path_tokens("| `crates/x` | the x crate, a long description of it |\n"),
            vec!["crates/x"]
        );
    }

    /// 校正の結果: 手書きの一覧は、これまでどおり閾値で検出する。
    #[test]
    fn a_hand_written_inventory_still_trips_the_threshold_but_prose_does_not() {
        let list = "# T\n\n## Files\n\n- `a.md` — one\n- `b.md` — two\n- `c.md` — three\n- `d.md` — four\n";
        assert_eq!(
            nav_doc_scan("AGENTS.md", list),
            vec!["nav-doc-path-inventory:AGENTS.md:Files:4"]
        );
        let prose = "# T\n\n## Rules\n\nRead `a.md`, `b.md`, `c.md`, `d.md` and `e.md` before you change anything.\n";
        assert!(nav_doc_scan("AGENTS.md", prose).is_empty());
        // 規約集(AGENTS.md)の節が、散文で 53 個の実在のパスを名指ししても検出しない。
        let many: String = (0..53)
            .map(|i| format!("`dir/f{i}.md` "))
            .collect::<String>();
        assert!(nav_doc_scan(
            "AGENTS.md",
            &format!("## Rules\n\nAlways fix {many}when you import.\n")
        )
        .is_empty());
    }

    #[test]
    fn heading() {
        assert_eq!(heading_text("## Scope"), Some("Scope"));
        assert_eq!(heading_text("####### x"), None);
        assert_eq!(heading_text("#x"), None);
    }
}
