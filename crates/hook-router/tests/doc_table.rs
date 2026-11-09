//! ルータの規則名の集合(`rules.rs`の `RULES`)と、`docs/agents/hook-router.md` の「載せるもの」の表を
//! 突き合わせる。規則名の集合の正本は `rules.rs` で、文書の表は同じ集合を持つ。件数は文書に書かない。
//!
//! 検査は純粋関数([`problems`])で、実際の文書への適用は [`the_doc_table_matches_the_router_rules`] が行う。
//! 負例の fixture は、検査が実際に落ちることを確かめる(文書の実体には依存しない)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use hook_router::RULES;

const SECTION: &str = "## 載せるもの";
const HEADER: &str = "| イベント | 検査 |";

/// 規則側の期待: イベント名 → 規則名の集合。
type Expected = BTreeMap<String, BTreeSet<String>>;

fn expected_from_rules() -> Expected {
    let mut out = Expected::new();
    for r in RULES.iter() {
        out.entry(r.event.name().to_string())
            .or_default()
            .insert(r.node.to_string());
    }
    out
}

/// 検査名として許す綴り(小文字 ASCII・数字・`-`)。pipeline-map の node id と同じ。
fn is_node_id(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 「30 件」のような、数字 + 「件」。
fn has_count(line: &str) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == '件' && i > 0 {
            let mut j = i;
            while j > 0 && chars[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                return Some(chars[j..=i].iter().collect());
            }
            // 「 30 件」のように空白を挟む形。
            if chars[i - 1] == ' ' {
                let mut k = i - 1;
                while k > 0 && chars[k - 1] == ' ' {
                    k -= 1;
                }
                let mut j = k;
                while j > 0 && chars[j - 1].is_ascii_digit() {
                    j -= 1;
                }
                if j < k {
                    return Some(format!("{} 件", chars[j..k].iter().collect::<String>()));
                }
            }
        }
    }
    None
}

/// 表と期待の食い違いを、メッセージの一覧で返す。空なら一致。行番号は `md` の 1 から数えた行。
fn problems(expected: &Expected, md: &str) -> Vec<String> {
    let at = |line: usize, msg: String| format!("docs/agents/hook-router.md:{line}: {msg}");
    let lines: Vec<&str> = md.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim_end() == SECTION) else {
        return vec![format!("見出し「{SECTION}」が無い")];
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map_or(lines.len(), |p| start + 1 + p);
    let Some(header) = (start + 1..end).find(|&i| lines[i].trim() == HEADER) else {
        return vec![at(start + 1, format!("表(`{HEADER}`)が無い"))];
    };

    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate().take(header).skip(start + 1) {
        if let Some(count) = has_count(line) {
            out.push(at(
                i + 1,
                format!("表の前の文に件数(「{count}」)がある。件数は書かない(rules.rs が正本)"),
            ));
        }
    }

    let mut rows: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    for (i, raw) in lines.iter().enumerate().take(end).skip(header + 2) {
        let line = raw.trim();
        if !line.starts_with('|') {
            break;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 4 {
            out.push(at(i + 1, "表の行が 2 列でない".to_string()));
            continue;
        }
        let event = cells[1].to_string();
        if !expected.contains_key(&event) {
            out.push(at(i + 1, format!("未知のイベント: {event}")));
            continue;
        }
        let names: Vec<String> = cells[2].split('、').map(|s| s.trim().to_string()).collect();
        for n in &names {
            if !is_node_id(n) {
                out.push(at(
                    i + 1,
                    format!("検査名 {n:?} が node id の綴りでない(区切りは「、」)"),
                ));
            }
        }
        rows.insert(event, (i + 1, names));
    }

    for (event, want) in expected {
        let Some((line, names)) = rows.get(event) else {
            out.push(at(header + 1, format!("行が無い: {event}")));
            continue;
        };
        let mut seen = BTreeSet::new();
        let dup: Vec<&str> = names
            .iter()
            .filter(|n| !seen.insert(n.as_str()))
            .map(String::as_str)
            .collect();
        if !dup.is_empty() {
            out.push(at(*line, format!("{event} の表に重複: {}", dup.join(", "))));
        }
        let have: BTreeSet<String> = names.iter().cloned().collect();
        let missing: Vec<&str> = want.difference(&have).map(String::as_str).collect();
        let extra: Vec<&str> = have.difference(want).map(String::as_str).collect();
        if !missing.is_empty() {
            out.push(at(
                *line,
                format!(
                    "{event} の表に無い規則: {}(rules.rs に載っている。表に足す)",
                    missing.join(", ")
                ),
            ));
        }
        if !extra.is_empty() {
            out.push(at(
                *line,
                format!(
                    "{event} の表にあるが rules.rs に無い: {}(表から消すか、rules.rs に載せる)",
                    extra.join(", ")
                ),
            ));
        }
    }
    out
}

#[test]
fn the_doc_table_matches_the_router_rules() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/agents/hook-router.md");
    let md = std::fs::read_to_string(&path).expect("read hook-router.md");
    let expected = expected_from_rules();
    // 番兵: 空の期待を読んで「一致」で通らない。
    for event in ["PreToolUse", "PostToolUse"] {
        assert!(
            expected.get(event).is_some_and(|s| !s.is_empty()),
            "rules.rs に {event} の規則が無い"
        );
    }
    let found = problems(&expected, &md);
    assert!(
        found.is_empty(),
        "hook-router.md の表が rules.rs とずれている:\n{}",
        found.join("\n")
    );
}

// ---- 純粋関数の単体テスト(合成した期待と文書で、検査が実際に落ちることを見る) ----

fn exp() -> Expected {
    BTreeMap::from([
        (
            "PreToolUse".to_string(),
            BTreeSet::from(["a-guard".to_string(), "b-guard".to_string()]),
        ),
        (
            "PostToolUse".to_string(),
            BTreeSet::from(["c-record".to_string()]),
        ),
    ])
}

fn doc(pre: &str, post: &str, prose: &str) -> String {
    format!(
        "# t\n\n{SECTION}\n\n{prose}\n\n{HEADER}\n|---|---|\n| PreToolUse | {pre} |\n| PostToolUse | {post} |\n\n次の節\n\n## 別の節\n"
    )
}

#[test]
fn an_exact_match_has_no_problems() {
    let md = doc(
        "a-guard、b-guard",
        "c-record",
        "ルータの表に載せた検査は次のとおり。",
    );
    assert_eq!(problems(&exp(), &md), Vec::<String>::new());
}

#[test]
fn a_rule_missing_from_the_table_is_reported() {
    let md = doc("a-guard", "c-record", "p");
    let p = problems(&exp(), &md);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].contains("PreToolUse の表に無い規則: b-guard"),
        "{}",
        p[0]
    );
}

#[test]
fn a_name_that_rules_rs_lacks_is_reported() {
    let md = doc("a-guard、b-guard、ghost-guard", "c-record", "p");
    let p = problems(&exp(), &md);
    assert!(
        p.iter().any(|m| m.contains("rules.rs に無い: ghost-guard")),
        "{p:?}"
    );
}

#[test]
fn a_post_rule_in_the_pre_row_is_reported_both_ways() {
    let md = doc("a-guard、b-guard、c-record", "c-record", "p");
    let p = problems(&exp(), &md);
    assert!(
        p.iter()
            .any(|m| m.contains("PreToolUse の表にあるが rules.rs に無い: c-record")),
        "{p:?}"
    );
}

#[test]
fn a_duplicate_is_reported() {
    let md = doc("a-guard、a-guard、b-guard", "c-record", "p");
    let p = problems(&exp(), &md);
    assert!(p.iter().any(|m| m.contains("重複: a-guard")), "{p:?}");
}

#[test]
fn an_ascii_comma_separator_is_not_a_node_id() {
    let md = doc("a-guard, b-guard", "c-record", "p");
    let p = problems(&exp(), &md);
    assert!(
        p.iter().any(|m| m.contains("node id の綴りでない")),
        "{p:?}"
    );
}

#[test]
fn a_missing_heading_row_or_table_is_reported() {
    assert_eq!(
        problems(&exp(), "# t\n"),
        vec![format!("見出し「{SECTION}」が無い")]
    );
    let no_table = format!("# t\n\n{SECTION}\n\nだけ\n");
    assert!(problems(&exp(), &no_table)[0].contains("表(`| イベント | 検査 |`)が無い"));
    let no_post = format!("{SECTION}\n\n{HEADER}\n|---|---|\n| PreToolUse | a-guard、b-guard |\n");
    let p = problems(&exp(), &no_post);
    assert!(
        p.iter().any(|m| m.contains("行が無い: PostToolUse")),
        "{p:?}"
    );
}

#[test]
fn a_count_in_the_prose_before_the_table_is_reported() {
    for prose in ["載せた検査は 30 件。", "全部で30件です", "26件と4件"] {
        let md = doc("a-guard、b-guard", "c-record", prose);
        let p = problems(&exp(), &md);
        assert!(p.iter().any(|m| m.contains("件数")), "{prose}: {p:?}");
    }
    // 「件」の前が数字でなければ、件数ではない。
    let md = doc("a-guard、b-guard", "c-record", "事件の話ではない。");
    assert_eq!(problems(&exp(), &md), Vec::<String>::new());
}

#[test]
fn an_unknown_event_row_is_reported() {
    let md = format!(
        "{SECTION}\n\n{HEADER}\n|---|---|\n| PreToolUse | a-guard、b-guard |\n| PostToolUse | c-record |\n| Stop | x-gate |\n"
    );
    let p = problems(&exp(), &md);
    assert!(
        p.iter().any(|m| m.contains("未知のイベント: Stop")),
        "{p:?}"
    );
}
