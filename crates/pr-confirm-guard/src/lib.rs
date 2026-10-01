//! pr-confirm-guard — PR 本文に (i) 未チェックの task list(`- [ ]`)、または
//! (ii) `## 要確認` に Issue 参照を持たない項目があれば `gh pr create` /
//! `gh pr edit` の呼び出し自体を deny する PreToolUse hook
//! (docs/claude/pr-confirm-guard.md、ADR-598、ADR-0024 Stage 4a #415)。
//!
//! bash 版 `config/claude/hooks/pr-confirm-guard.sh`(と Codex adapter
//! `config/codex/hooks/pr-confirm-guard.sh`)の移植。コマンド解析と本文抽出
//! (`extract_body`)は `guard-core` にあり、ここには本文の判定だけを置く。
//!
//! 動機(2026-09-30 のグリルセッション): 「要確認」を人手ブロッキング項目を
//! 手順付きで書く節から、既に払い出した後続 Issue へのポインタだけを書く節に
//! 転換し、人待ちの未チェック task list を本文に存在できない形にした。確認者の
//! 予定が PR の寿命を決め、base が進むほど rebase 負債になるため。
//!
//! 発火は全リポジトリで無条件(owner スコープを持たない)。この規律は「自分が
//! 書く PR 本文に人待ち作業を残さない」という書き手側の規律で、
//! attribution-guard(自分の投稿本文の規律)と同じ類型 — pr-title-guard の
//! ADR-0031 D4(squash title = main の履歴、tarotene 配下の merge 設定に依存
//! する契約)とは根拠が異なるので、その限定は継承しない。
//!
//! 判定基準:
//! - (i) 本文全体(フェンス外・インラインコードスパン除去後)に未チェックの
//!   task list 行(`- [ ]` / `* [ ]` / `+ [ ]`、インデント可)が 1 つでもあれば違反
//! - (ii) 「要確認」を含む見出しの配下の各トップレベル項目(フェンス外・列頭の
//!   `- ` / `* ` / `N. ` で始まる行から次の列頭マーカーまたは節末まで)が
//!   Issue への参照(`#<番号>` または `github.com/<owner>/<repo>/issues/<番号>`)
//!   を 1 つ以上持つこと。見出しが無い・項目が無いなら pass
//!
//! (i)(ii) の違反は 1 つの deny メッセージに合流させる(`gh pr edit` 1 回で
//! 両方直せる)。「払い出し先の Issue が実在し妥当か」は判定せず、
//! pr-description スキル(LLM の判断)の責務にする(`G_visual` と同じ二層分担)。
//!
//! escape hatch: 環境変数 `PR_CONFIRM_GUARD_ALLOW=1`(本文タグ型の恒久
//! エスケープではない)。
//!
//! bash 版は `LC_ALL=C` で動くので `[[:space:]]` / `[[:alnum:]]` は ASCII だけ。
//! ここでもバイト列で ASCII として扱う。

use guard_core::command::{first_deny, gh_command_at};
use guard_core::gh::extract_body;
use guard_core::shell::is_posix_space;

/// bash の `is_target_at`: `gh pr create` / `gh pr edit`(種別は判定に使わない)。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<()> {
    (gh_command_at(tokens, i, &["pr", "create"]) || gh_command_at(tokens, i, &["pr", "edit"]))
        .then_some(())
}

/// bash の `strip_code_spans`(pr-gate の同名関数と同じ簡略化 — GitHub が
/// 実際にどう解釈するかを判定基準にする)。フェンス行(`` ``` `` / `~~~` で
/// 始まる行、インデント可)で開閉を切り替え、フェンス内の行は捨てる。
/// フェンス外の行はインラインコードスパン(`` `…` ``)を空白 1 つに置き換える。
/// 戻り値は残った行(各行を改行で連結、末尾の改行無し)。
pub fn strip_code_spans(text: &str) -> Vec<String> {
    let mut fence = false;
    let mut out = Vec::new();
    for line in text.split('\n') {
        let t = line.trim_start_matches(|c: char| c.is_ascii() && is_posix_space(c as u8));
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        out.push(replace_inline_code(line));
    }
    out
}

/// awk の `gsub(/`[^`]*`/, " ")`: 左から最短の対(`` `…` ``)を空白 1 つに置く。
/// 対にならない末尾のバッククォートはそのまま残る。
fn replace_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        match rest[open + 1..].find('`') {
            Some(close) => {
                out.push_str(&rest[..open]);
                out.push(' ');
                rest = &rest[open + 1 + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn is_space(b: u8) -> bool {
    is_posix_space(b)
}

/// `UNCHECKED_TASK_RE='^[[:space:]]*[-*+][[:space:]]+\[[[:space:]]\]'`(1 行)。
fn is_unchecked_task_line(line: &str) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    if i >= b.len() || !matches!(b[i], b'-' | b'*' | b'+') {
        return false;
    }
    i += 1;
    let ws = i;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    if i == ws {
        return false;
    }
    b.get(i) == Some(&b'[')
        && b.get(i + 1).is_some_and(|&c| is_space(c))
        && b.get(i + 2) == Some(&b']')
}

/// (i): 未チェック task list 行が 1 つでもあるか(コード内は数えない)。
pub fn body_has_unchecked_task(body: &str) -> bool {
    strip_code_spans(body)
        .iter()
        .any(|l| is_unchecked_task_line(l))
}

/// bash の `find_confirm_section`: 「要確認」を含む見出し行(`^#+[[:space:]]`)の
/// 次行から、次の見出し行の手前まで(見出しレベルの追跡はしない)。見出しが
/// 複数あれば該当する節を連結する。見出しが無い・節が空白だけなら `None`
/// (bash は `$(…)` が末尾の改行を落とすので `[[ -n $section ]]` が偽になる)。
pub fn find_confirm_section(body: &str) -> Option<String> {
    let mut insec = false;
    let mut out = String::new();
    for line in body.split('\n') {
        if is_heading(line) {
            insec = line.contains("要確認");
            continue;
        }
        if insec {
            out.push_str(line);
            out.push('\n');
        }
    }
    let out = out.trim_end_matches('\n');
    (!out.is_empty()).then(|| out.to_string())
}

/// `/^#+[[:space:]]/`
fn is_heading(line: &str) -> bool {
    let b = line.as_bytes();
    let hashes = b.iter().take_while(|&&c| c == b'#').count();
    hashes > 0 && b.get(hashes).is_some_and(|&c| is_space(c))
}

/// bash の `collect_items`: フェンス外の列頭 `- ` / `* ` / `N. ` で始まる各項目
/// (次の列頭マーカーまたは節末まで)。フェンスの開閉は列頭の `` ``` `` / `~~~`
/// だけ(`strip_code_spans` と違いインデント付きは見ない)。最初のマーカー
/// より前の行は捨てる。
pub fn collect_items(section: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut infence = false;
    for line in section.split('\n') {
        if line.starts_with("```") || line.starts_with("~~~") {
            infence = !infence;
            if started {
                cur.push('\n');
                cur.push_str(line);
            }
            continue;
        }
        if !infence && is_item_start(line) {
            if started {
                items.push(std::mem::take(&mut cur));
            }
            cur = line.to_string();
            started = true;
            continue;
        }
        if started {
            cur.push('\n');
            cur.push_str(line);
        }
    }
    if started {
        items.push(cur);
    }
    items
}

/// `^[-*][[:space:]]` または `^[0-9]+\.[[:space:]]`
fn is_item_start(line: &str) -> bool {
    let b = line.as_bytes();
    if matches!(b.first(), Some(b'-' | b'*')) {
        return b.get(1).is_some_and(|&c| is_space(c));
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && b.get(digits) == Some(&b'.') && b.get(digits + 1).is_some_and(|&c| is_space(c))
}

/// `ISSUE_REF_RE='(^|[^[:alnum:]/])#[0-9]+|github\.com/[^/[:space:]]+/[^/[:space:]]+/issues/[0-9]+'`
/// を 1 行に対して調べる(grep は行単位)。
fn line_has_issue_ref(line: &str) -> bool {
    let b = line.as_bytes();
    // `#<番号>`: 直前が行頭か、英数字でも `/` でもない 1 バイト
    for (i, &c) in b.iter().enumerate() {
        if c == b'#'
            && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'/'))
        {
            return true;
        }
    }
    // `github.com/<owner>/<repo>/issues/<番号>`(<owner>/<repo> は `/` と空白を含まない)
    const HOST: &str = "github.com/";
    let seg = |s: &[u8]| s.iter().take_while(|&&c| c != b'/' && !is_space(c)).count();
    let mut from = 0;
    while let Some(p) = line[from..].find(HOST) {
        let s = from + p + HOST.len();
        let owner = seg(&b[s..]);
        if owner > 0 && b.get(s + owner) == Some(&b'/') {
            let r = s + owner + 1;
            let repo = seg(&b[r..]);
            let tail = &b[r + repo..];
            if repo > 0
                && tail.starts_with(b"/issues/")
                && tail.get(8).is_some_and(u8::is_ascii_digit)
            {
                return true;
            }
        }
        from += p + 1;
    }
    false
}

/// bash の `item_has_issue_ref`: コードスパン内の引用は数えない。
pub fn item_has_issue_ref(item: &str) -> bool {
    strip_code_spans(item).iter().any(|l| line_has_issue_ref(l))
}

/// bash の `judge_confirm_section`: `## 要確認` の項目のうち Issue 参照が無い
/// ものを 1 行 1 件にしたもの。見出し・項目が無い、全項目に参照があるなら `None`。
pub fn judge_confirm_section(body: &str) -> Option<String> {
    let section = find_confirm_section(body)?;
    let items = collect_items(&section);
    let violations: Vec<String> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| !item_has_issue_ref(it))
        .map(|(i, _)| {
            format!(
                "項目{}: Issue 参照(#N または .../issues/N の URL)がありません",
                i + 1
            )
        })
        .collect();
    (!violations.is_empty()).then(|| violations.join("\n"))
}

/// bash の `judge_confirm`: (i)(ii) いずれかの違反があれば案内メッセージ。
pub fn judge_confirm(body: &str) -> Option<String> {
    let mut violations: Vec<String> = Vec::new();
    if body_has_unchecked_task(body) {
        violations.push(
            "未チェックの task list(`- [ ]`)が本文にあります。完了して\nいれば `[x]` に倒し、人の確認が要るなら後続 Issue へ払い出して\n`## 要確認` に `- #N — <一言>` の形で参照してください。"
                .to_string(),
        );
    }
    if let Some(section_out) = judge_confirm_section(body) {
        violations.push(format!(
            "`## 要確認` に Issue 参照の無い項目があります:\n{section_out}"
        ));
    }
    if violations.is_empty() {
        return None;
    }
    let mut out = String::from("PR 本文に次の不備があります:\n\n");
    for v in &violations {
        out.push_str(v);
        out.push_str("\n\n");
    }
    out.push_str(
        "各項目は次の形で書いてください(pr-description スキル §1):\n  ## 検証\n  - [x] <実施済みの確認>\n\n  ## 要確認\n  - #<Issue番号> — <一言>\n\n一時的に無効化するには PR_CONFIRM_GUARD_ALLOW=1 を設定してください。",
    );
    Some(out)
}

/// bash の `decide_pr_confirm`: コマンド文字列全体。範囲ごとに独立に本文を
/// 抽出して判定し、最初の deny の理由文を返す。`allow` は
/// `PR_CONFIRM_GUARD_ALLOW=1`(bash は範囲ごとに環境変数を見る)。
pub fn decide(cmd: &str, allow: bool) -> Option<String> {
    first_deny(cmd, is_target_at, |r| {
        if allow {
            return None;
        }
        let body = extract_body(r.tokens, r.heredoc_bodies)?;
        judge_confirm(&body)
    })
}

/// 環境変数 `PR_CONFIRM_GUARD_ALLOW`(厳密に `1` のときだけ有効)。
pub fn allow_from_env() -> bool {
    std::env::var("PR_CONFIRM_GUARD_ALLOW").is_ok_and(|v| v == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_spans_and_fences() {
        let t = "a `- [ ] x` b\n```\n- [ ] y\n```\n  ~~~\n- [ ] z\n  ~~~\n- [x] ok\nstray ` tick";
        assert_eq!(strip_code_spans(t), ["a   b", "- [x] ok", "stray ` tick"]);
        assert!(!body_has_unchecked_task(t));
        assert!(body_has_unchecked_task("x\n\t+\t[\t] y"));
        assert!(!body_has_unchecked_task("-[ ] y"));
        assert!(!body_has_unchecked_task("- [] y"));
    }

    #[test]
    fn confirm_section_and_items() {
        let body =
            "a\n## 要確認\n- x\n  cont\n```\n- fenced\n```\n2. y\n## 他\n- z\n#### 要確認 2\n* w";
        let sec = find_confirm_section(body).unwrap();
        let items = collect_items(&sec);
        assert_eq!(items[0], "- x\n  cont\n```\n- fenced\n```");
        assert_eq!(items[1], "2. y");
        assert_eq!(items[2], "* w");
        assert_eq!(items.len(), 3);
        // `#42` で始まる行は見出しではない
        assert_eq!(
            find_confirm_section("## 要確認\n#42 — x\n"),
            Some("#42 — x".into())
        );
        assert_eq!(find_confirm_section("## 要確認\n\n\n"), None);
        assert_eq!(find_confirm_section("本文"), None);
    }

    #[test]
    fn issue_refs() {
        for s in [
            "#42",
            "- #42 — x",
            "—#42",
            "(#1)",
            "see https://github.com/o/r/issues/9",
            "xgithub.com/o/r/issues/9",
        ] {
            assert!(line_has_issue_ref(s), "{s}");
        }
        for s in [
            "a#42",
            "o/r#42",
            "#x",
            "github.com/o/r/pull/9",
            "github.com//r/issues/9",
            "github.com/o/r/issues/x",
            "github.com/o r/issues/9",
        ] {
            assert!(!line_has_issue_ref(s), "{s}");
        }
        assert!(!item_has_issue_ref("- `#42` だけ"));
    }
}
