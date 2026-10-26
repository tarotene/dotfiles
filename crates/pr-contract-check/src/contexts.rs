//! rulesets-context-check — required check の context 文字列が、この PR の head の
//! `.github/workflows/` 配下の workflow の job 名として宣言されているかを検査する(ADR-503)。
//!
//! かつては、実行中の job 名が Actions API に報告されるまで最大 1200 秒待っていた。`CI passed`
//! は重い job が全部終わるまで報告されないので、Nix のキャッシュが壊れて `Build vega` が 35 分
//! かかると待ち窓を超えて誤検知し(#1014)、窓の中でも毎回 3〜4 分待っていた(#1065)。今は
//! 待たず、head の全 workflow から読んだ job 名と静的に突き合わせる(ADR-503 Amendment
//! 2026-10-27)。報告の有無は required check 自身が保証する。
//!
//! `scripts/pr-title-context-check`(ADR-0031 Amendment 2026-09-26)の一般化版。
//! あちらは「PR title」1 context だけを自己照合したが、これは宣言
//! (`<repo>/.github/rulesets/quality.json`)と live な branch ruleset の
//! `required_status_checks` を丸ごと対象にする — required context の正本を対象
//! リポジトリ自身の宣言に一本化した以上(D1)、実測との整合を検査する対象も
//! PR title だけに留める理由が無い。
//!
//! 読むのは `repos/{owner}/{repo}/contents/.github/rulesets/quality.json`、
//! `repos/{owner}/{repo}/contents/.github/workflows/`(一覧と各ファイル。`ref` は head_sha)、
//! `repos/{owner}/{repo}/rules/branches/{branch}` だけ。

use crate::gh::{self, Gh};
use crate::job_names::{self, JobNames};
use crate::verdict::Verdict;
use std::collections::BTreeSet;

pub const USAGE: &str = "\
usage: rulesets-context-check <owner>/<repo> <head_sha> <default_branch>

<head_sha> の .github/workflows/ 配下の全 workflow から読んだ job の名前(`name:`、
無ければ job id。matrix・式・reusable workflow の呼び出しは取りうる名前に展開)と、
次の 2 つを静的に突合する(待たない):

  - 宣言: <owner>/<repo>@<head_sha> の .github/rulesets/quality.json の
    required_status_checks[].context
  - live: <default_branch> に有効な branch ruleset の
    required_status_checks[].context

判定:
  - live の context に、<head_sha> の workflow の job 名として読めないものがあれば FAIL
    (remedy: 宣言を merge 後 `apply-rulesets.sh <owner>/<repo> --reconcile`)。
  - 宣言の context に、読めないものがあれば FAIL。
  - 宣言 ≠ live(どちらも workflow に存在する)なら WARN(apply pending)。
  - 宣言が存在しない(まだ播いていない)場合は live 側だけを検査し、
    宣言欠落は warning に留める(github-audit の
    rulesets-declaration-missing が該当)。
  - 読み切れない workflow(フロー形式の jobs など)があり、context が見つからないときは、
    FAIL でなく WARN(判定できないことを fail にしない)。
  - API 取得に失敗した場合は fail-closed。

環境変数 RULESETS_CONTEXT_CHECK_TIMEOUT / RULESETS_CONTEXT_CHECK_INTERVAL は、以前の
待ち方の名残で、今は読まない(設定されていても無視する)。
存在するが PR で走らない job(trigger の条件)は静的には分からない。報告の実測は
apply-rulesets と github-audit が担う。
";

/// GitHub から取得できなかった(fail-closed にする)ことを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchFailed;

/// 文字列の集合(jq の `unique` と同じく、重複なしで、コードポイント順)。
pub type Names = BTreeSet<String>;

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

/// `sha` の `.github/workflows/` 配下の全 workflow を読み、job が報告しうる名前を集める。
/// ディレクトリが無い(404)ときは空。一覧・各ファイルの取得に失敗したら `Err`(fail-closed)。
pub fn fetch_job_names(gh: &Gh, owner_repo: &str, sha: &str) -> Result<JobNames, FetchFailed> {
    let ref_arg = format!("ref={sha}");
    let list = gh.api(
        &format!("repos/{owner_repo}/contents/.github/workflows"),
        &[
            "-X",
            "GET",
            "-F",
            &ref_arg,
            "--jq",
            ".[] | select(.type == \"file\") | .name",
        ],
    );
    if !list.ok {
        if list.stderr.contains("404") || list.stderr.contains("Not Found") {
            return Ok(JobNames::new());
        }
        return Err(FetchFailed);
    }
    let mut names = JobNames::new();
    for file in gh::split_lines(&list.stdout) {
        if !(file.ends_with(".yml") || file.ends_with(".yaml")) {
            continue;
        }
        let raw = gh.raw_file(owner_repo, &format!(".github/workflows/{file}"), sha);
        if !raw.ok {
            return Err(FetchFailed);
        }
        names.extend(job_names::parse(&file, &raw.stdout));
    }
    Ok(names)
}

fn bullets(v: &mut Vec<String>, names: impl IntoIterator<Item = impl std::fmt::Display>) {
    v.extend(names.into_iter().map(|n| format!("  - {n}")));
}

/// `contexts` のうち、`jobs` に job 名として読めないもの。
fn unreported<'a>(contexts: &'a Names, jobs: &JobNames) -> Vec<&'a String> {
    contexts.iter().filter(|c| !jobs.matches(c)).collect()
}

/// 読み切れない workflow があるときの、fail でなく warning にする注意書き。
fn unresolved_warning(jobs: &JobNames, what: &str, names: &[&String]) -> Vec<String> {
    let files: Vec<&str> = jobs.unresolved_files().map(String::as_str).collect();
    let mut out = vec![format!(
        "::warning::rulesets-context-check: {what}の required context が workflow の job 名として見つかりませんが、読み切れない workflow({})があるため判定しません:",
        files.join(", ")
    )];
    bullets(&mut out, names.iter().copied());
    out
}

/// 宣言・live・workflow の job 名の突合。
pub fn judge(declared: Option<&Names>, live: &Names, jobs: &JobNames, plan_free: bool) -> Verdict {
    let mut v = Verdict::default();

    match declared {
        Some(d) => {
            let missing = unreported(d, jobs);
            if !missing.is_empty() {
                if jobs.has_unresolved() {
                    v.out.extend(unresolved_warning(jobs, "宣言", &missing));
                } else {
                    v.err.push(
                        "rulesets-context-check: 宣言(.github/rulesets/quality.json)の required context がこの head で報告されていません:"
                            .into(),
                    );
                    bullets(&mut v.err, missing);
                    v.err.push(
                        "  head の .github/workflows/ 配下の job 名(`name:`、無ければ job id)に、この context と一致するものがありません。"
                            .into(),
                    );
                    v.code = 1;
                    return v;
                }
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

    let missing_live = unreported(live, jobs);
    if !missing_live.is_empty() {
        if jobs.has_unresolved() {
            v.out
                .extend(unresolved_warning(jobs, "live な ruleset", &missing_live));
        } else {
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
pub fn check_run(gh: &Gh, owner_repo: &str, sha: &str, branch: &str) -> Verdict {
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

    let Ok(jobs) = fetch_job_names(gh, owner_repo, sha) else {
        return Verdict::fail_closed(format!(
            "rulesets-context-check: {owner_repo}@{sha} の .github/workflows/ の読み取りに失敗しました(fail-closed)"
        ));
    };
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
            check_run(&gh, &args[0], &args[1], &args[2]).emit()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: &[&str]) -> Names {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn jobs(v: &[&str]) -> JobNames {
        JobNames::from_exact(v.iter().copied())
    }

    #[test]
    fn all_reported_is_ok() {
        let a = n(&["test", "PR Title / PR title"]);
        let j = jobs(&["test", "PR Title / PR title"]);
        let v = judge(Some(&a), &a, &j, false);
        assert_eq!(v.code, 0);
        assert_eq!(v.out, ["rulesets-context-check: OK"]);
    }

    #[test]
    fn declared_context_not_reported_fails_and_names_it() {
        let v = judge(Some(&n(&["Ghost"])), &n(&["test"]), &jobs(&["test"]), false);
        assert_eq!(v.code, 1);
        assert!(v.err.iter().any(|l| l == "  - Ghost"), "{v:?}");
    }

    #[test]
    fn live_context_not_reported_fails_with_the_remedy() {
        let v = judge(Some(&n(&["test"])), &n(&["Other"]), &jobs(&["test"]), false);
        assert_eq!(v.code, 1);
        assert!(v.err.last().unwrap().contains("--reconcile"));
    }

    #[test]
    fn declared_differs_from_live_warns_but_passes() {
        let j = jobs(&["test", "x"]);
        let v = judge(Some(&n(&["test", "x"])), &n(&["test"]), &j, false);
        assert_eq!(v.code, 0);
        assert!(v.out[0].starts_with("::warning::") && v.out[0].contains("apply pending"));
    }

    #[test]
    fn missing_declaration_warns_and_checks_live_only() {
        let a = n(&["test"]);
        let v = judge(None, &a, &jobs(&["test"]), false);
        assert_eq!(v.code, 0);
        assert!(v.out[0].contains("見つかりません"));
        assert_eq!(v.out[1], "rulesets-context-check: OK");
    }

    #[test]
    fn plan_free_private_skips_live() {
        let d = n(&["a"]);
        let v = judge(Some(&d), &n(&["never-reported"]), &jobs(&["a"]), true);
        assert_eq!(v.code, 0);
        assert!(v.out[0].contains("plan-free-private"));
        // 宣言の context が未報告なら、plan-free-private でも red のまま。
        let v = judge(Some(&n(&["a"])), &Names::new(), &JobNames::new(), true);
        assert_eq!(v.code, 1);
    }

    /// 読み切れない workflow があるとき、見つからない context は fail でなく warning
    /// (判定できないことを、他リポジトリの CI の fail にしない)。
    #[test]
    fn unresolved_workflow_downgrades_a_miss_to_a_warning() {
        let mut j = job_names::parse("ok.yml", "jobs:\n  a:\n    name: A\n");
        j.extend(job_names::parse("odd.yml", "jobs: { b: { runs-on: x } }\n"));
        let v = judge(Some(&n(&["Ghost"])), &n(&["Ghost"]), &j, false);
        assert_eq!(v.code, 0, "{v:?}");
        assert!(v.out.iter().any(|l| l.contains("odd.yml")), "{v:?}");
        // 見つかるものは、不明な workflow があっても普通に OK。
        let v = judge(Some(&n(&["A"])), &n(&["A"]), &j, false);
        assert_eq!(v.out, ["rulesets-context-check: OK"]);
    }
}
