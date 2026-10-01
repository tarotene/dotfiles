//! titles ドメイン(ADR-0031 + Amendment 2026-09-26)。
//!
//! PR タイトル契約の強制機構が「在って動いているか」だけを見る。個々の open
//! PR のタイトルは client guard と required check の仕事で、ここでも判定すると
//! CI が緑なのに drift と出る(docs/claude/pr-title-contract.md)。

use super::rulesets::required_contexts;
use crate::gh::Gh;
use crate::model::{Finding, RepoGql};
use serde_json::Value;

/// dotfiles 自身は pull_request で自己適用するので連結されない単一名。
pub const REQUIRED_PR_TITLE_CONTEXT_SELF: &str = "PR title";
/// 他リポジトリは workflow_call 経由なので「呼び出し側 **job** の name /
/// 呼び出される job の name」に連結される(telepath#243 の Actions API 応答で
/// 実測、2026-09-26)。呼び出し側テンプレートは job に `name: PR Title` を
/// 固定しているので常にこの文字列。
///
/// 経緯: 当初は「呼び出し側 **workflow** の name」という誤読(GitHub
/// Community Discussion #46752、ADR-0031 D2 追補、2026-09-22)から接尾辞
/// ` / PR title` の後方一致で ok にしていた。そのせいで呼び出し側 job に
/// name が無く check 名が "check / PR title" になっていた #337 の播きを ok と
/// 誤判定し続けた(15 リポジトリで required check が永久に Expected、
/// 2026-09-26 発覚)。完全一致に締め、実 run の job 名との突き合わせを足した。
pub const REQUIRED_PR_TITLE_CONTEXT_CONNECTED: &str = "PR Title / PR title";

/// bash: `judge_titles`。`run_jobs` は pr-title.yml の最新 run の job 名
/// (空 = run 未実行・取得不能)。
pub fn judge_titles(gh: &Gh, repo: &str, gql: &RepoGql, run_jobs: &[String]) -> Finding {
    // CI を持たない repo には required check を要求する土台が無い(renovate と
    // 同じ判定)。ruleset の取得もしない。
    if !gql.has_workflows() {
        return Finding::not_applicable();
    }
    judge_titles_with(&gh.default_branch_rulesets(repo), gql, run_jobs)
}

/// `judge_titles` の純粋部分(default branch の ruleset 詳細を受け取る)。
pub fn judge_titles_with(rulesets: &[Value], gql: &RepoGql, run_jobs: &[String]) -> Finding {
    if !gql.has_workflows() {
        return Finding::not_applicable();
    }
    let mut missing = Vec::new();
    let has_caller = gql.has_workflow_file("pr-title.yml");
    if !has_caller {
        missing.push("pr-title-workflow-missing".into());
    }
    let checks = required_contexts(rulesets);
    if !checks
        .iter()
        .any(|c| c == REQUIRED_PR_TITLE_CONTEXT_SELF || c == REQUIRED_PR_TITLE_CONTEXT_CONNECTED)
    {
        missing.push("pr-title-check-not-required".into());
    }
    // ground truth との突き合わせ: run が一度も無ければ判定材料が無いので
    // 何もしない(#337 は「run はあったが誰も見なかった」ケース)。
    if has_caller && !run_jobs.is_empty() && !checks.iter().any(|c| run_jobs.contains(c)) {
        missing.push("pr-title-context-mismatch".into());
    }
    Finding::ok_or_drifted(missing)
}
