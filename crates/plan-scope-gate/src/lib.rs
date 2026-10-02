//! plan-scope-gate — ExitPlanMode 直前に、要求インベントリ(config/claude/CLAUDE.md
//! の「複数項目の依頼は要求インベントリで受ける」節、config/claude/skills/
//! scope-inventory/SKILL.md)の脱落を機械的に検査する hook
//! (旧 `config/claude/hooks/plan-scope-gate.sh`、ADR-0024 Stage 4b #412)。
//!
//! 設計と根拠: docs/claude/scope-inventory.md
//!
//! この gate は LLM を呼ばず、文字列と gh の応答だけで判定する純粋な judge。
//!
//!   経路A(Issue起点の実カバレッジ): ユーザー自身が書いたメッセージから参照
//!     Issue を抽出し、子(sub-issues、無ければ本文の未チェック task-list)が
//!     2件以上ある Issue ごとに、プランが「実装対象として全子項目を処分」または
//!     `Reference-Only: #N — <理由>` のどちらかを宣言しているかを検査する。
//!   経路B(インベントリ内整合性): `## 要求インベントリ` 節が存在するときだけ、
//!     各 `Rn` 行に処分(段の指定 or 閉じたタグ)があるか・タグが妥当か・
//!     重複処分がないかを検査する。
//!
//!   - いずれかで問題が見つかれば deny(欠落項目を列挙してプラン修正を促す)。
//!   - 問題なし / gh 不在 / 認証エラー / ネットワーク不通 / 対象 Issue 無し
//!     → 何も決定しない(exit 0)。allow は決して返さない。
//!
//! 既知の限界(意図的な選択、docs/claude/scope-inventory.md 参照):
//!   - checkbox フォールバックの「子項目」はテキストの部分一致でしか検査できない。
//!   - `Reference-Only: #N` は Issue の番号だけで照合する。
//!
//! メッセージ文面はスキル文書(scope-inventory)と一致している必要があるため、
//! bash 版の出力とのバイト一致を `tests/cmd/*.toml` で固定している。
//!
//! 行の扱いは grep/awk/`read` と同じく `\n` だけで区切る(`\r` は行の一部として
//! 残る)。`[[:space:]]` は Unicode 空白(`\s`)で写した。

use hook_io::jqfmt::J;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

static RN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-*]?\s*\*{0,2}R([0-9]+)\*{0,2}[:.)]").unwrap());
static STAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(段[0-9]+|Stage\s*[0-9]+)").unwrap());
static CLOSED_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(Blocked-Upstream|Obsolete|User-Excluded):").unwrap());
static STAGE_OR_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(段[0-9]+|Stage\s*[0-9]+|(Blocked-Upstream|Obsolete|User-Excluded):)").unwrap()
});
static OTHER_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([A-Za-z][A-Za-z-]*):").unwrap());
static SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#+\s*要求インベントリ").unwrap());
static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#+\s").unwrap());
static ISSUE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#[0-9]+|[A-Za-z0-9_.-]+#[0-9]+|#[0-9]+").unwrap()
});
static CHECKBOX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-*]\s*\[\s\]\s*.+").unwrap());
static CHECKBOX_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-*]\s*\[\s\]\s*").unwrap());

/// grep/awk/`read` と同じ行分割(`\n` 区切り、末尾の空要素は落とす)。
pub fn lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

fn grep(re: &Regex, text: &str) -> bool {
    lines(text).iter().any(|l| re.is_match(l))
}

/// `$(…)` と同じく末尾の改行を落とす。
pub fn trim_newlines(mut s: String) -> String {
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

/// origin の owner/repo(git remote だけで判定、gh は呼ばない)。判定できなければ空。
pub fn resolve_owner_repo(cwd: &Path) -> String {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["remote", "get-url", "origin"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return String::new();
    };
    if !out.status.success() {
        return String::new();
    }
    let url = trim_newlines(String::from_utf8_lossy(&out.stdout).into_owned());
    let rest = [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
    ]
    .iter()
    .find_map(|p| url.strip_prefix(p));
    match rest {
        Some(r) => r.strip_suffix(".git").unwrap_or(r).to_string(),
        None => String::new(),
    }
}

/// "owner/repo#N" を重複無しで返す(バイト順)。
///
/// "name#N"(スラッシュ無しの接頭辞付き、他リポジトリの略記)は自リポジトリの
/// Issue 参照ではないため破棄する(#288)。裸の `#N` は既定の owner/repo を補う
/// (既定が空なら破棄)。
pub fn extract_issue_refs(text: &str, default: &str) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    for line in lines(text) {
        for m in ISSUE_REF_RE.find_iter(line) {
            let r = m.as_str();
            if r.contains('/') {
                refs.push(r.to_string());
            } else if r.starts_with('#') && !default.is_empty() {
                refs.push(format!("{default}{r}"));
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

/// 参照 Issue の子項目。`{"totalCount":N,"kind":"sub"|"checkbox"|"none","items":[...]}`。
#[derive(Debug, Clone, PartialEq)]
pub struct Children {
    pub total: Value,
    pub kind: String,
    pub items: Vec<Value>,
}

impl Children {
    /// `jq -c .` と同じ 1 行表記。
    pub fn compact(&self) -> String {
        J::obj(vec![
            ("totalCount", J::from(&self.total)),
            ("kind", J::str(self.kind.clone())),
            ("items", J::Arr(self.items.iter().map(J::from).collect())),
        ])
        .compact()
    }
}

fn gh_output(args: &[&str]) -> Option<String> {
    let out = Command::new("gh")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

const SUB_ISSUES_QUERY: &str = "
    query($owner:String!,$repo:String!,$number:Int!){
      repository(owner:$owner,name:$repo){
        issue(number:$number){
          subIssues(first:100){ totalCount nodes{ number title } }
        }
      }
    }";

/// 子項目を gh から取る。gh 不在・認証切れ・ネットワーク不通・不正な応答は
/// `None`(呼び出し側はその参照だけを黙って諦める、fail-open)。
pub fn fetch_children(r: &str) -> Option<Children> {
    let (ownerrepo, number) = match (r.find('#'), r.rfind('#')) {
        (Some(first), Some(last)) => (&r[..first], &r[last + 1..]),
        _ => (r, r),
    };
    let (owner, repo) = match ownerrepo.find('/') {
        Some(i) => (&ownerrepo[..i], &ownerrepo[i + 1..]),
        None => (ownerrepo, ownerrepo),
    };
    let resp = gh_output(&[
        "api",
        "graphql",
        "-f",
        &format!("query={SUB_ISSUES_QUERY}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={repo}"),
        "-F",
        &format!("number={number}"),
    ])?;
    let resp: Value = serde_json::from_str(&resp).ok()?;
    let issue = resp.pointer("/data/repository/issue")?;
    if issue.is_null() || issue == &Value::Bool(false) {
        return None;
    }
    let total = issue
        .pointer("/subIssues/totalCount")
        .cloned()
        .unwrap_or(Value::Null);
    if total.as_u64() != Some(0) {
        let items = match issue.pointer("/subIssues/nodes") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        return Some(Children {
            total,
            kind: "sub".into(),
            items,
        });
    }

    // フォールバック: 本文の未チェック task-list
    let Some(body) = gh_output(&[
        "issue",
        "view",
        number,
        "--repo",
        &format!("{owner}/{repo}"),
        "--json",
        "body",
        "-q",
        ".body",
    ]) else {
        return Some(Children {
            total: Value::from(0),
            kind: "none".into(),
            items: Vec::new(),
        });
    };
    let items: Vec<Value> = lines(&trim_newlines(body))
        .into_iter()
        .filter(|l| CHECKBOX_RE.is_match(l))
        .map(|l| CHECKBOX_PREFIX_RE.replace(l, "").into_owned())
        .filter(|t| !t.is_empty())
        .map(|t| serde_json::json!({ "text": t }))
        .collect();
    Some(Children {
        total: Value::from(items.len()),
        kind: "checkbox".into(),
        items,
    })
}

/// `## 要求インベントリ` 節(見出し行込み、次の見出しの手前まで)。無ければ空。
pub fn extract_inventory_section(plan: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut insec = false;
    for line in lines(plan) {
        if SECTION_RE.is_match(line) {
            insec = true;
            out.push(line);
            continue;
        }
        if insec && HEADING_RE.is_match(line) {
            break;
        }
        if insec {
            out.push(line);
        }
    }
    out
}

/// [`judge_issue`] の判定。
#[derive(Debug, Clone, PartialEq)]
pub enum IssueJudgement {
    Skip,
    ReferenceOnly,
    Covered,
    Missing(Vec<String>),
}

impl IssueJudgement {
    /// bash 版の出力(1行目がステータス、MISSING のときは続けて欠落項目)。
    pub fn render(&self) -> String {
        match self {
            IssueJudgement::Skip => "SKIP\n".into(),
            IssueJudgement::ReferenceOnly => "REFERENCE_ONLY\n".into(),
            IssueJudgement::Covered => "COVERED\n".into(),
            IssueJudgement::Missing(m) => {
                let mut s = String::from("MISSING\n");
                for x in m {
                    s.push_str(x);
                    s.push('\n');
                }
                s
            }
        }
    }
}

/// jq `@tsv` のエスケープ。
fn tsv_escape(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// 参照 Issue 1 件について、プランが子項目を処分しているかを判定する。
pub fn judge_issue(r: &str, children: &Children, plan: &str) -> IssueJudgement {
    let total = match &children.total {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    if total.is_empty() || !total.bytes().all(|b| b.is_ascii_digit()) {
        return IssueJudgement::Skip;
    }
    if total.parse::<u128>().map(|t| t < 2).unwrap_or(false) {
        return IssueJudgement::Skip;
    }

    let num = r.rsplit('#').next().unwrap_or(r);
    let refonly = Regex::new(&format!(
        r"Reference-Only:\s*([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#{}([^0-9]|$)",
        regex::escape(num)
    ))
    .expect("valid regex");
    if grep(&refonly, plan) {
        return IssueJudgement::ReferenceOnly;
    }

    let section = extract_inventory_section(plan);
    let mut missing: Vec<String> = Vec::new();
    if children.kind == "sub" {
        for item in &children.items {
            let cnum = tsv_escape(item.get("number").unwrap_or(&Value::Null));
            let ctitle = tsv_escape(item.get("title").unwrap_or(&Value::Null));
            let pat =
                Regex::new(&format!("#{}([^0-9]|$)", regex::escape(&cnum))).expect("valid regex");
            let line = section.iter().find(|l| pat.is_match(l));
            if !line.is_some_and(|l| STAGE_OR_TAG_RE.is_match(l)) {
                missing.push(format!("#{cnum} {ctitle}"));
            }
        }
    } else if children.kind == "checkbox" {
        let plan_lower = plan.to_lowercase();
        for item in &children.items {
            let ctext = match item.get("text") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => "null".into(),
                Some(other) => other.to_string(),
            };
            for ctext in lines(&ctext) {
                if ctext.chars().count() < 10 {
                    continue;
                }
                let key: String = ctext.chars().take(40).collect();
                let key = key.to_lowercase();
                if !lines(&plan_lower).iter().any(|l| l.contains(&key)) {
                    missing.push(ctext.to_string());
                }
            }
        }
    }
    if missing.is_empty() {
        IssueJudgement::Covered
    } else {
        IssueJudgement::Missing(missing)
    }
}

/// 経路B: `## 要求インベントリ` 節の整合性。節が無いプランでは何もしない。
pub fn judge_inventory(plan: &str) -> Vec<String> {
    let section = extract_inventory_section(plan);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in section {
        let Some(c) = RN_RE.captures(line) else {
            continue;
        };
        let id = c[1].to_string();
        let rest = &line[c.get(0).expect("match").end()..];
        if !seen.insert(id.clone()) {
            out.push(format!("R{id} が複数回処分されています"));
        }
        if STAGE_RE.is_match(rest) || CLOSED_TAG_RE.is_match(rest) {
            continue;
        }
        if let Some(t) = OTHER_TAG_RE.captures(rest) {
            out.push(format!(
                "R{id}: 閉じたタグ集合に無い棄却タグ({}:)が使われています",
                &t[1]
            ));
        } else {
            out.push(format!(
                "R{id}: 処分(実装する段、または棄却タグ)が未記載です"
            ));
        }
    }
    out
}

/// そのまま貼れば書式検査を通る完全な例文ブロック(末尾改行なし)。プレース
/// ホルダ `#<番号>` は数字必須の Reference-Only 免除正規表現にマッチしないため、
/// 丸写しのまま送っても実在 Issue の免除は誤発動しない。
pub const EXAMPLE_BLOCK: &str =
    "そのまま構造を写し、山括弧の中身だけ事実に置き換えれば書式検査は通ります:

## 要求インベントリ

- R1: <依頼文からの逐語の要求項目> — 段1で実装
- R2: <逐語項目> — Blocked-Upstream: <upstream 未修正など、外部にブロックされている理由>
- R3: <逐語項目> — Obsolete: <既に別の変更で解消済みである理由>
- R4: <逐語項目> — User-Excluded: <ユーザーが依頼文で明示的に除外した文言>

参照 Issue の子 #番号 を実装対象に含める行は、その行内に段の指定
(例: - R5: #137 <子タイトル逐語> — 段2で実装)か閉じたタグを書く。
参照しただけで実装対象でない Issue は本文のどこかに1行:

Reference-Only: #<番号> — <参照しただけである理由>";

/// 経路A で欠落があった参照 Issue の deny 行。
pub fn missing_lines(r: &str, items: &[String]) -> Vec<String> {
    let mut v = vec![format!(
        "参照 Issue {r} の子項目が要求インベントリに欠落しています(全子項目を処分するか、Reference-Only: {r} — <理由> を宣言してください):"
    )];
    v.extend(items.iter().map(|i| format!("  - {i}")));
    v
}

/// hook モードの deny 理由。
pub fn deny_message(deny_lines: &[String]) -> String {
    format!(
        "要求インベントリの検査で問題が見つかりました。scope-inventory スキルの手順に従って計画を修正してください。\n\n{}\n\n{EXAMPLE_BLOCK}",
        deny_lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ch(v: Value) -> Children {
        Children {
            total: v["totalCount"].clone(),
            kind: v["kind"].as_str().unwrap().into(),
            items: v["items"].as_array().unwrap().clone(),
        }
    }

    #[test]
    fn refs_mixed_dedup_sorted() {
        assert_eq!(
            extract_issue_refs(
                "#136 の話と owner2/repo2#5 も参照。#136 は再掲。",
                "tarotene/dotfiles"
            ),
            vec!["owner2/repo2#5", "tarotene/dotfiles#136"]
        );
        assert_eq!(
            extract_issue_refs("telepath#105 の話 #136", "tarotene/dotfiles"),
            vec!["tarotene/dotfiles#136"]
        );
        assert!(extract_issue_refs("#1", "").is_empty());
    }

    #[test]
    fn resolve_owner_repo_ssh_https() {
        let d = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(args)
                .status()
                .unwrap()
                .success())
        };
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", "git@github.com:foo/bar.git"]);
        assert_eq!(resolve_owner_repo(d.path()), "foo/bar");
        git(&[
            "remote",
            "set-url",
            "origin",
            "https://github.com/foo/bar.git",
        ]);
        assert_eq!(resolve_owner_repo(d.path()), "foo/bar");
        git(&["remote", "set-url", "origin", "file:///x"]);
        assert_eq!(resolve_owner_repo(d.path()), "");
    }

    #[test]
    fn judge_issue_cases() {
        let none = ch(json!({"totalCount":0,"kind":"none","items":[]}));
        assert_eq!(judge_issue("o/r#1", &none, ""), IssueJudgement::Skip);
        let one = ch(json!({"totalCount":1,"kind":"sub","items":[{"number":9,"title":"x"}]}));
        assert_eq!(judge_issue("o/r#1", &one, ""), IssueJudgement::Skip);

        let sub = ch(
            json!({"totalCount":2,"kind":"sub","items":[{"number":137,"title":"t1"},{"number":138,"title":"t2"}]}),
        );
        let r = "tarotene/dotfiles#136";
        assert_eq!(
            judge_issue(
                r,
                &sub,
                "## 要求インベントリ\n- R1: #137 は段1で実装\n- R2: #138 は段2で実装\n"
            ),
            IssueJudgement::Covered
        );
        assert_eq!(
            judge_issue(r, &sub, "## 要求インベントリ\n- R1: #137 は段1で実装\n"),
            IssueJudgement::Missing(vec!["#138 t2".into()])
        );
        assert_eq!(
            judge_issue(
                r,
                &sub,
                "依頼文\nReference-Only: #136 — 参考のみで実装対象ではない\n"
            ),
            IssueJudgement::ReferenceOnly
        );
        assert!(matches!(
            judge_issue(r, &sub, "依頼文だけで要求インベントリを書いていない\n"),
            IssueJudgement::Missing(_)
        ));

        let cb = ch(
            json!({"totalCount":2,"kind":"checkbox","items":[{"text":"child item one two three four five six"},{"text":"another distinct item text entirely"}]}),
        );
        assert_eq!(
            judge_issue(
                "o/r#1",
                &cb,
                "計画本文には child item one two three four five six は載っているが、もう一方は無い"
            ),
            IssueJudgement::Missing(vec!["another distinct item text entirely".into()])
        );
    }

    #[test]
    fn judge_inventory_cases() {
        assert!(judge_inventory(
            "## 要求インベントリ\n- R1: 段1で実装\n- R2: Obsolete: もう不要\n"
        )
        .is_empty());
        assert!(judge_inventory("依頼文だけ\n").is_empty());
        assert_eq!(
            judge_inventory("## 要求インベントリ\n- R1: 段1で実装\n- R1: 段2でも実装\n- R2: Conflicts: 何か\n- R3: よくわからない項目\n"),
            vec![
                "R1 が複数回処分されています",
                "R2: 閉じたタグ集合に無い棄却タグ(Conflicts:)が使われています",
                "R3: 処分(実装する段、または棄却タグ)が未記載です",
            ]
        );
        assert!(judge_inventory(EXAMPLE_BLOCK).is_empty());
    }
}
