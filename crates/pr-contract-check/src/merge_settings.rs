//! pr-merge-settings-check — PR タイトル契約(ADR-0031)の前提となる merge 設定
//! (squash-only、`squash_merge_commit_title=PR_TITLE`、`squash_merge_commit_message=BLANK`
//! 等)が生きているかを、required check(`.github/actions/pr-title`)として検査する。
//!
//! 判定は [`crate::settings::judge_settings`] の単一正本(`crates/github-audit` の
//! settings ドメインと同じ関数)。audit(手動実行、検出のみ)とは違い、この検査は
//! fail-closed — 設定取得に失敗した場合も red にする。契約の前提が確認できないまま
//! merge を通さないことが目的で、再実行で復旧できる。
//!
//! 例外: `dependabot-security-updates-enabled`(`.security_and_analysis.
//! dependabot_security_updates.status` から出す code)はここでは検査しない。GitHub REST
//! API の `GET /repos/{owner}/{repo}` は `security_and_analysis` ブロックを repository
//! admin 権限を持つ token でしか返さない(GitHub Docs "Get a repository" — "you must
//! have admin permissions for the repository" — 取得 2026-09-29)。`GITHUB_TOKEN`
//! (このワークフローが使う token)に付与できる permission の一覧に `administration` は
//! 存在しない(GitHub Docs "Workflow syntax for GitHub Actions" の permissions テーブル、
//! 取得 2026-09-29)ため、この token では `security_and_analysis` が構造的に永久に読めない —
//! 再実行やキャッシュ伝播待ちでは直らない。この項目のライブ drift 検出は
//! `github-audit` の settings ドメイン(人間の個人 token で実行)に任せ、ここでは
//! 対象外にする。

use crate::gh::Gh;
use crate::settings::{judge_settings, Settings, GH_PROJECTION};
use crate::verdict::Verdict;

pub const USAGE: &str = "\
usage: pr-merge-settings-check <owner>/<repo>

指定リポジトリの merge 設定が ADR-0031 の前提(squash-only、
squash_merge_commit_title=PR_TITLE、squash_merge_commit_message=BLANK 等)を
満たすかを検査する。判定は crates/github-audit の judge_settings と同じ関数
(crates/pr-contract-check の settings)。設定取得に失敗した場合も non-zero
(fail-closed)。
";

/// `GITHUB_TOKEN` では構造的に検証できない code(上のコメント参照)。
const UNVERIFIABLE_BY_GITHUB_TOKEN: &[&str] = &["dependabot-security-updates-enabled"];

/// `owner/repo` の merge 設定を検査する。
pub fn check_repo(gh: &Gh, owner_repo: &str) -> Verdict {
    let out = gh.api(&format!("repos/{owner_repo}"), &["--jq", GH_PROJECTION]);
    let settings = out
        .ok
        .then(|| Settings::from_tsv(out.stdout.lines().next().unwrap_or("")))
        .flatten();
    let Some(settings) = settings else {
        return Verdict::fail_closed(format!(
            "pr-merge-settings-check: {owner_repo} の merge 設定取得に失敗しました(fail-closed で red にします)"
        ));
    };

    let missing: Vec<&str> = judge_settings(&settings)
        .into_iter()
        .filter(|c| !UNVERIFIABLE_BY_GITHUB_TOKEN.contains(c))
        .collect();
    if !missing.is_empty() {
        return Verdict::fail_closed(format!(
            "pr-merge-settings-check: {owner_repo} の merge 設定が ADR-0031 の前提から drift しています: {}",
            missing.join(", ")
        ));
    }
    Verdict {
        code: 0,
        out: vec![format!("pr-merge-settings-check: {owner_repo} OK")],
        err: vec![],
    }
}

/// CLI の入口。`args` は `argv[1..]`。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--selftest") => {
            eprintln!(
                "pr-merge-settings-check: --selftest は `cargo test -p pr-contract-check` に移った"
            );
            0
        }
        Some("-h" | "--help") => {
            print!("{USAGE}");
            0
        }
        None => {
            print!("{USAGE}");
            1
        }
        Some(a) if a.contains('/') => check_repo(&Gh::from_env("GITHUB_AUDIT_GH_BIN"), a).emit(),
        Some(_) => {
            eprint!("{USAGE}");
            2
        }
    }
}
