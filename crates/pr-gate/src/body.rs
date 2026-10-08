//! PR 本文の純粋な判定(G_link / G_visual / Handoff 行 / G_prior の照合)。
//!
//! bash 版は `grep -E` / `awk` を行単位で当てていた。ここでも本文を `\n` で
//! 行に割ってから 1 行ずつ正規表現を当てる(`[[:space:]]` が改行を跨がない
//! ように。`\r` は bash 版と同じく行の一部として残す)。

use regex::Regex;
use std::sync::LazyLock;

/// GitHub が解釈する closing keyword は close/closes/closed, fix/fixes/fixed,
/// resolve/resolves/resolved の 9 語。参照形式は同一リポジトリの `#N` と
/// クロスリポジトリの `owner/repo#N`。keyword は大文字でもよく、コロンを伴っても
/// よい(`Closes: #10` も公式に解釈される)ので、いずれも受理する。
/// 出典: docs.github.com「Linking a pull request to an issue」
///
/// issue URL 形式は上記の表には無いが、書かれていれば意図は明白で、これを
/// MISSING と判定すると gate が誤って止める側に倒れる。受理する。
static CLOSING_KEYWORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(close[sd]?|fix(e[sd])?|resolve[sd]?)[[:space:]]*:?[[:space:]]+(#[0-9]+|[A-Za-z0-9._-]+/[A-Za-z0-9._-]+#[0-9]+|https://github\.com/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+/issues/[0-9]+)",
    )
    .expect("CLOSING_KEYWORD_RE")
});

/// closing keyword の直後に番号を並べた形(`Closes #1 #2`、`Closes #1, #2`、
/// `Closes #1 and #2`)。GitHub は keyword 直後の 1 件しか閉じず、残りは黙って
/// open のまま残る(1 keyword 1 Issue が規則。出典: docs.github.com「Linking a
/// pull request to an issue」、#722)。
static KEYWORD_LIST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(close[sd]?|fix(e[sd])?|resolve[sd]?)[[:space:]]*:?[[:space:]]+(?:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)?#[0-9]+(?:(?:[[:space:]]*,|[[:space:]]+and|[[:space:]]*&)?[[:space:]]+(?:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)?#[0-9]+)+",
    )
    .expect("KEYWORD_LIST_RE")
});
static ISSUE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)?#[0-9]+").expect("ISSUE_REF_RE")
});

/// `No-Issue:` は理由を伴って初めて成立する。空の `No-Issue:` を通すと、沈黙を
/// 決定に変えるという G_link の目的が失われ、ただのおまじないになる。
static NO_ISSUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[[:space:]]*No-Issue:[[:space:]]*[^[:space:]]").expect("NO_ISSUE_RE")
});

/// 中断ハンドオフの宣言行(docs/claude/handoff.md)。
static HANDOFF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[[:space:]]*Handoff:[[:space:]]*#([0-9]+)[[:space:]]*$").expect("HANDOFF_RE")
});

/// `No-Visual:` は `No-Issue:` と同型 — 理由を伴って初めて成立する。
static NO_VISUAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[[:space:]]*No-Visual:[[:space:]]*[^[:space:]]").expect("NO_VISUAL_RE")
});

/// gh pr create/edit --attach (>= 2.99.0) がアップロード後に本文へ書く markdown
/// 画像の形。ローカルパスのままの `![x](./a.png)` はアップロードの証拠にならない
/// ので数えない。
static ATTACH_IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"!\[[^\]]*\]\(https://github\.com/user-attachments/assets/[^)[:space:]]+\)")
        .expect("ATTACH_IMAGE_RE")
});

static FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[[:space:]]*(```|~~~)").expect("FENCE_RE"));
static CODE_SPAN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`]*`").expect("CODE_SPAN_RE"));
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#+[[:space:]]").expect("HEADING_RE"));
static BEFORE_AFTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"before[[:space:]]*/?[[:space:]]*after").expect("BEFORE_AFTER_RE")
});

fn any_line(text: &str, re: &Regex) -> bool {
    text.split('\n').any(|l| re.is_match(l))
}

/// GitHub は **コード内の closing keyword を解釈しない**。fenced code block の中も、
/// `` `Closes #30` `` のようなインラインのコードスパンの中も無視される。判定前に
/// 両方落としておかないと、ゲートが「閉じないのに LINKED」と読む — つまり G_link
/// が防ごうとしているまさにその事故(マージしても Issue が open のまま)を、ゲート
/// 自身が見逃す側に倒れる。
///
/// これは机上の懸念ではない。この判定を入れた PR #46 の本文が
/// 「本文に `Closes #30 / #33 / …` を明記し」と実例をコードスパンで引用しており、
/// 初回の実地検証で `gh pr view --json closingIssuesReferences` が空を返して発覚した。
///
/// 判定基準は「GitHub がどう読むか」であって「人がどう書いたつもりか」ではない。
pub fn strip_code_spans(body: &str) -> String {
    let mut fence = false;
    let mut out = Vec::new();
    for line in body.split('\n') {
        if FENCE_RE.is_match(line) {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        out.push(CODE_SPAN_RE.replace_all(line, " ").into_owned());
    }
    out.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Linked,
    NoIssue,
    Missing,
}

/// G_link の判定。
///
/// 「言及があるのに keyword が無い」だけを見る形にはしない。#28/#29 を実質解決
/// した PR は Issue に一切言及していなかった — 観測された失敗そのものを通す判定には
/// 意味がない。参照先 Issue の実在確認もしない(捕まえられるのは「存在しない番号」
/// だけで、より起きやすい「存在するが別の Issue」は捕まえられない一方、API 呼び出し
/// が 1 本増えて縮退表が太る)。
pub fn judge_link(body: &str) -> Link {
    let s = strip_code_spans(body);
    if any_line(&s, &CLOSING_KEYWORD_RE) {
        Link::Linked
    } else if any_line(&s, &NO_ISSUE_RE) {
        Link::NoIssue
    } else {
        Link::Missing
    }
}

/// keyword の後ろに並べられ、GitHub が閉じない参照(先頭の 1 件を除いた残り)。
/// コード内の記述は GitHub が解釈しないので `strip_code_spans` を通してから見る。
/// 重複は最初の出現だけ残す。
pub fn unclosed_listed_refs(body: &str) -> Vec<String> {
    let s = strip_code_spans(body);
    let mut out: Vec<String> = Vec::new();
    for line in s.split('\n') {
        for m in KEYWORD_LIST_RE.find_iter(line) {
            for r in ISSUE_REF_RE.find_iter(m.as_str()).skip(1) {
                let r = r.as_str().to_string();
                if !out.contains(&r) {
                    out.push(r);
                }
            }
        }
    }
    out
}

/// コード外の最初の `Handoff: #N` 行の N(`grep -m1` の 1 行目)。
pub fn handoff_issue(body: &str) -> Option<String> {
    let s = strip_code_spans(body);
    s.split('\n')
        .find_map(|l| HANDOFF_RE.captures(l).map(|c| c[1].to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visual {
    Image,
    CodeBlock,
    NoVisual,
    Missing,
}

/// `## Before / After` 系の見出し配下に fenced code block が 1 つ以上あるか。
/// strip_code_spans は通さない — G_link と逆で、ここでは fence 自体が判定対象の
/// 証跡だから(strip すると判定材料ごと消える)。次の見出し(レベルを問わない)に
/// 当たるまでを「節の中」とみなす。
pub fn before_after_has_fence(body: &str) -> bool {
    let mut insec = false;
    for line in body.split('\n') {
        if HEADING_RE.is_match(line) {
            insec = BEFORE_AFTER_RE.is_match(&line.to_lowercase());
            continue;
        }
        if insec && FENCE_RE.is_match(line) {
            return true;
        }
    }
    false
}

/// G_visual の判定。3 択の OR:
///   (a) 本文に user-attachments の画像
///   (b) `## Before / After` 見出し配下に fenced code block が 1 つ以上
///   (c) `No-Issue:` と同型の `No-Visual: <理由>`
///
/// **検査しないこと**: Before と After が実際にペアで揃っているか、画像が何枚か、
/// fence の中身が本当に対比になっているかは見ない。機械的に判定できるのは
/// 「証跡があるかどうか」までで、「対比として十分か」は pr-description スキル
/// (LLM の判断)の責務にする。ペア強制はゲート側でやると、Before が存在しない
/// 正当ケース(新規追加など)を誤って止める側に倒れる。
pub fn judge_visual(body: &str) -> Visual {
    let s = strip_code_spans(body);
    if any_line(&s, &ATTACH_IMAGE_RE) {
        Visual::Image
    } else if before_after_has_fence(body) {
        Visual::CodeBlock
    } else if any_line(&s, &NO_VISUAL_RE) {
        Visual::NoVisual
    } else {
        Visual::Missing
    }
}

/// 本文(strip_code_spans 済み)に `既存手段: <path> — …` 行があるか。前後を
/// 空白+ダッシュで区切ることで、他パスの接頭辞への誤マッチを防ぐ
/// (plan-precedent-gate の CITATION_RE と同じダッシュ種の許容)。
pub fn body_has_kizon_for(stripped: &str, path: &str) -> bool {
    let pat = format!(
        r"既存手段:[[:space:]]*{}[[:space:]]+[—–-][[:space:]]",
        regex::escape(path)
    );
    match Regex::new(&pat) {
        Ok(re) => any_line(stripped, &re),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 1 keyword 1 Issue(#722) ---

    #[test]
    fn unclosed_listed_refs_names_everything_after_the_first() {
        let body =
            "Closes #1 #2 #3\nCloses #4, #5\nFixes #6\nCloses #7 and #8\nCloses o/r#9 o/r#10";
        assert_eq!(
            unclosed_listed_refs(body),
            ["#2", "#3", "#5", "#8", "o/r#10"]
        );
    }

    #[test]
    fn unclosed_listed_refs_accepts_one_keyword_per_issue() {
        assert!(unclosed_listed_refs("Closes #1\nCloses #2\nFixes #3").is_empty());
        // keyword を挟めば別の参照
        assert!(unclosed_listed_refs("Closes #1 Fixes #2").is_empty());
        assert!(unclosed_listed_refs("Closes #1 (see #2)").is_empty());
    }

    #[test]
    fn unclosed_listed_refs_ignores_code() {
        assert!(unclosed_listed_refs("`Closes #1 #2`").is_empty());
        assert!(unclosed_listed_refs("```\nCloses #1 #2\n```").is_empty());
    }

    // --- 旧 selftest「G_prior(ADR-543)の純粋関数部分」の 6 ケース ---

    #[test]
    fn g_prior_exact_match() {
        // ok   G_prior: 完全一致で照合
        assert!(body_has_kizon_for(
            "既存手段: scripts/foo.sh — 採用: jq",
            "scripts/foo.sh"
        ));
    }

    #[test]
    fn g_prior_longer_path_prefix_collision() {
        // ok   G_prior: 接頭辞衝突(長い方)を誤照合しない
        assert!(!body_has_kizon_for(
            "既存手段: scripts/foo.sh — 採用: jq",
            "scripts/foo.sh.bak"
        ));
    }

    #[test]
    fn g_prior_shorter_path_prefix_collision() {
        // ok   G_prior: 接頭辞衝突(短い方)を誤照合しない
        assert!(!body_has_kizon_for(
            "既存手段: scripts/foo.sh.bak — 採用: jq",
            "scripts/foo.sh"
        ));
    }

    #[test]
    fn g_prior_dash_variants() {
        // ok   G_prior: ダッシュ種 [—] / [–] / [-] を許容(3 ケース)
        for dash in ["—", "–", "-"] {
            assert!(
                body_has_kizon_for(
                    &format!("既存手段: bin/x {dash} 自前 — 却下: なし"),
                    "bin/x"
                ),
                "{dash}"
            );
        }
    }

    // --- 以下は移植時に足した、bash の grep/awk との対応の固定 ---

    #[test]
    fn strip_code_spans_drops_fences_and_spans() {
        let s = strip_code_spans("a `Closes #1` b\n```\nCloses #2\n```\nCloses #3");
        assert_eq!(s, "a   b\nCloses #3");
    }

    #[test]
    fn link_forms() {
        assert_eq!(judge_link("CLOSES: #30"), Link::Linked);
        assert_eq!(judge_link("Fixes octo-org/octo-repo#100"), Link::Linked);
        assert_eq!(judge_link("No-Issue: x"), Link::NoIssue);
        assert_eq!(judge_link("No-Issue:"), Link::Missing);
        assert_eq!(judge_link("No-Issue:   \r"), Link::Missing);
    }

    #[test]
    fn handoff_line() {
        assert_eq!(handoff_issue("x\nHandoff: #12 \n").as_deref(), Some("12"));
        assert_eq!(handoff_issue("```\nHandoff: #1\n```"), None);
        assert_eq!(handoff_issue("Handoff: #1 で再開"), None);
    }

    #[test]
    fn visual_forms() {
        assert_eq!(
            judge_visual("## Before/After\n~~~\nx\n~~~"),
            Visual::CodeBlock
        );
        assert_eq!(judge_visual("## 検証\n```\nx\n```"), Visual::Missing);
    }
}
