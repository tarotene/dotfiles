//! 決定成果物(ADR / 設計文書 / skill)の追加を、その執行点と同じ PR に
//! 作成時から機械強制する PreToolUse hook(ADR-396)。
//!
//! bash 版 `config/claude/hooks/decision-colocation-guard.sh` の移植。
//! 設計と根拠は docs/claude/decision-colocation.md。
//!
//! 対象コマンド: コマンド位置の `gh pr create`(`-R/--repo` によるクロス
//! リポジトリ指定にも対応)。判定は [`crate::check::run_check`] 1 本に
//! 一元化する(client guard とサーバ側 required check が同じ判定根拠を
//! 共有する — pr-title-guard / adr-number-check と同じ型)。bash 版は
//! `scripts/decision-colocation-check` を子プロセスで呼んでいたが、同じ
//! クレートの関数を直接呼ぶ(「どの checker を呼ぶか」の解決
//! `DECISION_COLOCATION_CHECK_BIN` / PATH フォールバックは不要になった)。
//!
//! base の解決順: 呼び出しの `--base`/`-B` フラグ → `default_branch`
//! (origin/HEAD symref → `gh repo view`)。解決できた base がローカルに
//! 存在しない場合(worktree で `origin/<base>` のみ持つケースを含む)は
//! `origin/<base>` にフォールバックする。どちらも無ければ判定不能で通す
//! (ADR-0005 の binary-existence gating と同じ fail-open 方針)。
//!
//! escape hatch: 環境変数 `SKIP_DECISION_COLOCATION_GUARD=1` で一時的に
//! 無効化する(`PR_TITLE_GUARD_ALLOW` と同じ性質 — 恒久的な本文タグでは
//! ない)。

use std::path::Path;
use std::process::{Command, Stdio};

use guard_core::command::{first_deny, gh_command_at};
use guard_core::gh::{scan_value_flags, ValueFlag};
use guard_core::repo::{default_branch_or_gh, owner_repo};

use crate::check;

/// checker の判定(bash 版の終了コード 0 / 1 / それ以外)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// 適合(rc=0)-> 通す。
    Conforming,
    /// 非適合(rc=1)。checker の出力(違反メッセージ)を持つ -> deny。
    NonConforming(String),
    /// 判定不能(rc=2 等)-> 通す。
    Indeterminate,
}

/// 実際の checker: `project` の git 作業ツリー先頭で [`check::run_check`]
/// を走らせる(bash 版は `cd "$project" && decision-colocation-check --base
/// "$base_ref"`、checker 側が `git rev-parse --show-toplevel || pwd`)。
///
/// 現在の `run_check` は判定不能を返さない(bash 版でも rc=2 は使い方誤りの
/// ときだけで、guard は常に正しい引数で呼ぶ)。
pub fn real_checker(project: &Path, base_ref: &str) -> CheckOutcome {
    let root = hook_io::git::toplevel(project).unwrap_or_else(|| project.to_path_buf());
    let violations = check::run_check(&root, base_ref);
    if violations.is_empty() {
        CheckOutcome::Conforming
    } else {
        CheckOutcome::NonConforming(violations.join("\n"))
    }
}

fn git_ok(project: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// bash の `resolve_ref`: ローカルに実在すればそのまま、無ければ
/// `origin/<branch>` にフォールバックする ref。どちらも無ければ `None`。
pub fn resolve_ref(project: &Path, branch: &str) -> Option<String> {
    if git_ok(project, &["rev-parse", "--verify", "--quiet", branch]) {
        return Some(branch.to_string());
    }
    let origin = format!("origin/{branch}");
    git_ok(project, &["rev-parse", "--verify", "--quiet", &origin]).then_some(origin)
}

/// deny の理由文の本体(checker の出力 `out` を埋め込む)。
fn deny_reason(out: &str) -> String {
    format!(
        "決定成果物(ADR/設計文書/skill)の新規追加、または既存 ADR への Amendment 追加が、その執行点(実際に実装する変更)を同じ PR に伴っていません(config/agents/AGENTS.md「決定成果物は執行点と同じ PR に出す」、docs/adr/396-decision-colocation.md)。詳細:
{out}
一時的に無効化するには SKIP_DECISION_COLOCATION_GUARD=1 を設定してください。"
    )
}

/// PreToolUse の deny は Bash 呼び出し全体に効く。`gh pr create` より前に
/// 文があれば(本文ファイルの生成など)それも実行されていない(#668)。
/// deny を部分的にかけることは仕組み上できないので警告で補う(軸: 検出のみ、
/// docs/claude/decision-colocation.md「複合コマンドの deny は部分的にできない」)。
const PRECEDING_STATEMENT_NOTE: &str = "注意: この Bash 呼び出しは全体が実行されていません。gh pr create より前の文(PR 本文ファイルの生成など)も実行されていないので、指摘を直したうえで、前段の文と gh pr create を別の呼び出しに分けて再実行してください。";

/// 判定の設定(環境変数・checker を差し替えられるようにする)。
pub struct Guard<'a> {
    /// `SKIP_DECISION_COLOCATION_GUARD=1`。
    pub skip: bool,
    /// `(project, base_ref)` で checker を走らせる。
    pub checker: &'a dyn Fn(&Path, &str) -> CheckOutcome,
}

impl Guard<'_> {
    /// 環境変数と実際の checker を使う既定の設定で `f` を走らせる。
    pub fn with_env<R>(f: impl FnOnce(&Guard<'_>) -> R) -> R {
        let skip = std::env::var("SKIP_DECISION_COLOCATION_GUARD").as_deref() == Ok("1");
        f(&Guard {
            skip,
            checker: &real_checker,
        })
    }

    /// 1 つの `gh pr create` 範囲(トークン列)の判定。deny なら理由。
    ///
    /// bash の `judge_range`: 通す条件はすべて `return 1`(escape hatch・
    /// base 解決不能・適合・判定不能)。
    fn judge_range(&self, project: &Path, tokens: &[String]) -> Option<String> {
        let f = scan_value_flags(
            tokens,
            &[
                ValueFlag {
                    names: &["--base", "-B"],
                    eq_prefix: "--base=",
                },
                ValueFlag {
                    names: &["--repo", "-R"],
                    eq_prefix: "--repo=",
                },
            ],
        );
        let (base, repo) = (&f[0], &f[1]);

        if self.skip {
            return None;
        }

        let nwo = if repo.value.is_empty() {
            owner_repo(project).unwrap_or_default()
        } else {
            repo.value.clone()
        };

        let branch = if base.present {
            base.value.clone()
        } else {
            if nwo.is_empty() {
                return None;
            }
            default_branch_or_gh(project, &nwo).unwrap_or_default()
        };
        if branch.is_empty() {
            return None;
        }

        let base_ref = resolve_ref(project, &branch)?;
        match (self.checker)(project, &base_ref) {
            CheckOutcome::NonConforming(out) => Some(deny_reason(&out)),
            CheckOutcome::Conforming | CheckOutcome::Indeterminate => None,
        }
    }

    /// bash の `decide_colocation`: コマンド文字列全体から `gh pr create` の
    /// 範囲を切り出し、最初の deny 理由を返す(複合コマンドの前段警告つき)。
    pub fn decide(&self, cmd: &str, project: &Path) -> Option<String> {
        // 範囲ごとの前段有無(`start > 0`)を理由に付けるため、judge の中で
        // 理由を組み立てる。
        first_deny(
            cmd,
            |t, i| gh_command_at(t, i, &["pr", "create"]).then_some(()),
            |r| {
                let mut reason = self.judge_range(project, r.tokens)?;
                if r.start > 0 {
                    reason.push('\n');
                    reason.push_str(PRECEDING_STATEMENT_NOTE);
                }
                Some(reason)
            },
        )
    }
}
