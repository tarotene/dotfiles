//! rulesets-context-check — required check の context 文字列が、この PR の head で
//! 実際に走る job 名として報告されるかを検査する(ADR-503)。
//!
//! `scripts/pr-title-context-check`(ADR-0031 Amendment 2026-09-26)の一般化版。
//! あちらは「PR title」1 context だけを自己照合したが、これは宣言
//! (`<repo>/.github/rulesets/quality.json`)と live な branch ruleset の
//! `required_status_checks` を丸ごと対象にする — required context の正本を対象
//! リポジトリ自身の宣言に一本化した以上(D1)、実測との整合を検査する対象も
//! PR title だけに留める理由が無い。
//!
//! 読むのは `repos/{owner}/{repo}/contents/.github/rulesets/quality.json`、
//! `repos/{owner}/{repo}/rules/branches/{branch}`、`repos/{owner}/{repo}/actions/runs`
//! (`head_sha` フィルタ)だけ。

use crate::gh::{self, Gh};
use crate::verdict::Verdict;
use std::collections::BTreeSet;
use std::time::Duration;

pub const USAGE: &str = "\
usage: rulesets-context-check <owner>/<repo> <head_sha> <default_branch>

<head_sha> が実際に報告する job 名(Actions API、head_sha フィルタで複数
run を合算)を実測し、次の 2 つと突合する:

  - 宣言: <owner>/<repo>@<head_sha> の .github/rulesets/quality.json の
    required_status_checks[].context
  - live: <default_branch> に有効な branch ruleset の
    required_status_checks[].context

判定:
  - live の context に <head_sha> が報告しない ものがあれば FAIL
    (remedy: 宣言を merge 後 `apply-rulesets.sh <owner>/<repo> --reconcile`)。
  - 宣言の context に <head_sha> が報告しない ものがあれば FAIL。
  - 宣言 ≠ live(どちらも実測で満たされている)なら WARN(apply pending)。
  - 宣言が存在しない(まだ播いていない)場合は live 側だけを検査し、
    宣言欠落は warning に留める(github-audit の
    rulesets-declaration-missing が該当)。
  - API 取得に失敗した場合は fail-closed。
";

/// GitHub から取得できなかった(fail-closed にする)ことを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchFailed;

/// 文字列の集合(jq の `unique` と同じく、重複なしで、コードポイント順)。
pub type Names = BTreeSet<String>;

/// 実測の待ち方(秒)。環境変数 `RULESETS_CONTEXT_CHECK_TIMEOUT` / `_INTERVAL`。
#[derive(Debug, Clone, Copy)]
pub struct Poll {
    pub timeout: u64,
    pub interval: u64,
}

impl Poll {
    pub fn from_env() -> Poll {
        let get = |k: &str, default: u64| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        Poll {
            timeout: get("RULESETS_CONTEXT_CHECK_TIMEOUT", 90),
            interval: get("RULESETS_CONTEXT_CHECK_INTERVAL", 10),
        }
    }
}

const REQUIRED_CONTEXTS_JQ: &str = "[.rules[]? | select(.type == \"required_status_checks\") \
| .parameters.required_status_checks[]?.context] | unique | .[]";

/// 宣言の context。`Ok(None)` はファイルが無い(404 — まだ播いていない正常な状態で、
/// github-audit の rulesets-declaration-missing が担当するので fail-closed にしない)、
/// `Err` はそれ以外の取得失敗。
pub fn fetch_declared(gh: &Gh, owner_repo: &str, sha: &str) -> Result<Option<Names>, FetchFailed> {
    let raw = gh.raw_file(owner_repo, ".github/rulesets/quality.json", sha);
    if !raw.ok {
        if raw.stderr.contains("404") || raw.stderr.contains("Not Found") {
            return Ok(None);
        }
        return Err(FetchFailed);
    }
    if raw.stdout.is_empty() {
        return Ok(None);
    }
    let lines = gh::jq(&["-r", REQUIRED_CONTEXTS_JQ], &raw.stdout).ok_or(FetchFailed)?;
    Ok(Some(gh::split_lines(&lines).into_iter().collect()))
}

/// default branch に有効な branch ruleset の context。取得に失敗したら `Err`。
pub fn fetch_live(gh: &Gh, owner_repo: &str, branch: &str) -> Result<Names, FetchFailed> {
    let jq = "[.[] | select(.type == \"required_status_checks\") \
| .parameters.required_status_checks[]?.context] | unique | .[]";
    gh.lines(
        &format!("repos/{owner_repo}/rules/branches/{branch}"),
        &[],
        jq,
    )
    .map(|l| l.into_iter().collect())
    .ok_or(FetchFailed)
}

/// `plan-free-private` トピックが付いているか(docs/agents/free-plan-declaration.md)。
/// 取得に失敗したときは宣言なしとして扱う — 従来どおり live を取りに行き、取れなければ
/// fail-closed になる。
pub fn plan_free_private(gh: &Gh, owner_repo: &str) -> bool {
    gh.lines(&format!("repos/{owner_repo}"), &[], "(.topics // [])[]")
        .is_some_and(|t| {
            t.iter()
                .any(|t| t == crate::settings::PLAN_FREE_PRIVATE_TOPIC)
        })
}

/// あるコミットに対して起動した全 workflow の run が報告した job 名の和集合
/// (ADR-503)。reusable workflow・matrix・複数トリガーの並走を吸収する。
/// 取得に失敗した run は無かったものとして数える。
pub fn head_sha_job_names(gh: &Gh, owner_repo: &str, sha: &str) -> Names {
    let head = format!("head_sha={sha}");
    let runs = gh
        .lines(
            &format!("repos/{owner_repo}/actions/runs"),
            &["-X", "GET", "-F", &head, "-F", "per_page=100"],
            ".workflow_runs[].id",
        )
        .unwrap_or_default();
    let mut names = Names::new();
    for id in runs.iter().filter(|s| !s.is_empty()) {
        let jobs = gh
            .lines(
                &format!("repos/{owner_repo}/actions/runs/{id}/jobs"),
                &[],
                ".jobs[].name",
            )
            .unwrap_or_default();
        names.extend(jobs);
    }
    names
}

/// `wanted` が実測の job 名に全て現れるまで(または timeout まで)待ち、最終的な
/// 実測の job 名を返す。
pub fn poll_job_names(gh: &Gh, owner_repo: &str, sha: &str, wanted: &Names, poll: Poll) -> Names {
    let mut elapsed = 0;
    loop {
        let jobs = head_sha_job_names(gh, owner_repo, sha);
        if wanted.is_subset(&jobs) || elapsed >= poll.timeout {
            return jobs;
        }
        std::thread::sleep(Duration::from_secs(poll.interval));
        elapsed += poll.interval;
    }
}

fn bullets(v: &mut Vec<String>, names: impl IntoIterator<Item = impl std::fmt::Display>) {
    v.extend(names.into_iter().map(|n| format!("  - {n}")));
}

/// 宣言・live・実測の突合。
pub fn judge(declared: Option<&Names>, live: &Names, jobs: &Names, plan_free: bool) -> Verdict {
    let mut v = Verdict::default();

    match declared {
        Some(d) => {
            let missing: Vec<&String> = d.difference(jobs).collect();
            if !missing.is_empty() {
                v.err.push(
                    "rulesets-context-check: 宣言(.github/rulesets/quality.json)の required context がこの head で報告されていません:"
                        .into(),
                );
                bullets(&mut v.err, missing);
                v.code = 1;
                return v;
            }
        }
        None => v.out.push(
            "::warning::rulesets-context-check: .github/rulesets/quality.json が見つかりません(未播き — github-audit の rulesets-declaration-missing が該当)。"
                .into(),
        ),
    }

    if plan_free {
        v.out
            .push("rulesets-context-check: OK (plan-free-private: live の照合なし)".into());
        return v;
    }

    let missing_live: Vec<&String> = live.difference(jobs).collect();
    if !missing_live.is_empty() {
        v.err.push(
            "rulesets-context-check: live な required_status_checks の context がこの head で報告されていません:"
                .into(),
        );
        bullets(&mut v.err, missing_live);
        v.err.push(
            "  宣言を merge した後、apply-rulesets.sh <owner>/<repo> --reconcile で live を宣言に合わせてください。"
                .into(),
        );
        v.code = 1;
        return v;
    }

    if declared.is_some_and(|d| d != live) {
        v.out.push(
            "::warning::rulesets-context-check: 宣言と live の required context が一致しません(apply pending — apply-rulesets.sh <owner>/<repo> --reconcile を実行してください)。"
                .into(),
        );
        return v;
    }

    v.out.push("rulesets-context-check: OK".into());
    v
}

/// 1 回の検査。`gh` の取得に失敗したら fail-closed(終了コード 1)。
pub fn check_run(gh: &Gh, poll: Poll, owner_repo: &str, sha: &str, branch: &str) -> Verdict {
    let Ok(declared) = fetch_declared(gh, owner_repo, sha) else {
        return Verdict::fail_closed(format!(
            "rulesets-context-check: {owner_repo}@{sha} の宣言取得に失敗しました(fail-closed)"
        ));
    };

    // `plan-free-private` 宣言(docs/agents/free-plan-declaration.md): この環境では
    // branch ruleset を作れず、取得は 403 になる。live を取りに行かず空として
    // 扱い、宣言(quality.json)の context が head で報告されているかだけを
    // 検査する。宣言を外せば(Team 切替)live の照合が元に戻る。
    let mut notice = None;
    let plan_free = plan_free_private(gh, owner_repo);
    let live = if plan_free {
        notice = Some(
            "::notice::rulesets-context-check: plan-free-private が宣言されているため、live の branch ruleset の照合は行いません(宣言の context の報告のみ検査)。"
                .to_string(),
        );
        Names::new()
    } else {
        match fetch_live(gh, owner_repo, branch) {
            Ok(l) => l,
            Err(FetchFailed) => {
                return Verdict::fail_closed(format!(
                    "rulesets-context-check: {owner_repo} の branch ruleset 取得に失敗しました(fail-closed)"
                ))
            }
        }
    };

    let wanted: Names = match &declared {
        Some(d) => d.union(&live).cloned().collect(),
        None => live.clone(),
    };
    let jobs = poll_job_names(gh, owner_repo, sha, &wanted, poll);
    let mut v = judge(declared.as_ref(), &live, &jobs, plan_free);
    if let Some(n) = notice {
        v.out.insert(0, n);
    }
    v
}

/// CLI の入口。`args` は `argv[1..]`。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--selftest") => {
            eprintln!(
                "rulesets-context-check: --selftest は `cargo test -p pr-contract-check` に移った"
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
        Some(_) if args.len() != 3 => {
            eprint!("{USAGE}");
            2
        }
        Some(_) => {
            let gh = Gh::from_env("GITHUB_AUDIT_GH_BIN");
            check_run(&gh, Poll::from_env(), &args[0], &args[1], &args[2]).emit()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: &[&str]) -> Names {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn all_reported_is_ok() {
        let a = n(&["test", "PR Title / PR title"]);
        let v = judge(Some(&a), &a, &a, false);
        assert_eq!(v.code, 0);
        assert_eq!(v.out, ["rulesets-context-check: OK"]);
    }

    #[test]
    fn declared_context_not_reported_fails_and_names_it() {
        let v = judge(Some(&n(&["Ghost"])), &n(&["test"]), &n(&["test"]), false);
        assert_eq!(v.code, 1);
        assert!(v.err.iter().any(|l| l == "  - Ghost"), "{v:?}");
    }

    #[test]
    fn live_context_not_reported_fails_with_the_remedy() {
        let v = judge(Some(&n(&["test"])), &n(&["Other"]), &n(&["test"]), false);
        assert_eq!(v.code, 1);
        assert!(v.err.last().unwrap().contains("--reconcile"));
    }

    #[test]
    fn declared_differs_from_live_warns_but_passes() {
        let jobs = n(&["test", "x"]);
        let v = judge(Some(&n(&["test", "x"])), &n(&["test"]), &jobs, false);
        assert_eq!(v.code, 0);
        assert!(v.out[0].starts_with("::warning::") && v.out[0].contains("apply pending"));
    }

    #[test]
    fn missing_declaration_warns_and_checks_live_only() {
        let a = n(&["test"]);
        let v = judge(None, &a, &a, false);
        assert_eq!(v.code, 0);
        assert!(v.out[0].contains("見つかりません"));
        assert_eq!(v.out[1], "rulesets-context-check: OK");
    }

    #[test]
    fn plan_free_private_skips_live() {
        let d = n(&["a"]);
        let v = judge(Some(&d), &n(&["never-reported"]), &d, true);
        assert_eq!(v.code, 0);
        assert!(v.out[0].contains("plan-free-private"));
        // 宣言の context が未報告なら、plan-free-private でも red のまま。
        let v = judge(Some(&n(&["a"])), &Names::new(), &Names::new(), true);
        assert_eq!(v.code, 1);
    }
}
