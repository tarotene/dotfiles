//! `gh` 経由の取得(bash 版の list_repos_meta / fetch_* / GraphQL バッチ)。
//!
//! 引数は bash 版と同じ並び・同じ `--jq` フィルタで `gh` に渡す。テストの
//! gh スタブ(bash 版 selftest と同じ、`--jq` を適用せずフィルタ後の形の
//! fixture を返す)にも、本物の gh にも同じように振る舞うため。
//!
//! 失敗時の縮退も bash 版に合わせる:
//! - `gh ... 2>/dev/null || printf '<fallback>'` は「stdout + fallback」
//! - `list_repos_meta` と GraphQL バッチだけは fail loud(#614、空データを
//!   「全部無い」と読んで全リポジトリ規模の偽 drift を出さないため)。
//!   この 2 つは gh の stderr もそのまま通す。

use crate::domains::annotations::JobWarnings;
use crate::domains::gate_health::{GateJobRun, MAX_RUNS};
use crate::jq;
use crate::model::{RepoGql, RepoMeta, RepoRest};
use serde_json::Value;
use std::collections::HashMap;
use std::process::{Command, Stdio};

/// GraphQL バッチの 1 チャンクのリポジトリ数。~35 リポジトリ(~50KB)を
/// 1 クエリにすると GitHub の GraphQL が 502 を返す(2026-09-19 観測、
/// docs.github.com/en/graphql/overview/resource-limitations)。
pub const GRAPHQL_CHUNK_SIZE: usize = 10;

/// fail loud な取得の失敗。メッセージは発生時点で stderr に出してある。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    RepoList,
    GraphQl,
}

/// gh の実行ハンドル(bash 版の `$GH_BIN` と `$OWNER`)。
#[derive(Debug, Clone)]
pub struct Gh {
    pub bin: String,
    pub owner: String,
}

struct Run {
    ok: bool,
    stdout: String,
}

impl Gh {
    pub fn new(bin: &str, owner: &str) -> Gh {
        Gh {
            bin: bin.to_string(),
            owner: owner.to_string(),
        }
    }

    fn run(&self, args: &[&str], inherit_stderr: bool) -> Run {
        let out = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .stderr(if inherit_stderr {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .output();
        match out {
            Ok(o) => Run {
                ok: o.status.success(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            },
            Err(e) => {
                if inherit_stderr {
                    eprintln!("{}: {e}", self.bin);
                }
                Run {
                    ok: false,
                    stdout: String::new(),
                }
            }
        }
    }

    /// `"$GH_BIN" <args> 2>/dev/null || printf '<fallback>'`
    fn quiet_or(&self, args: &[&str], fallback: &str) -> String {
        let r = self.run(args, false);
        if r.ok {
            r.stdout
        } else {
            r.stdout + fallback
        }
    }

    fn repo_path(&self, repo: &str, rest: &str) -> String {
        format!("repos/{}/{repo}{rest}", self.owner)
    }

    /// bash: `list_repos_meta`。#278 以降 archived も含める(`isArchived`)。
    /// `viewer_permission` があれば `viewerPermission` が一致するものだけ(#533)。
    pub fn list_repos_meta(
        &self,
        limit: &str,
        viewer_permission: Option<&str>,
    ) -> Result<Vec<RepoMeta>, FetchError> {
        let r = self.run(
            &[
                "repo",
                "list",
                &self.owner,
                "--source",
                "--limit",
                limit,
                "--json",
                "name,primaryLanguage,visibility,description,repositoryTopics,defaultBranchRef,\
squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed,deleteBranchOnMerge,\
hasWikiEnabled,hasProjectsEnabled,createdAt,pushedAt,isArchived,viewerPermission",
            ],
            true,
        );
        if !r.ok {
            // #614: 一覧の取得失敗(GraphQL rate limit 等)を「0 repo、全部
            // clean」に見せない — fetch_graphql_batch と同じ abort の形。
            eprintln!("github-audit: aborting — gh repo list failed (not treating as 0 repos)");
            return Err(FetchError::RepoList);
        }
        let repos: Vec<RepoMeta> =
            serde_json::from_str(&r.stdout).map_err(|_| FetchError::RepoList)?;
        Ok(match viewer_permission {
            Some(p) => repos
                .into_iter()
                .filter(|m| m.viewer_permission.as_deref() == Some(p))
                .collect(),
            None => repos,
        })
    }

    /// bash: `fetch_latest_run_conclusion`(#275 lifecycle)。直近の完了 run の
    /// conclusion、実行なし・取得不能は空文字。
    pub fn fetch_latest_run_conclusion(&self, repo: &str) -> String {
        let path = self.repo_path(repo, "/actions/runs?per_page=1&status=completed");
        let out = self.quiet_or(
            &[
                "api",
                &path,
                "--jq",
                ".workflow_runs[0].conclusion // empty",
            ],
            "",
        );
        jq::sh_trim(&out).to_string()
    }

    /// bash: `fetch_latest_release_run_conclusion`(#673)。workflow を
    /// ファイル名で特定し、その最新の完了 run の conclusion。全 workflow の
    /// 最新 1 件しか見ない上の関数では、同時刻の CI の成功に release の
    /// 失敗が隠れる(#659)。
    pub fn fetch_latest_release_run_conclusion(&self, repo: &str, workflow: &str) -> String {
        let path = self.repo_path(repo, &format!("/actions/workflows/{workflow}/runs"));
        let out = self.quiet_or(
            &[
                "api",
                &path,
                "-X",
                "GET",
                "-F",
                "per_page=1",
                "-F",
                "status=completed",
                "--jq",
                ".workflow_runs[0].conclusion // empty",
            ],
            "",
        );
        jq::sh_trim(&out).to_string()
    }

    /// workflows ドメインの取得(ADR-1005 D4)。`branch`(既定ブランチ)の完了 run を
    /// workflow ごとに最新 1 件へ絞り、結論が `failure` のものの workflow ファイル名を返す。
    /// 1 リポジトリあたり REST 1 回(annotations のように job を読まない)。取得失敗は空に
    /// 縮退する。run は 1 ページ(100 件)まで。
    pub fn fetch_default_branch_failed_workflows(&self, repo: &str, branch: &str) -> Vec<String> {
        let branch_arg = format!("branch={branch}");
        let out = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, "/actions/runs"),
                "-X",
                "GET",
                "-F",
                &branch_arg,
                "-F",
                "status=completed",
                "-F",
                "per_page=100",
                "--jq",
                "[.workflow_runs | group_by(.workflow_id)[] | sort_by(.created_at) | last \
                 | select(.conclusion == \"failure\") | .path | split(\"/\") | last]",
            ],
            "[]",
        );
        parse_strings(&out)
    }

    /// gate-health ドメインの取得(ADR-1005 D5)。ci.yml の完了した run のうち、
    /// `branch`(既定ブランチ)宛ての PR のものを新しい順に [`MAX_RUNS`] 件読み、
    /// 再実行を含む全 job(`filter=all`)の名前・結論・所要時間を返す。1 + [`MAX_RUNS`] 回の
    /// REST を払う。どの段の取得失敗も空に縮退する(判定材料なしとして ok に倒すのは judge)。
    /// run は 1 ページ(50 件)まで。
    pub fn fetch_gate_job_runs(&self, repo: &str, branch: &str) -> Vec<GateJobRun> {
        let runs = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, "/actions/workflows/ci.yml/runs"),
                "-X",
                "GET",
                "-F",
                "event=pull_request",
                "-F",
                "status=completed",
                "-F",
                "per_page=50",
                "--jq",
                "[.workflow_runs[] | {id: .id, head_sha: .head_sha, \
                 base: (.pull_requests[0].base.ref // \"\")}]",
            ],
            "[]",
        );
        let mut out = Vec::new();
        let wanted = parse_array(&runs)
            .into_iter()
            .filter(|r| r.get("base").and_then(Value::as_str) == Some(branch))
            .take(MAX_RUNS);
        for run in wanted {
            let (Some(run_id), Some(head_sha)) = (
                run.get("id").and_then(Value::as_i64),
                run.get("head_sha").and_then(Value::as_str),
            ) else {
                continue;
            };
            let jobs = self.quiet_or(
                &[
                    "api",
                    &self.repo_path(repo, &format!("/actions/runs/{run_id}/jobs")),
                    "-X",
                    "GET",
                    "-F",
                    "per_page=100",
                    "-F",
                    "filter=all",
                    "--jq",
                    "[.jobs[] | {name: .name, conclusion: (.conclusion // \"\"), \
                     seconds: (try ((.completed_at | fromdate) - (.started_at | fromdate)) \
                     catch null)}]",
                ],
                "[]",
            );
            for job in parse_array(&jobs) {
                let Some(name) = job.get("name").and_then(Value::as_str) else {
                    continue;
                };
                out.push(GateJobRun {
                    job: name.to_string(),
                    head_sha: head_sha.to_string(),
                    conclusion: job
                        .get("conclusion")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    seconds: job.get("seconds").and_then(Value::as_i64),
                });
            }
        }
        out
    }

    /// bash: `fetch_repo_settings` の生 JSON。squash_merge_commit_title 等は
    /// REST にしか無い(GraphQL の Repository 型に無い、2026-09-22 確認)。
    /// 失敗は `{}`(drift 側に倒れる)、JSON として読めない出力は null。
    ///
    /// 非公開リポジトリの repo オブジェクトは `security_and_analysis` を含まない
    /// ので、Dependabot の状態が無いときは専用 endpoint
    /// (`GET repos/O/R/automated-security-fixes` → `{"enabled": bool}`)に
    /// 引き直し、答えを `status` へ畳み込む(bash 版 #678)。引き直しも失敗
    /// したら欠落のまま残し、drift 側に倒す。
    pub fn fetch_repo_settings_raw(&self, repo: &str) -> Value {
        let r = self.run(&["api", &self.repo_path(repo, "")], false);
        let ok = r.ok;
        let out = if ok { r.stdout } else { r.stdout + "{}" };
        let mut rest: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
        if ok && rest.is_object() && !has_dependabot_status(&rest) {
            let sec = self.quiet_or(
                &["api", &self.repo_path(repo, "/automated-security-fixes")],
                "",
            );
            if let Some(status) = dependabot_status_from_endpoint(&sec) {
                set_dependabot_status(&mut rest, status);
            }
        }
        rest
    }

    /// bash: `fetch_repo_settings`(判定に使うフィールドだけの型)。
    pub fn fetch_repo_settings(&self, repo: &str) -> RepoRest {
        RepoRest::from_value(&self.fetch_repo_settings_raw(repo))
    }

    /// bash: `fetch_repo_secrets`(releaser)。secret の名前一覧(値は REST も
    /// 返さない)。取得失敗は空 — 単一リポジトリの失敗はその判定にしか
    /// 響かないのでアボートしない。
    pub fn fetch_repo_secrets(&self, repo: &str) -> Vec<String> {
        let out = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, "/actions/secrets"),
                "--jq",
                "[.secrets[].name]",
            ],
            "[]",
        );
        parse_strings(&out)
    }

    /// bash: `fetch_run_job_names`。ある run が実際に報告した job 名
    /// (required check の ground truth、#337 の再発防止、ADR-0031 Amendment)。
    pub fn fetch_run_job_names(&self, repo: &str, run_id: &str) -> Vec<String> {
        let out = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, &format!("/actions/runs/{run_id}/jobs")),
                "--jq",
                "[.jobs[].name]",
            ],
            "[]",
        );
        parse_strings(&out)
    }

    /// annotations ドメインの取得。`branch`(既定ブランチ)の完了 run を workflow
    /// ごとに最新 1 件へ絞り、その全 job の `warning` annotation を読む。どの
    /// 段の取得失敗も空に縮退する(空データを「全部 clean」ではなく「判定材料
    /// なし」と読ませる側は judge)。run・job・annotation は 1 ページ(100 件)
    /// まで。それを超える repo は 100 件目以降を見ない。
    pub fn fetch_warning_annotations(&self, repo: &str, branch: &str) -> Vec<JobWarnings> {
        let branch_arg = format!("branch={branch}");
        let runs = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, "/actions/runs"),
                "-X",
                "GET",
                "-F",
                &branch_arg,
                "-F",
                "status=completed",
                "-F",
                "per_page=100",
                "--jq",
                "[.workflow_runs | group_by(.workflow_id)[] | .[0] | {id: .id, name: .name}]",
            ],
            "[]",
        );
        let mut out = Vec::new();
        for run in parse_array(&runs) {
            let (Some(run_id), Some(workflow)) = (
                run.get("id").and_then(Value::as_i64),
                run.get("name").and_then(Value::as_str),
            ) else {
                continue;
            };
            let jobs = self.quiet_or(
                &[
                    "api",
                    &self.repo_path(repo, &format!("/actions/runs/{run_id}/jobs")),
                    "-X",
                    "GET",
                    "-F",
                    "per_page=100",
                    "--jq",
                    "[.jobs[] | {id: .id, name: .name}]",
                ],
                "[]",
            );
            for job in parse_array(&jobs) {
                let (Some(job_id), Some(name)) = (
                    job.get("id").and_then(Value::as_i64),
                    job.get("name").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let ann = self.quiet_or(
                    &[
                        "api",
                        &self.repo_path(repo, &format!("/check-runs/{job_id}/annotations")),
                        "-X",
                        "GET",
                        "-F",
                        "per_page=100",
                        "--jq",
                        "[.[] | select(.annotation_level == \"warning\") | .message]",
                    ],
                    "[]",
                );
                out.push(JobWarnings {
                    workflow: workflow.to_string(),
                    job: name.to_string(),
                    messages: parse_strings(&ann),
                });
            }
        }
        out
    }

    /// bash: `fetch_latest_pr_title_job_names`。pr-title.yml の最新 run の
    /// job 名。run が一度も無ければ空(drift 扱いにしない)。
    pub fn fetch_latest_pr_title_job_names(&self, repo: &str) -> Vec<String> {
        let r = self.run(
            &[
                "api",
                &self.repo_path(repo, "/actions/workflows/pr-title.yml/runs"),
                "-X",
                "GET",
                "-F",
                "per_page=1",
                "--jq",
                ".workflow_runs[0].id // empty",
            ],
            false,
        );
        let run_id = if r.ok {
            jq::sh_trim(&r.stdout).to_string()
        } else {
            String::new()
        };
        if run_id.is_empty() {
            return Vec::new();
        }
        self.fetch_run_job_names(repo, &run_id)
    }

    /// bash: `fetch_head_sha_job_names`(ADR-503)。あるコミットに対して
    /// 起動した全 workflow の run が報告した job 名の和集合(重複なし、
    /// バイト順)。reusable workflow・matrix・複数トリガーの並走を吸収する。
    pub fn fetch_head_sha_job_names(&self, repo: &str, sha: &str) -> Vec<String> {
        let out = self.quiet_or(
            &[
                "api",
                &self.repo_path(repo, "/actions/runs"),
                "-X",
                "GET",
                "-F",
                &format!("head_sha={sha}"),
                "-F",
                "per_page=100",
                "--jq",
                "[.workflow_runs[].id]",
            ],
            "[]",
        );
        let run_ids: Vec<String> = serde_json::from_str::<Value>(jq::sh_trim(&out))
            .map(|v| jq::iter_values(&v).into_iter().map(jq::raw).collect())
            .unwrap_or_default();
        let mut names: Vec<String> = Vec::new();
        for id in run_ids.iter().filter(|s| !s.is_empty()) {
            names.extend(self.fetch_run_job_names(repo, id));
            names = jq::unique_strings(names);
        }
        names
    }

    /// bash: `select_latest_pr_head_sha`。最新 PR の head SHA(#843)。
    ///
    /// 「最新」は、**既定ブランチ宛て**の **open または merged** の PR のうち、
    /// 最後に更新されたもの。次の 2 つを選ばない。
    ///
    /// - close しただけの古い PR。コメントして close すると `updated` が最新に
    ///   なるが、head は旧 workflow しか走らせておらず、必須 check を報告していない。
    /// - base が既定ブランチでない積み重ねの PR。`pull_request` の
    ///   `branches: [main]` で動く `ci.yml` は、その PR では走らない。
    ///
    /// どちらも live の ruleset と workflow は正しいのに `required-context-unreportable`
    /// を誤報する。条件に合う PR が無い・PR が無い・取得できないときは `None`
    /// (ground truth が無いので、報告不能と判定しない)。
    ///
    /// `gh` に射影させて最近の 30 件だけを持ち帰り、選択は手元で行う(射影後の JSON
    /// を返すスタブで検査できる)。
    pub fn latest_pr_head_sha(&self, repo: &str) -> Option<String> {
        let r = self.run(
            &[
                "api",
                &self.repo_path(repo, "/pulls"),
                "-X",
                "GET",
                "-F",
                "state=all",
                "-F",
                "sort=updated",
                "-F",
                "direction=desc",
                "-F",
                "per_page=30",
                "--jq",
                "[.[] | {state: .state, merged: (.merged_at != null), base: .base.ref, default: .base.repo.default_branch, sha: .head.sha}]",
            ],
            false,
        );
        if !r.ok {
            return None;
        }
        let v: Value = serde_json::from_str(jq::sh_trim(&r.stdout)).ok()?;
        v.as_array()?
            .iter()
            .find(|p| {
                let open = p.get("state").and_then(Value::as_str) == Some("open");
                let merged = p.get("merged").and_then(Value::as_bool) == Some(true);
                let base = p.get("base").and_then(Value::as_str);
                let default = p.get("default").and_then(Value::as_str);
                (open || merged) && base.is_some() && base == default
            })
            .and_then(|p| p.get("sha").and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    /// bash: `fetch_latest_pr_head_job_names`(ADR-503)。最新 PR の head が
    /// 報告した job 名。対象の PR が無ければ空。
    pub fn fetch_latest_pr_head_job_names(&self, repo: &str) -> Vec<String> {
        match self.latest_pr_head_sha(repo) {
            Some(sha) => self.fetch_head_sha_job_names(repo, &sha),
            None => Vec::new(),
        }
    }

    /// bash: `fetch_rulesets_list`(生の stdout、失敗は `None`)。
    pub fn fetch_rulesets_list(&self, repo: &str) -> Option<String> {
        let r = self.run(&["api", &self.repo_path(repo, "/rulesets")], false);
        r.ok.then_some(r.stdout)
    }

    /// bash: `fetch_ruleset_detail`(生の stdout、失敗は `None`)。
    pub fn fetch_ruleset_detail(&self, repo: &str, id: &str) -> Option<String> {
        let r = self.run(
            &["api", &self.repo_path(repo, &format!("/rulesets/{id}"))],
            false,
        );
        r.ok.then_some(r.stdout)
    }

    /// bash: `default_branch_rulesets`。default branch に掛かる active な
    /// branch ruleset の詳細。`~ALL` も default branch を縛るので含める。
    pub fn default_branch_rulesets(&self, repo: &str) -> Vec<Value> {
        let ids = self
            .fetch_rulesets_list(repo)
            .map(|s| active_branch_ruleset_ids(&s))
            .unwrap_or_default();
        let mut arr = Vec::new();
        for id in ids.iter().filter(|s| !s.is_empty()) {
            let Some(detail) = self.fetch_ruleset_detail(repo, id) else {
                continue;
            };
            if detail.is_empty() {
                continue;
            }
            let Ok(d) = serde_json::from_str::<Value>(&detail) else {
                continue;
            };
            if applies_to_default_branch(&d) {
                arr.push(d);
            }
        }
        arr
    }

    /// bash: `fetch_graphql_batch`。チャンクごとに 1 クエリ。失敗は全体を
    /// アボートする — 空オブジェクトに縮退すると「全ファイルが無い」と
    /// 読まれ、全リポジトリ規模の偽 drift になるため。
    pub fn fetch_graphql_batch(
        &self,
        names: &[String],
    ) -> Result<HashMap<String, RepoGql>, FetchError> {
        let mut merged = HashMap::new();
        for chunk in names.chunks(GRAPHQL_CHUNK_SIZE) {
            let query = build_graphql_query(&self.owner, chunk);
            let r = self.run(&["api", "graphql", "-f", &format!("query={query}")], true);
            let parsed = if r.ok {
                serde_json::from_str::<Value>(&r.stdout).ok()
            } else {
                None
            };
            let Some(result) = parsed else {
                eprintln!(
                    "github-audit: GraphQL batch fetch failed for repos [{}] — aborting (not falling back to empty data)",
                    chunk.join(", ")
                );
                return Err(FetchError::GraphQl);
            };
            let data = result.get("data").cloned().unwrap_or(Value::Null);
            for (i, name) in chunk.iter().enumerate() {
                let v = data.get(format!("r{}", i + 1)).unwrap_or(&Value::Null);
                merged.insert(name.clone(), RepoGql::from_value(v));
            }
        }
        Ok(merged)
    }
}

/// bash: `jq -e '.security_and_analysis.dependabot_security_updates.status'`。
fn has_dependabot_status(rest: &Value) -> bool {
    rest.pointer("/security_and_analysis/dependabot_security_updates/status")
        .and_then(Value::as_str)
        .is_some()
}

/// `automated-security-fixes` endpoint の応答 `{"enabled": bool}` を、repo
/// オブジェクトと同じ語彙(`enabled` / `disabled`)に直す。boolean でなければ None。
fn dependabot_status_from_endpoint(out: &str) -> Option<&'static str> {
    let v: Value = serde_json::from_str(out).ok()?;
    match v.get("enabled")?.as_bool()? {
        true => Some("enabled"),
        false => Some("disabled"),
    }
}

/// `.security_and_analysis.dependabot_security_updates.status = $s`(途中の
/// オブジェクトが無ければ作り、兄弟のキーは残す)。
fn set_dependabot_status(rest: &mut Value, status: &str) {
    let Some(root) = rest.as_object_mut() else {
        return;
    };
    let mut cur = root;
    for key in ["security_and_analysis", "dependabot_security_updates"] {
        let entry = cur
            .entry(key)
            .or_insert_with(|| Value::Object(Default::default()));
        if !entry.is_object() {
            *entry = Value::Object(Default::default());
        }
        cur = entry.as_object_mut().expect("just made an object");
    }
    cur.insert("status".into(), Value::String(status.into()));
}

/// JSON 配列として読めなければ空。
fn parse_array(out: &str) -> Vec<Value> {
    match serde_json::from_str::<Value>(out) {
        Ok(Value::Array(a)) => a,
        _ => Vec::new(),
    }
}

fn parse_strings(out: &str) -> Vec<String> {
    serde_json::from_str::<Value>(jq::sh_trim(out))
        .map(|v| jq::string_array(&v))
        .unwrap_or_default()
}

/// `jq -r '.[]? | select(.target=="branch" and .enforcement=="active") | .id'`。
/// jq がどこかで失敗すれば bash 版は `|| ids=''` で全体を空にする。
fn active_branch_ruleset_ids(list: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(list) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for item in jq::iter_values(&v) {
        match item {
            Value::Object(_) => {
                if item.get("target").and_then(Value::as_str) == Some("branch")
                    && item.get("enforcement").and_then(Value::as_str) == Some("active")
                {
                    ids.push(jq::raw(item.get("id").unwrap_or(&Value::Null)));
                }
            }
            Value::Null => {}
            _ => return Vec::new(),
        }
    }
    ids
}

/// `include` が `~DEFAULT_BRANCH` か `~ALL` を含み、`exclude` が
/// `~DEFAULT_BRANCH` を含まない。
fn applies_to_default_branch(d: &Value) -> bool {
    let list = |k: &str| -> Vec<Value> {
        match d.pointer(&format!("/conditions/ref_name/{k}")) {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        }
    };
    let has = |l: &[Value], s: &str| l.iter().any(|x| x.as_str() == Some(s));
    let inc = list("include");
    let exc = list("exclude");
    (has(&inc, "~DEFAULT_BRANCH") || has(&inc, "~ALL")) && !has(&exc, "~DEFAULT_BRANCH")
}

/// bash: `build_graphql_query`。クエリ文字列は bash 版と 1 字違わず同じ。
pub fn build_graphql_query(owner: &str, names: &[String]) -> String {
    let mut query = String::from("query {");
    for (i, name) in names.iter().enumerate() {
        let i = i + 1;
        query.push_str(&format!(
            " r{i}: repository(owner: \"{owner}\", name: \"{name}\") {{
      readme: object(expression: \"HEAD:README.md\") {{ ... on Blob {{ text }} }}
      contributing: object(expression: \"HEAD:CONTRIBUTING.md\") {{ ... on Blob {{ text }} }}
      claudeMd: object(expression: \"HEAD:CLAUDE.md\") {{ ... on Blob {{ text }} }}
      agentsMd: object(expression: \"HEAD:AGENTS.md\") {{ ... on Blob {{ text }} }}
      rootTree: object(expression: \"HEAD:\") {{ ... on Tree {{ entries {{ name type }} }} }}
      claudeSkillsDir: object(expression: \"HEAD:.claude/skills\") {{ ... on Tree {{ entries {{ name type mode }} }} }}
      routinesDir: object(expression: \"HEAD:.claude/routines\") {{ ... on Tree {{ entries {{ name type }} }} }}
      cargoToml: object(expression: \"HEAD:Cargo.toml\") {{ ... on Blob {{ id }} }}
      packageJson: object(expression: \"HEAD:package.json\") {{ ... on Blob {{ id text }} }}
      pyprojectToml: object(expression: \"HEAD:pyproject.toml\") {{ ... on Blob {{ id }} }}
      goMod: object(expression: \"HEAD:go.mod\") {{ ... on Blob {{ id }} }}
      flakeNix: object(expression: \"HEAD:flake.nix\") {{ ... on Blob {{ id }} }}
      workflowsDir: object(expression: \"HEAD:.github/workflows\") {{ ... on Tree {{ entries {{ name object {{ ... on Blob {{ text }} }} }} }} }}
      declCi: object(expression: \"HEAD:.github/workflows/ci.yml\") {{ ... on Blob {{ text }} }}
      declPrTitle: object(expression: \"HEAD:.github/workflows/pr-title.yml\") {{ ... on Blob {{ text }} }}
      rulesetsDir: object(expression: \"HEAD:.github/rulesets\") {{ ... on Tree {{ entries {{ name }} }} }}
      declSecurity: object(expression: \"HEAD:.github/rulesets/security.json\") {{ ... on Blob {{ text }} }}
      declQuality: object(expression: \"HEAD:.github/rulesets/quality.json\") {{ ... on Blob {{ text }} }}
      declWorkflow: object(expression: \"HEAD:.github/rulesets/workflow.json\") {{ ... on Blob {{ text }} }}
      declReview: object(expression: \"HEAD:.github/rulesets/review.json\") {{ ... on Blob {{ text }} }}
      renovateJson: object(expression: \"HEAD:renovate.json\") {{ ... on Blob {{ text }} }}
      renovateJson5: object(expression: \"HEAD:renovate.json5\") {{ ... on Blob {{ text }} }}
      ghRenovateJson: object(expression: \"HEAD:.github/renovate.json\") {{ ... on Blob {{ text }} }}
      ghRenovateJson5: object(expression: \"HEAD:.github/renovate.json5\") {{ ... on Blob {{ text }} }}
      renovaterc: object(expression: \"HEAD:.renovaterc.json\") {{ ... on Blob {{ text }} }}
      renovateDashboard: issues(states: OPEN, first: 1, filterBy: {{createdBy: \"renovate[bot]\"}}) {{ totalCount }}
      latestRelease: releases(first: 1, orderBy: {{field: CREATED_AT, direction: DESC}}) {{ nodes {{ createdAt }} }}
      openIssueActivity: issues(states: OPEN, first: 1, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ totalCount nodes {{ updatedAt }} }}
    }}"
        ));
    }
    query.push_str(" }");
    query
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_branch_condition() {
        let d = json!({"conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}}});
        assert!(applies_to_default_branch(&d));
        let d = json!({"conditions": {"ref_name": {"include": ["~ALL"], "exclude": ["~DEFAULT_BRANCH"]}}});
        assert!(!applies_to_default_branch(&d));
        assert!(!applies_to_default_branch(&json!({})));
    }

    #[test]
    fn ruleset_ids_filter() {
        let ids = active_branch_ruleset_ids(
            r#"[{"id":1,"target":"branch","enforcement":"active"},{"id":2,"target":"tag","enforcement":"active"},{"id":3,"target":"branch","enforcement":"disabled"}]"#,
        );
        assert_eq!(ids, vec!["1"]);
        assert!(active_branch_ruleset_ids(r#"{"message":"Not Found"}"#).is_empty());
    }
}
