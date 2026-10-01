//! docs ドメイン(ADR-640): スタック標準の API docs を CI で厳格にビルド
//! しているか。適用可否は主言語ではなく閉じた manifest → ツール表で決める。

use super::workflows::line_uses;
use crate::jq;
use crate::model::{blob_text, Finding, RepoGql, RepoRest, Verdict};
use serde_json::Value;

/// ADR-640 D3 の閉じた表(stack, tarotene/dotfiles/.github/actions/ 下の
/// composite action 名)。この配列が「どのスタックが docs を建てるべきか」の
/// 単一正本で、ここに無いスタックは not-applicable(drift ではない)。行を
/// 足すときは `.github/actions/docs-<stack>` と [`docs_stacks_for`] の検出も足す。
pub const DOCS_STACK_ACTIONS: [(&str, &str); 3] = [
    ("rust", "docs-rust"),
    ("python", "docs-python"),
    ("typescript", "docs-typescript"),
];

/// bash: `docs_stacks_for`。Cargo.toml → rust、pyproject.toml → python、
/// ライブラリの入口(exports / main / types)を宣言する package.json →
/// typescript。読めない package.json は非ライブラリ扱い(Astro サイトや
/// アプリを「API docs が無い」と誤検出するより偽陰性の方がよい)。
pub fn docs_stacks_for(gql: &RepoGql) -> Vec<&'static str> {
    let mut out = Vec::new();
    if gql.cargo_toml.is_some() {
        out.push("rust");
    }
    if gql.pyproject_toml.is_some() {
        out.push("python");
    }
    let pkg = gql
        .package_json
        .as_ref()
        .and_then(|b| b.text.as_deref())
        .unwrap_or("");
    if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(pkg) {
        if ["exports", "main", "types"]
            .iter()
            .any(|k| m.contains_key(*k))
        {
            out.push("typescript");
        }
    }
    out
}

fn ends_token(rest: &str) -> bool {
    rest.chars().next().is_none_or(jq::is_space)
}

/// bash: `judge_docs`。`rest` は PRIVATE のときだけ取る REST(Pages の状態)。
/// docs job が `ci-passed.needs` に入っているかは workflows ドメインの仕事
/// (ADR-591 D1)なのでここでは見ない。
pub fn judge_docs(repo: &str, visibility: &str, gql: &RepoGql, rest: &RepoRest) -> Finding {
    let ci = blob_text(&gql.decl_ci);
    let lines: Vec<&str> = ci.split('\n').collect();
    let any_line = |target: &str, tail: &dyn Fn(&str) -> bool| {
        lines.iter().any(|l| line_uses(l, target, tail))
    };
    let stacks = docs_stacks_for(gql);
    let mut missing: Vec<String> = Vec::new();
    for stack in &stacks {
        let action = DOCS_STACK_ACTIONS
            .iter()
            .find(|(s, _)| s == stack)
            .map(|(_, a)| *a)
            .unwrap_or("");
        // tarotene/dotfiles は自前の action を相対パスで適用する(action を
        // 変える PR がその PR 自身で検査される)。他リポジトリは @main 参照
        // (ADR-640 D2: 厳格な基準を 1 箇所に置き、全リポジトリを一度に変える)。
        if repo == "dotfiles" {
            if !any_line(&format!("./.github/actions/{action}"), &ends_token) {
                missing.push(format!("docs-absent:{stack}"));
            }
        } else if any_line(
            &format!("tarotene/dotfiles/.github/actions/{action}@main"),
            &ends_token,
        ) {
        } else if any_line(
            &format!("tarotene/dotfiles/.github/actions/{action}@"),
            &|_| true,
        ) {
            missing.push(format!("docs-wrong-ref:{stack}"));
        } else {
            missing.push(format!("docs-absent:{stack}"));
        }
    }
    // 個人アカウントでは private リポジトリの Pages サイトは公開される
    // (ADR-640 D7)ので、スタックに関わらず開示リスク。
    if visibility == "PRIVATE" && rest.has_pages == Some(true) {
        missing.push("private-pages-enabled".into());
    }
    if missing.is_empty() {
        if stacks.is_empty() {
            Finding::not_applicable()
        } else {
            Finding::plain(Verdict::Ok, Vec::new())
        }
    } else {
        Finding::plain(Verdict::Drifted, missing)
    }
}
