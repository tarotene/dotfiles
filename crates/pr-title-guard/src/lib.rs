//! PR タイトルを commit-message 契約として作成時に機械強制する PreToolUse
//! hook の判定(ADR-0031、設計と根拠は docs/claude/pr-title-contract.md)。
//!
//! bash 版 `config/claude/hooks/pr-title-guard.sh` と、その Codex/Copilot
//! adapter の移植(ADR-0024 Stage 4a、#415)。
//!
//! 対象コマンド: コマンド位置の `gh pr create --title ...` と
//! `gh pr edit ... --title ...`(`-R/--repo` によるクロスリポジトリ指定にも
//! 対応)。文法検査は `pr-title-check` クレートの `check_title` を直接呼ぶ —
//! bash 版は `scripts/pr-title-check` を子プロセスで呼んでいた(checker の
//! 単一正本、client guard とサーバ側 required check が同じ判定根拠を共有)
//! が、Rust では同じ判定関数を共有すれば足りる。
//!
//! 発火は owner が tarotene のリポジトリ、または**呼び出し先リポジトリ自身が
//! PR タイトル契約の workflow(`.github/workflows/pr-title.yml`)を持つ**場合に
//! 限定する(ADR-0031 D4 と、その 2026-10-13 の追補)。owner が解決できない場合は
//! fail-open(通す)。
//!
//! escape hatch: 環境変数 `PR_TITLE_GUARD_ALLOW=1` で一時的に無効化する
//! (`No-Issue:` のような本文タグ型ではない — 単発の緊急対応向けの一時解除
//! であり、本文に恒久的に残す決定ではないため)。

use guard_core::command::{first_deny, gh_command_at};
use guard_core::gh::{scan_value_flags, ValueFlag};
use guard_core::hook::{jq_r_path, ToolCall};
use guard_core::repo::owner_repo;
use pr_title_check::{check_title, Verdict};
use std::path::Path;

/// `is_target_at` の種別。bash の `TARGET_KIND`(create|edit)。判定は両者で
/// 同じなので値は使わない。
#[derive(Debug, Clone, Copy)]
pub struct Kind;

/// bash 版 `is_target_at`: `gh pr create` / `gh pr edit`。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<Kind> {
    (gh_command_at(tokens, i, &["pr", "create"]) || gh_command_at(tokens, i, &["pr", "edit"]))
        .then_some(Kind)
}

/// `owner/repo`(または repo 単体)の owner が tarotene か。
fn is_tarotene_owned(nwo: &str) -> bool {
    nwo.split('/').next() == Some("tarotene")
}

/// 作業ディレクトリが属するリポジトリが、PR タイトル契約の workflow を自分で
/// 持っているか(ADR-0031 D4 の追補、2026-10-13)。`repo-governance-common` の
/// テンプレートが播く `pr-title.yml` は、サーバ側の required check と同じ文法
/// (`pr-title-check`)を呼ぶ。それがあるリポジトリでは、最下段の PR タイトルが
/// CI で落ちる前に client で止めてよい。リポジトリ外・解決不能は `false`。
fn has_pr_title_workflow(project: &Path) -> bool {
    let top = std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match top {
        Some(top) if !top.is_empty() => Path::new(&top)
            .join(".github/workflows/pr-title.yml")
            .is_file(),
        _ => false,
    }
}

/// コマンド文字列全体を判定する(`decide_pr_title`)。deny なら理由を返す。
/// `allow` は `PR_TITLE_GUARD_ALLOW=1`(呼び出し側が env から決める)。
pub fn decide(cmd: &str, project: &Path, allow: bool) -> Option<String> {
    first_deny(cmd, is_target_at, |range| {
        // 1 パスで読む(`--title -R` の `-R` を `--repo` と誤読しない)。
        let f = scan_value_flags(
            range.tokens,
            &[
                ValueFlag {
                    names: &["--title", "-t"],
                    eq_prefix: "--title=",
                },
                ValueFlag {
                    names: &["--repo", "-R"],
                    eq_prefix: "--repo=",
                },
            ],
        );
        let (title, repo) = (&f[0], &f[1]);

        if allow {
            return None;
        }
        let nwo = if repo.value.is_empty() {
            owner_repo(project)?
        } else {
            repo.value.clone()
        };
        // owner が tarotene か、対象が「いま居るリポジトリ」でそこに契約の workflow
        // がある場合だけ発火する。`-R` で別のリポジトリを指しているときは、その
        // リポジトリの中身を見られないので workflow の有無では発火させない。
        let in_this_repo = owner_repo(project).as_deref() == Some(nwo.as_str());
        if !is_tarotene_owned(&nwo) && !(in_this_repo && has_pr_title_workflow(project)) {
            return None;
        }
        // タイトルが分からない(--title 未指定 = --web でエディタ入力等)場合は
        // 判定不能で通す — ローカルで内容を検査できない。
        if !title.present || title.value.is_empty() {
            return None;
        }
        if check_title(title.value.as_bytes()) == Verdict::Conforming {
            return None;
        }
        Some(format!(
            "PR タイトル '{}' は commit-message 契約(ADR-0031)に非適合です。'type(scope)?!?: subject' 形式(type は feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)にしてください。一時的に無効化するには PR_TITLE_GUARD_ALLOW=1 を設定してください。",
            title.value
        ))
    })
}

/// hook 入力から deny 理由を求める(bash の `main()` / `main_codex` /
/// `main_copilot`)。判定不能はすべて `None`(fail-open)。
///
/// project は Claude のみ `CLAUDE_PROJECT_DIR` を優先し、無ければ `.cwd`。
/// Codex/Copilot adapter は `.cwd` だけを見ていた(env を見ない)。
pub fn check(call: &ToolCall, claude_project_dir: Option<&str>, allow: bool) -> Option<String> {
    use hook_io::Agent;
    let env_project = match call.agent {
        Agent::Claude => claude_project_dir.filter(|s| !s.is_empty()),
        Agent::Codex | Agent::Copilot => None,
    };
    let project = match env_project {
        Some(p) => p.to_string(),
        None => jq_r_path(&call.raw, &["cwd"])
            .ok()
            .flatten()
            .unwrap_or_default(),
    };
    if project.is_empty() {
        return None;
    }
    if !is_inside_work_tree(Path::new(&project)) {
        return None;
    }
    let cmd = call.bash_command()?;
    decide(&cmd, Path::new(&project), allow)
}

/// `git -C <project> rev-parse --is-inside-work-tree` が成功するか。
fn is_inside_work_tree(project: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
