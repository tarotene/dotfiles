//! `gh` の呼び出し(外部コマンド)。引数の並びは bash 版と同じにしてある
//! (テストの gh スタブは `$*` の部分一致で応答を選ぶ)。stderr は常に捨てる。

use crate::trim_nl;
use std::process::{Command, Stdio};

/// `gh <args>` の (成功したか, stdout 末尾改行除去)。起動できなければ `None`。
pub fn gh(args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new("gh")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some((
        out.status.success(),
        trim_nl(&String::from_utf8_lossy(&out.stdout)).to_string(),
    ))
}

/// 成功時の stdout。失敗・起動不能は `None`。
pub fn gh_ok(args: &[&str]) -> Option<String> {
    match gh(args)? {
        (true, out) => Some(out),
        _ => None,
    }
}

/// `x="$(gh …)" || x=""; [[ -n "$x" ]] || x='[]'`。
fn or_empty_array(out: Option<String>) -> String {
    out.filter(|s| !s.is_empty())
        .unwrap_or_else(|| "[]".to_string())
}

/// 現在のブランチを head とする open PR(最大 1 件)の JSON 配列。
pub fn pr_for_branch(nwo: &str, branch: &str, fields: &str) -> String {
    or_empty_array(gh_ok(&[
        "pr", "list", "-R", nwo, "--head", branch, "--state", "open", "--limit", "1", "--json",
        fields,
    ]))
}

/// G_pr の merged/closed 検査(`--state all`)。gh 失敗は `None`(fail-open)。
pub fn pr_for_branch_any_state(nwo: &str, branch: &str) -> Option<String> {
    gh_ok(&[
        "pr", "list", "-R", nwo, "--head", branch, "--state", "all", "--limit", "1", "--json",
        "state",
    ])
    .map(|s| if s.is_empty() { "[]".to_string() } else { s })
}

/// G_stack 用: open PR の一覧(head/base のチェーンをたどる材料)。
pub fn open_prs(nwo: &str) -> Option<String> {
    gh_ok(&[
        "pr",
        "list",
        "-R",
        nwo,
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "number,headRefName,baseRefName",
    ])
}

/// judge_handoff 用: Issue の state(`OPEN` 等)。取得できなければ `None`。
pub fn issue_state(num: &str, nwo: &str) -> Option<String> {
    gh_ok(&[
        "issue", "view", num, "-R", nwo, "--json", "state", "-q", ".state",
    ])
}

/// gh-stack 拡張が導入済みか。`--version` がバイナリの唯一の存在確認手段
/// (サブコマンド一覧に "installed" フラグは無い)。
pub fn stack_extension_available() -> bool {
    matches!(gh(&["stack", "--version"]), Some((true, _)))
}

/// `repos/<nwo>/stacks` の生 JSON。取得不能(404・機能撤収・ネットワーク障害の
/// いずれか区別しない)なら `None`。
pub fn stacks(nwo: &str) -> Option<String> {
    gh_ok(&["api", &format!("repos/{nwo}/stacks")])
}

/// ruleset の `rules/branches/<base>`(base の `/` は `%2F`)。gh 自体の失敗は `None`。
pub fn branch_rules(nwo: &str, base: &str) -> Option<String> {
    let enc = base.replace('/', "%2F");
    gh_ok(&["api", &format!("repos/{nwo}/rules/branches/{enc}")])
}

/// `gh pr checks --json` の応答(失敗・空は `[]`)。
///
/// pending のチェックがあると gh は exit 8 を返す(`gh pr checks --help` の
/// "Additional exit codes")。終了コードでなく、標準出力が配列として読めるかで採る。
pub fn reported_checks(pr_num: &str, nwo: &str) -> String {
    let out = gh(&[
        "pr",
        "checks",
        pr_num,
        "-R",
        nwo,
        "--json",
        "name,bucket,link,workflow",
    ])
    .map(|(_, s)| s)
    .filter(|s| s.trim_start().starts_with('['));
    or_empty_array(out)
}

/// PR の base ブランチ名。取得できなければ `None`。
pub fn pr_base(pr_num: &str, nwo: &str) -> Option<String> {
    gh_ok(&[
        "pr",
        "view",
        pr_num,
        "-R",
        nwo,
        "--json",
        "baseRefName",
        "-q",
        ".baseRefName",
    ])
    .filter(|s| !s.is_empty())
}

/// 現在のブランチの PR 番号(`pr-checks-wait` で PR を省いたとき)。
pub fn current_pr_number(nwo: &str) -> Option<String> {
    gh_ok(&["pr", "view", "-R", nwo, "--json", "number", "-q", ".number"]).filter(|s| !s.is_empty())
}

/// リポジトリの既定ブランチ名。取得できなければ `None`。
pub fn default_branch(nwo: &str) -> Option<String> {
    gh_ok(&[
        "repo",
        "view",
        nwo,
        "--json",
        "defaultBranchRef",
        "-q",
        ".defaultBranchRef.name",
    ])
    .filter(|s| !s.is_empty())
}
