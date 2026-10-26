//! plan-scope-gate — ExitPlanMode 直前に、要求インベントリ(config/claude/CLAUDE.md
//! の「複数項目の依頼は要求インベントリで受ける」節、config/agents/skills/
//! requirements-inventory/SKILL.md)の脱落を機械的に検査する hook
//! (旧 `config/claude/hooks/plan-scope-gate.sh`、ADR-0024 Stage 4b #412)。
//!
//! 設計と根拠: docs/agents/requirements-inventory.md
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
//! 既知の限界(意図的な選択、docs/agents/requirements-inventory.md 参照):
//!   - checkbox フォールバックの「子項目」はテキストの部分一致でしか検査できない。
//!   - `Reference-Only: #N` は Issue の番号だけで照合する。
//!
//! メッセージ文面はスキル文書(requirements-inventory)と一致している必要があるため、
//! bash 版の出力とのバイト一致を `tests/cmd/*.toml` で固定している。
//!
//! 行の扱いは grep/awk/`read` と同じく `\n` だけで区切る(`\r` は行の一部として
//! 残る)。`[[:space:]]` は Unicode 空白(`\s`)で写した。

use jq_json_compat::jqfmt::J;
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

/// `--check` の issue-ref 引数を `owner/repo#N` に直す(#896)。
///
/// - `owner/repo#N` はそのまま。
/// - `#N` と、番号だけの `N` は、カレントの origin(`owner_repo`、判定できなければ
///   空を返す関数)から owner/repo を補う。補えなければその旨で落とす。
/// - それ以外(`owner/repo1273` のように `#` が無い、`name#N` など)は、書式を案内して
///   落とす。組み立てた誤った参照のまま gh を呼ぶと、書式の誤りが「gh 不在・未認証・
///   ネットワーク不通」に見えてしまう。
pub fn normalize_issue_ref(
    arg: &str,
    owner_repo: impl FnOnce() -> String,
) -> Result<String, String> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let segment = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    if let Some((repo, n)) = arg.rsplit_once('#') {
        if digits(n) {
            if repo.is_empty() {
                return with_owner_repo(owner_repo(), n);
            }
            if repo
                .split_once('/')
                .is_some_and(|(o, r)| segment(o) && segment(r))
            {
                return Ok(arg.to_string());
            }
        }
    } else if digits(arg) {
        return with_owner_repo(owner_repo(), arg);
    }
    Err(format!(
        "issue-ref の書式が不正です: {arg:?} — `owner/repo#N`(例: tarotene/dotfiles#136)、または cwd の origin から補う `#N` / `N` で渡してください"
    ))
}

fn with_owner_repo(owner_repo: String, n: &str) -> Result<String, String> {
    if owner_repo.is_empty() {
        Err("cwd is not inside a GitHub-remote repo; pass owner/repo#N explicitly".to_string())
    } else {
        Ok(format!("{owner_repo}#{n}"))
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

/// gh を実行して stdout を返す。失敗は診断用の 1 行(起動失敗の理由、または
/// 終了状態と stderr の要点)にして返す — 呼び出し側が fail-open で捨てるか、
/// `--check` のように表示するかを選ぶ(#1036)。
fn run_gh(args: &[&str]) -> Result<String, String> {
    let label = args.iter().take(2).copied().collect::<Vec<_>>().join(" ");
    let out = Command::new("gh")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("gh を起動できません(gh {label}): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh {label} が失敗しました({}){}",
            out.status,
            stderr_summary(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 子プロセスの stderr の要点(先頭の非空 3 行・300 文字まで)。空なら空文字列。
fn stderr_summary(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let head: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(3)
        .collect();
    if head.is_empty() {
        return String::new();
    }
    let joined: String = head.join(" / ").chars().take(300).collect();
    format!(": {joined}")
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
/// `None`(呼び出し側はその参照だけを黙って諦める、fail-open)。理由が要る
/// 呼び出し側は [`try_fetch_children`] を使う。
pub fn fetch_children(r: &str) -> Option<Children> {
    try_fetch_children(r).ok()
}

/// [`fetch_children`] と同じ取得で、失敗の理由(起動失敗・終了状態・stderr の
/// 要点・応答の形)を `Err` に残す(#1036)。
pub fn try_fetch_children(r: &str) -> Result<Children, String> {
    let (ownerrepo, number) = match (r.find('#'), r.rfind('#')) {
        (Some(first), Some(last)) => (&r[..first], &r[last + 1..]),
        _ => (r, r),
    };
    let (owner, repo) = match ownerrepo.find('/') {
        Some(i) => (&ownerrepo[..i], &ownerrepo[i + 1..]),
        None => (ownerrepo, ownerrepo),
    };
    let resp = run_gh(&[
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
    let resp: Value = serde_json::from_str(&resp)
        .map_err(|e| format!("gh api graphql の応答が JSON として読めません: {e}"))?;
    let issue = resp
        .pointer("/data/repository/issue")
        .filter(|i| !i.is_null() && *i != &Value::Bool(false))
        .ok_or_else(|| {
            format!("Issue {owner}/{repo}#{number} が見つかりません(参照・権限を確認してください)")
        })?;
    let total = issue
        .pointer("/subIssues/totalCount")
        .cloned()
        .unwrap_or(Value::Null);
    if total.as_u64() != Some(0) {
        let items = match issue.pointer("/subIssues/nodes") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        return Ok(Children {
            total,
            kind: "sub".into(),
            items,
        });
    }

    // フォールバック: 本文の未チェック task-list
    let Ok(body) = run_gh(&[
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
        // 本文を読めないときは子無し扱いに縮退する(通常の fallback)。
        return Ok(Children {
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
    Ok(Children {
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

/// 節の行を項目に束ねる(#876)。`Rn` 行は、直後のインデントされた継続行(次の
/// `Rn` 行・インデントの無い行・空行の手前まで)を `\n` でつないで 1 項目にする。
/// 折り返した項目の処分(`— 段1で実装` など)が継続行にあっても、同じ項目として
/// 読むため。`Rn` でない行は、それぞれ 1 項目。
fn inventory_entries(section: &[&str]) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut i = 0;
    while i < section.len() {
        let mut entry = section[i].to_string();
        i += 1;
        if RN_RE.is_match(&entry) {
            while i < section.len() && is_continuation(section[i]) {
                entry.push('\n');
                entry.push_str(section[i]);
                i += 1;
            }
        }
        entries.push(entry);
    }
    entries
}

/// `Rn` 項目の継続行か: 空でなく、空白で始まり、それ自体が `Rn` 行でない。
fn is_continuation(line: &str) -> bool {
    line.starts_with([' ', '\t']) && !line.trim().is_empty() && !RN_RE.is_match(line)
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
    let entries = inventory_entries(&section);
    let mut missing: Vec<String> = Vec::new();
    if children.kind == "sub" {
        for item in &children.items {
            let cnum = tsv_escape(item.get("number").unwrap_or(&Value::Null));
            let ctitle = tsv_escape(item.get("title").unwrap_or(&Value::Null));
            let pat =
                Regex::new(&format!("#{}([^0-9]|$)", regex::escape(&cnum))).expect("valid regex");
            let entry = entries.iter().find(|e| pat.is_match(e));
            if !entry.is_some_and(|e| STAGE_OR_TAG_RE.is_match(e)) {
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
    for entry in inventory_entries(&section) {
        let Some(c) = RN_RE.captures(&entry) else {
            continue;
        };
        let id = c[1].to_string();
        let rest = &entry[c.get(0).expect("match").end()..];
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
        "要求インベントリの検査で問題が見つかりました。requirements-inventory スキルの手順に従って計画を修正してください。\n\n{}\n\n{EXAMPLE_BLOCK}",
        deny_lines.join("\n")
    )
}

fn gate_dir() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_PLAN_SCOPE_GATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".claude/plan-scope-gate")
        })
}

/// hook 本体(`main` が標準入力を読んだあとの処理)。`main` と、ルータ(`crates/hook-router`)が
/// 同じ関数を呼ぶ。返すのは deny だけ(allow / ask は決して返さない)。
///
/// プロセス全体の状態は変えない。作業ディレクトリへは移らず(`HookInput::effective_cwd` の
/// パスを `git -C` と相対パスの解決に使う)、書き込みも標準出力・標準エラーへの出力もしない。
/// 外へ出るのは、`git remote get-url` と `gh api graphql`(・フォールバックの `gh issue view`)の
/// 読み取りだけで、失敗は参照 Issue 単位で黙って諦める(fail-open)。
pub fn hook(raw: &str) -> Option<agent_session::Outcome> {
    // ADR-0005: バイナリの存在でゲートする(gh 不在のマシンでは経路Bも含め沈黙)。
    if !process_control::command_exists("gh") {
        return None;
    }
    if gate_dir().join("skip").exists()
        || std::env::var("SKIP_PLAN_SCOPE_GATE").as_deref() == Ok("1")
    {
        return None;
    }
    // 不正 JSON でも bash 版(jq が空を返す)と同じく最新プランへ縮退する。
    let input = agent_session::HookInput::parse(raw).unwrap_or_default();
    let cwd = input.effective_cwd();
    let plan = agent_session::plan::plan_text_at(&input, cwd.as_deref())?;
    let plan = trim_newlines(plan);
    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let mut deny_lines: Vec<String> = Vec::new();

    // --- 経路A: Issue 起点の実カバレッジ ---
    let transcript = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("transcript_path")?.as_str().map(str::to_string))
        .unwrap_or_default();
    if !transcript.is_empty() && Path::new(&transcript).is_file() {
        let bytes = std::fs::read(&transcript).unwrap_or_default();
        let user_text = trim_newlines(agent_session::transcript::extract_user_text(
            &String::from_utf8_lossy(&bytes),
        ));
        let owner_repo = resolve_owner_repo(&cwd);
        for r in extract_issue_refs(&user_text, &owner_repo) {
            let Some(children) = fetch_children(&r) else {
                continue;
            };
            if let IssueJudgement::Missing(items) = judge_issue(&r, &children, &plan) {
                deny_lines.extend(missing_lines(&r, &items));
            }
        }
    }

    // --- 経路B: インベントリ内整合性 ---
    deny_lines.extend(judge_inventory(&plan));

    if deny_lines.is_empty() {
        return None;
    }
    agent_session::Outcome::deny(deny_message(&deny_lines))
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
    fn stderr_summary_keeps_the_first_lines_only() {
        assert_eq!(stderr_summary(b""), "");
        assert_eq!(stderr_summary(b"\n  \n"), "");
        assert_eq!(
            stderr_summary(b"\n a \nb\n\nc\nd\n"),
            ": a / b / c",
            "先頭の非空 3 行"
        );
        let long = "x".repeat(500);
        assert_eq!(stderr_summary(long.as_bytes()).chars().count(), 2 + 300);
    }

    #[test]
    fn normalize_issue_ref_accepts_full_and_fills_bare_numbers() {
        let origin = || "tarotene/dotfiles".to_string();
        for (arg, want) in [
            ("o/r#1273", "o/r#1273"),
            ("tarotene/dotfiles#136", "tarotene/dotfiles#136"),
            ("a.b/c_d-e#7", "a.b/c_d-e#7"),
            ("#1273", "tarotene/dotfiles#1273"),
            ("1273", "tarotene/dotfiles#1273"),
        ] {
            assert_eq!(
                normalize_issue_ref(arg, origin).as_deref(),
                Ok(want),
                "{arg}"
            );
        }
    }

    #[test]
    fn normalize_issue_ref_rejects_a_malformed_ref_with_the_expected_format() {
        let origin = || "tarotene/dotfiles".to_string();
        // `owner/repo` と番号を連結した形(# が無い)・略記・空・数字以外。
        for arg in [
            "o/r1273",
            "telepath#7",
            "o/r#",
            "o/r#x",
            "",
            "#",
            "abc",
            "o/#1",
            "/r#1",
        ] {
            let err = normalize_issue_ref(arg, origin).unwrap_err();
            assert!(err.contains("owner/repo#N"), "{arg}: {err}");
            assert!(err.contains(&format!("{arg:?}")), "{arg}: {err}");
        }
    }

    #[test]
    fn normalize_issue_ref_needs_an_origin_only_for_bare_numbers() {
        let none = || String::new();
        assert!(normalize_issue_ref("o/r#1", none).is_ok());
        for arg in ["#1", "1"] {
            let err = normalize_issue_ref(arg, none).unwrap_err();
            assert!(
                err.contains("cwd is not inside a GitHub-remote repo"),
                "{err}"
            );
        }
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

    /// #876: 折り返した項目の処分は、継続行にあっても同じ項目として読む。
    #[test]
    fn judge_inventory_reads_indented_continuation_lines() {
        // Issue の再現(補足が長くて折り返し、処分は次の行)。
        assert!(judge_inventory(
            "## 要求インベントリ\n\n- R1: <依頼文の逐語>(補足が長くて折り返した部分)\n  — 段1で実装\n"
        )
        .is_empty());
        // 継続行が複数、タブ字下げ、閉じたタグ。
        assert!(judge_inventory(
            "## 要求インベントリ\n- R1: 長い逐語\n  さらに折り返し\n\t— Obsolete: もう不要\n- R2: 段2で実装\n"
        )
        .is_empty());
        // 継続行に処分が無ければ、従来どおり未記載。次の Rn 行の処分は借りない。
        assert_eq!(
            judge_inventory("## 要求インベントリ\n- R1: 逐語\n  折り返しだけ\n- R2: 段2で実装\n"),
            vec!["R1: 処分(実装する段、または棄却タグ)が未記載です"]
        );
        // 継続行は空行・インデントの無い行・見出しで終わる。
        assert_eq!(
            judge_inventory("## 要求インベントリ\n- R1: 逐語\n\n  段1で実装\n"),
            vec!["R1: 処分(実装する段、または棄却タグ)が未記載です"]
        );
        assert_eq!(
            judge_inventory("## 要求インベントリ\n- R1: 逐語\n段1で実装\n"),
            vec!["R1: 処分(実装する段、または棄却タグ)が未記載です"]
        );
        assert_eq!(
            judge_inventory("## 要求インベントリ\n- R1: 逐語\n## 別の節\n  段1で実装\n"),
            vec!["R1: 処分(実装する段、または棄却タグ)が未記載です"]
        );
    }

    /// #876: 経路A(子 Issue の照合)も、折り返した項目の処分を同じ項目として読む。
    #[test]
    fn judge_issue_reads_indented_continuation_lines() {
        let sub = ch(
            json!({"totalCount":2,"kind":"sub","items":[{"number":137,"title":"t1"},{"number":138,"title":"t2"}]}),
        );
        let r = "tarotene/dotfiles#136";
        assert_eq!(
            judge_issue(
                r,
                &sub,
                "## 要求インベントリ\n- R1: #137 子タイトル逐語(長くて折り返した)\n  — 段1で実装\n- R2: #138 t2\n  — Obsolete: 済\n"
            ),
            IssueJudgement::Covered
        );
        assert_eq!(
            judge_issue(
                r,
                &sub,
                "## 要求インベントリ\n- R1: #137 子タイトル逐語\n  折り返しだけ\n- R2: #138 段2で実装\n"
            ),
            IssueJudgement::Missing(vec!["#137 t1".into()])
        );
    }
}
