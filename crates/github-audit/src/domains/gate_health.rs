//! gate-health ドメイン(ADR-1005 D5)。
//!
//! ci.yml の job が、ADR-1005 D1 ②の除外条件のうち機械で測れる 2 つ — 時間と flaky —
//! に当たっていないかを、既定ブランチ宛ての PR の直近の run の履歴から読む。
//!
//! - `gate-job-slow:<job>` — 成功した run での所要時間の p75(nearest-rank)が
//!   [`SLOW_P75_SECONDS`] 秒を超える。標本が [`MIN_SAMPLES`] 件未満の job は判定しない。
//! - `gate-job-flaky:<job>` — 同じ head SHA で、同じ job が `failure` と `success` の両方を
//!   出した(再実行で通った)。
//!
//! 「落ちたまま main に入ると main の不変条件が壊れるか」(D1 ①)と「外部依存か」は job の
//! 意味の判断で、ここでは見ない。run ごとに job 一覧を読んで REST を多く払うので、明示指定で
//! だけ走る([`crate::model::Domain::OPT_IN`])。取得は
//! [`crate::gh::Gh::fetch_gate_job_runs`]、ここは純粋な判定だけ。

use crate::model::{Finding, RepoGql};
use std::collections::{BTreeMap, BTreeSet};

/// 時間の上限(秒)。成功した run の所要時間の p75 がこれを超えたら slow(ADR-1005 D2)。
pub const SLOW_P75_SECONDS: i64 = 600;

/// p75 を出すのに要る、成功した run の件数の下限。これ未満の job は判定しない。
pub const MIN_SAMPLES: usize = 3;

/// 読む run の件数(既定ブランチ宛ての PR の、完了した ci.yml の run の新しい順)。
pub const MAX_RUNS: usize = 20;

/// ci.yml の 1 つの run の 1 つの job(再実行を含む。`filter=all`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateJobRun {
    /// 展開後の job 名(matrix なら `Build vega` のように展開された名前)。
    pub job: String,
    pub head_sha: String,
    /// `success` / `failure` / `cancelled` / `skipped` など。
    pub conclusion: String,
    /// 所要時間(秒)。計算できない(開始・終了の時刻が無い)ものは `None`。
    pub seconds: Option<i64>,
}

/// nearest-rank の p75。`sorted` は昇順で、空でないこと。
pub fn p75(sorted: &[i64]) -> i64 {
    let rank = (sorted.len() * 3).div_ceil(4).max(1);
    sorted[rank - 1]
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ',' => ';',
            c if c.is_whitespace() => '-',
            c => c,
        })
        .collect()
}

/// `jobs` は既定ブランチ宛ての PR の直近の run の全 job。空は「run が無い・取れなかった」で、
/// 何も報告しない(空データを drift に読まない)。
pub fn judge_gate_health(gql: &RepoGql, jobs: &[GateJobRun]) -> Finding {
    // ci.yml が無いリポジトリは workflows ドメインの `ci-yml-missing` が拾う。
    if !gql.has_workflow_file("ci.yml") {
        return Finding::not_applicable();
    }
    let mut durations: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    let mut by_sha: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    for j in jobs {
        if j.conclusion == "success" {
            if let Some(s) = j.seconds {
                durations.entry(&j.job).or_default().push(s);
            }
        }
        if !j.head_sha.is_empty() {
            by_sha
                .entry((&j.head_sha, &j.job))
                .or_default()
                .insert(&j.conclusion);
        }
    }
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for (job, mut secs) in durations {
        if secs.len() >= MIN_SAMPLES {
            secs.sort_unstable();
            if p75(&secs) > SLOW_P75_SECONDS {
                missing.insert(sanitize(&format!("gate-job-slow:{job}")));
            }
        }
    }
    for ((_, job), conclusions) in by_sha {
        if conclusions.contains("failure") && conclusions.contains("success") {
            missing.insert(sanitize(&format!("gate-job-flaky:{job}")));
        }
    }
    Finding::ok_or_drifted(missing.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Verdict;
    use serde_json::json;

    fn gql_with_ci() -> RepoGql {
        RepoGql::from_value(&json!({
            "workflowsDir": {"entries": [{"name": "ci.yml"}]}
        }))
    }

    fn run(job: &str, sha: &str, conclusion: &str, seconds: Option<i64>) -> GateJobRun {
        GateJobRun {
            job: job.into(),
            head_sha: sha.into(),
            conclusion: conclusion.into(),
            seconds,
        }
    }

    #[test]
    fn p75_is_nearest_rank() {
        assert_eq!(p75(&[5]), 5);
        assert_eq!(p75(&[1, 2]), 2);
        assert_eq!(p75(&[1, 2, 3]), 3);
        assert_eq!(p75(&[1, 2, 3, 4]), 3);
        assert_eq!(p75(&[10, 20, 30, 40, 50, 60, 70, 80]), 60);
    }

    #[test]
    fn no_ci_yml_is_not_applicable() {
        let f = judge_gate_health(&RepoGql::default(), &[run("a", "s", "success", Some(1))]);
        assert_eq!(f.verdict, Verdict::NotApplicable);
    }

    #[test]
    fn no_data_is_ok() {
        assert_eq!(judge_gate_health(&gql_with_ci(), &[]).verdict, Verdict::Ok);
    }

    #[test]
    fn slow_job_is_judged_by_p75_of_successful_runs() {
        // p75 = 3 番目 = 700 > 600 → slow。最大値(外れ値)ではなく p75 で見る。
        let slow: Vec<_> = [100, 650, 700, 9000]
            .iter()
            .enumerate()
            .map(|(i, s)| run("Build vega", &format!("s{i}"), "success", Some(*s)))
            .collect();
        let f = judge_gate_health(&gql_with_ci(), &slow);
        assert_eq!(f.verdict, Verdict::Drifted);
        assert_eq!(f.missing, vec!["gate-job-slow:Build-vega".to_string()]);

        // 同じ最大値でも p75 が上限以内なら slow ではない。
        let fast: Vec<_> = [100, 200, 300, 9000]
            .iter()
            .enumerate()
            .map(|(i, s)| run("Build vega", &format!("s{i}"), "success", Some(*s)))
            .collect();
        assert_eq!(
            judge_gate_health(&gql_with_ci(), &fast).verdict,
            Verdict::Ok
        );
    }

    #[test]
    fn slow_needs_enough_samples_and_ignores_unsuccessful_runs() {
        let few = [
            run("a", "s1", "success", Some(5000)),
            run("a", "s2", "success", Some(5000)),
        ];
        assert_eq!(judge_gate_health(&gql_with_ci(), &few).verdict, Verdict::Ok);
        // 失敗・skipped・時刻の無い run は所要時間に数えない。
        let noisy = [
            run("a", "s1", "failure", Some(5000)),
            run("a", "s2", "skipped", Some(5000)),
            run("a", "s3", "success", None),
            run("a", "s4", "success", Some(5000)),
            run("a", "s5", "success", Some(5000)),
        ];
        assert_eq!(
            judge_gate_health(&gql_with_ci(), &noisy).verdict,
            Verdict::Ok
        );
    }

    #[test]
    fn flaky_is_failure_and_success_on_the_same_sha() {
        let f = judge_gate_health(
            &gql_with_ci(),
            &[
                run("Rust workspace", "abc", "failure", Some(100)),
                run("Rust workspace", "abc", "success", Some(100)),
                // 別の SHA の失敗は、コードの変更による失敗で flaky ではない。
                run("Nix flake check", "abc", "success", Some(10)),
                run("Nix flake check", "def", "failure", Some(10)),
            ],
        );
        assert_eq!(f.verdict, Verdict::Drifted);
        assert_eq!(f.missing, vec!["gate-job-flaky:Rust-workspace".to_string()]);
    }

    #[test]
    fn findings_are_sorted_and_deduped() {
        let f = judge_gate_health(
            &gql_with_ci(),
            &[
                run("b, job", "x", "failure", None),
                run("b, job", "x", "success", None),
                run("b, job", "y", "failure", None),
                run("b, job", "y", "success", None),
                run("a", "x", "failure", None),
                run("a", "x", "success", None),
            ],
        );
        assert_eq!(
            f.missing,
            vec![
                "gate-job-flaky:a".to_string(),
                "gate-job-flaky:b;-job".to_string()
            ]
        );
    }
}
