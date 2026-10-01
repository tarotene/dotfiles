//! 全体の組み立て(bash 版の parse_domains / audit / write_ledger /
//! render_report / any_drift)。

use crate::config::Config;
use crate::domains::{
    charters, docs, lifecycle, naming, releaser, renovate, routines, rulesets, settings, titles,
    workflows,
};
use crate::gh::FetchError;
use crate::jq;
use crate::model::{Domain, Finding, RepoFindings, RepoGql, RepoRest, Verdict};
use crate::vocab;
use hook_io::jqfmt::J;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// bash: `parse_domains`。空なら全ドメイン。未知の名前はその名前を `Err` で返す。
pub fn parse_domains<S: AsRef<str>>(args: &[S]) -> Result<Vec<Domain>, String> {
    let mut out = Vec::new();
    for a in args {
        let a = a.as_ref();
        out.push(Domain::parse(a).ok_or_else(|| a.to_string())?);
    }
    Ok(if out.is_empty() {
        Domain::ALL.to_vec()
    } else {
        out
    })
}

/// app-snapshot.json(ADR-590 D3)から releaser App の install 先を読む。
/// 戻り値は (snapshot に releaser App があるか, install 先の `owner/repo`)。
/// ファイルが無い・読めないのは静かなフォールバック(github-app-snapshot 未実行
/// や bws 未設定)— stderr に出すと `--json 2>&1` で読む呼び出し側を壊す。
pub fn read_releaser_snapshot(path: &Path, app_name: &str) -> (bool, Vec<String>) {
    if !path.is_file() {
        return (false, Vec::new());
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return (false, Vec::new());
    };
    let Ok(snap) = serde_json::from_str::<Value>(&text) else {
        return (false, Vec::new());
    };
    let apps: Vec<&Value> = snap.get("apps").map(jq::iter_values).unwrap_or_default();
    let matching: Vec<&&Value> = apps
        .iter()
        .filter(|a| a.get("name").and_then(Value::as_str) == Some(app_name))
        .collect();
    if matching.is_empty() {
        return (false, Vec::new());
    }
    let installs = matching
        .iter()
        .flat_map(|a| {
            a.get("installations")
                .map(jq::iter_values)
                .unwrap_or_default()
        })
        .flat_map(|i| {
            i.get("repositories")
                .map(jq::iter_values)
                .unwrap_or_default()
        })
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect();
    (true, installs)
}

/// bash: `audit`。`domains` の順に判定し、`gh repo list` の順にリポジトリを
/// 並べる。fail loud な取得(リポジトリ一覧・GraphQL バッチ)が失敗したら
/// `Err`(理由は stderr に出してある)。
pub fn audit(cfg: &Config, domains: &[Domain]) -> Result<Vec<RepoFindings>, FetchError> {
    let gh = cfg.gh();
    let repos = gh.list_repos_meta(&cfg.repo_limit, cfg.viewer_permission.as_deref())?;
    let overrides = vocab::read_overrides(&cfg.overrides_file);

    let has = |d: Domain| domains.contains(&d);
    // ADR-0020 以降 rulesets も workflowsDir(CI の有無)と宣言テキスト
    // (ADR-503)のために GraphQL が要る。titles / releaser / routines /
    // workflows / docs / lifecycle も同じバッチを読む。
    let need_graphql = domains
        .iter()
        .any(|d| !matches!(d, Domain::Naming | Domain::Settings));

    let mut gql_batch: HashMap<String, RepoGql> = HashMap::new();
    if need_graphql {
        let names: Vec<String> = repos.iter().map(|m| m.name.clone()).collect();
        gql_batch = gh.fetch_graphql_batch(&names).inspect_err(|_| {
            eprintln!(
                "github-audit: aborting — charters/renovate/rulesets judgement requires GraphQL data that failed to fetch"
            );
        })?;
    }

    // 閉語彙は実行全体で 1 回だけ読む(リポジトリごとではない)。
    let vocab = naming::NamingVocab {
        species: vocab::read_closed_set(&cfg.descriptive_species_file, None),
        registry: vocab::read_closed_set(
            &cfg.codename_registry_file,
            Some(&cfg.codename_registry_local_file),
        ),
        site_domains: vocab::read_closed_set(
            &cfg.site_domains_file,
            Some(&cfg.site_domains_local_file),
        ),
        // ADR-0026: PRIVATE 版の .local.tsv は無い。
        lifecycle_species: vocab::read_closed_set(&cfg.lifecycle_species_file, None),
    };
    let routines_sources = vocab::read_closed_set(
        &cfg.routines_auditor_sources_file,
        Some(&cfg.routines_auditor_sources_local_file),
    );

    // ADR-590 D3/D4、ADR-436 Amendment 2026-09-30: 実行全体で 1 回だけ読む。
    let (snapshot_present, installs) = if has(Domain::Releaser) {
        read_releaser_snapshot(&cfg.app_snapshot_file, &cfg.releaser_app_name)
    } else {
        (false, Vec::new())
    };

    let mut results = Vec::new();
    for meta in &repos {
        let repo = meta.name.as_str();
        let description = jq::sh_trim(meta.description.as_deref().unwrap_or("")).to_string();
        let created_at = jq::sh_trim(meta.created_at.as_deref().unwrap_or("")).to_string();
        let pushed_at = jq::sh_trim(meta.pushed_at.as_deref().unwrap_or("")).to_string();
        let is_archived = meta.is_archived == Some(true);
        let topics = meta.topics();
        let visibility = meta.visibility_str();
        let lang = meta.language();
        let empty = RepoGql::default();
        let gql = gql_batch.get(repo).unwrap_or(&empty);

        let mut out: Vec<(Domain, Finding)> = Vec::new();
        for &d in domains {
            let entry = if vocab::is_exempt(repo, d, &overrides) {
                Finding::exempt()
            } else if is_archived && d != Domain::Naming {
                // #278: archived は読み取り専用で、governance drift は直される
                // ことが無い。naming だけは isArchived 自体が lifecycle 候補の
                // 入力(ADR-0026)なので判定する。lifecycle(#275)は例外に
                // しない — archived は既に triage 済みで、休眠候補として
                // 再び数えるのは雑音でしかない。
                Finding::not_applicable()
            } else {
                match d {
                    Domain::Rulesets => {
                        let has_workflows = gql.has_workflows();
                        let jobs = if has_workflows {
                            gh.fetch_latest_pr_head_job_names(repo)
                        } else {
                            Vec::new()
                        };
                        rulesets::judge_rulesets(&gh, repo, has_workflows, gql, &jobs)
                    }
                    Domain::Charters => charters::judge_charters(&description, topics.len(), gql),
                    Domain::Naming => naming::judge_naming(
                        repo,
                        &topics,
                        &created_at,
                        &vocab,
                        &description,
                        is_archived,
                    ),
                    Domain::Settings => {
                        settings::judge_settings(meta, &gh.fetch_repo_settings(repo))
                    }
                    Domain::Renovate => {
                        renovate::judge_renovate(gql, &lang, &cfg.renovate_policy_preset)
                    }
                    Domain::Titles => {
                        // pr-title.yml の caller が無い repo は REST を払わない。
                        let jobs = if gql.has_workflow_file("pr-title.yml") {
                            gh.fetch_latest_pr_title_job_names(repo)
                        } else {
                            Vec::new()
                        };
                        titles::judge_titles(&gh, repo, gql, &jobs)
                    }
                    Domain::Lifecycle => {
                        let has_workflows = gql.has_workflows();
                        let ci = if has_workflows {
                            gh.fetch_latest_run_conclusion(repo)
                        } else {
                            String::new()
                        };
                        lifecycle::judge_lifecycle(
                            &pushed_at,
                            has_workflows,
                            gql,
                            &ci,
                            cfg.lifecycle_now.as_deref(),
                        )
                    }
                    Domain::Releaser => {
                        let refs = releaser::releaser_workflow_refs(gql);
                        let has_workflow = releaser::has_releaser_workflow(gql, &refs);
                        let secrets = if has_workflow {
                            gh.fetch_repo_secrets(repo)
                        } else {
                            Vec::new()
                        };
                        let full = format!("{}/{repo}", cfg.owner);
                        let installed = snapshot_present && installs.contains(&full);
                        // #673: 実在する release 系 workflow の最新の完了 run。
                        // どれか 1 つでも failure なら failure。
                        let mut release_conclusion = String::new();
                        if has_workflow {
                            for e in gql.workflow_entries() {
                                let Some(name) = e.name.as_deref() else {
                                    continue;
                                };
                                if !releaser::RELEASE_WORKFLOW_FILES.contains(&name) {
                                    continue;
                                }
                                if gh.fetch_latest_release_run_conclusion(repo, name) == "failure" {
                                    release_conclusion = "failure".into();
                                }
                            }
                        }
                        releaser::judge_releaser(&releaser::ReleaserInput {
                            has_workflow,
                            secrets,
                            snapshot_present,
                            installed,
                            refs,
                            release_conclusion,
                        })
                    }
                    Domain::Routines => {
                        routines::judge_routines(&cfg.owner, repo, gql, &routines_sources)
                    }
                    Domain::Workflows => {
                        workflows::judge_workflows(gql, &cfg.workflows_canonical_quality_file)
                    }
                    Domain::Docs => {
                        // Pages の状態は REST にしか無く、PRIVATE だけが問題
                        // (ADR-640 D7)なので PRIVATE だけが余分な呼び出しを払う。
                        let rest = if visibility == "PRIVATE" {
                            gh.fetch_repo_settings(repo)
                        } else {
                            RepoRest::default()
                        };
                        docs::judge_docs(repo, &visibility, gql, &rest)
                    }
                }
            };
            // 同じドメインを 2 回指定されたら jq の `. + {($d): $e}` と同じく
            // 後勝ち・位置は最初のまま。
            if let Some(slot) = out.iter_mut().find(|(k, _)| *k == d) {
                slot.1 = entry;
            } else {
                out.push((d, entry));
            }
        }
        results.push(RepoFindings {
            repo: repo.to_string(),
            language: lang,
            visibility,
            domains: out,
        });
    }
    Ok(results)
}

/// bash: `write_ledger`。`<state_dir>/ledger.json` に `{generated_at, repos}`
/// を書く(jq の既定整形)。一時ファイル(mktemp と同じ 0600)から rename する。
pub fn write_ledger(state_dir: &Path, findings: &[RepoFindings]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::create_dir_all(state_dir)?;
    let generated_at = hook_io::proc::date("%Y-%m-%dT%H:%M:%S%:z");
    let doc = J::obj(vec![
        ("generated_at", J::str(generated_at)),
        (
            "repos",
            J::Arr(findings.iter().map(RepoFindings::to_j).collect()),
        ),
    ]);
    let tmp = state_dir.join(format!("ledger.json.{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(doc.pretty().as_bytes())?;
        f.write_all(b"\n")?;
    }
    std::fs::rename(&tmp, state_dir.join("ledger.json"))
}

/// bash: `render_report`。not-applicable を除く 1 判定 1 行(`LC_ALL=C sort`)
/// と、合計・verdict ごとの件数。
pub fn render_report(findings: &[RepoFindings]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for r in findings {
        for (d, f) in &r.domains {
            if f.verdict == Verdict::NotApplicable {
                continue;
            }
            let mut line = format!(
                "{}\tdomain={} verdict={} repo={}",
                f.verdict.as_str(),
                d.as_str(),
                f.verdict.as_str(),
                r.repo
            );
            if !f.missing.is_empty() {
                line.push_str(" missing=");
                line.push_str(&f.missing.join(","));
            }
            if let Some(rl) = f.review_layer() {
                line.push_str(" review_layer=");
                line.push_str(rl.as_str());
            }
            lines.push(line);
        }
    }
    lines.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut out = String::new();
    for l in &lines {
        // cut -f2-
        out.push_str(l.split_once('\t').map_or(l.as_str(), |(_, rest)| rest));
        out.push('\n');
    }
    let total: usize = findings.iter().map(|r| r.domains.len()).sum();
    out.push_str(&format!(
        "total: {total} finding(s) across {} repo(s)\n",
        findings.len()
    ));
    for v in Verdict::REPORT_ORDER {
        let count = findings
            .iter()
            .flat_map(|r| r.domains.iter())
            .filter(|(_, f)| f.verdict == v)
            .count();
        out.push_str(&format!("  {}={count}\n", v.as_str()));
    }
    out
}

/// drift があるか(bash 版 `any_drift` は「drift が無ければ exit 0」で、
/// 真偽が逆なので注意)。dormancy-candidate(lifecycle の候補一覧、#261)と
/// advisory(releaser の過剰付与等、ADR-436 Amendment)は数えない — 「間違って
/// いる」状態ではなく、実行を失敗させる理由にならない。
pub fn any_drift(findings: &[RepoFindings]) -> bool {
    findings
        .iter()
        .flat_map(|r| r.domains.iter())
        .any(|(_, f)| {
            !matches!(
                f.verdict,
                Verdict::Ok
                    | Verdict::Exempt
                    | Verdict::NotApplicable
                    | Verdict::DormancyCandidate
                    | Verdict::Advisory
            )
        })
}
