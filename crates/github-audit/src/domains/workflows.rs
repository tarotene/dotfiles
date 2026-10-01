//! workflows ドメイン(CI workflow 命名基準、grill セッション由来 ADR-591)。
//!
//! ci.yml の job 構造は汎用 YAML パーサではなく行ベースの走査で読む(ADR-543
//! Q1: rulesets / github-audit 系はどれも yq に依存しておらず、汎用パーサは
//! この狭い検査に要る以上の仕事をする)。対象は自リポジトリのテンプレートで
//! 書式を統制できる ci.yml に限り、2 スペース刻み・`jobs:` が末尾という前提を
//! 置く(崩れれば workflows ドメイン自身のテストが壊れて検出する)。

use crate::jq::{self, j_get, parse_ordered};
use crate::model::{blob_text, Finding, RepoGql};
use hook_io::jqfmt::J;
use serde_json::Value;
use std::path::Path;

/// `parse_ci_yaml` の結果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CiYaml {
    /// 先頭の `name:`(無ければ `None`)。
    pub workflow_name: Option<String>,
    /// `jobs:` 直下の job(出現順、id と `name:`)。
    pub jobs: Vec<CiJob>,
    /// `ci-passed` job の `needs: [...]`(行が無ければ `None`)。
    pub ci_passed_needs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CiJob {
    pub id: String,
    pub name: Option<String>,
}

fn strip_ws_tab(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

/// gawk の `gsub(/^"|"$/, "", v)`(先頭と末尾の `"` を 1 つずつ)。
fn strip_quotes(v: &str) -> &str {
    let v = v.strip_prefix('"').unwrap_or(v);
    v.strip_suffix('"').unwrap_or(v)
}

fn is_job_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// bash: `parse_ci_yaml`(awk の行走査 + jq の組み立て)。
pub fn parse_ci_yaml(text: &str) -> CiYaml {
    // awk のレコード(タブ区切り)。jq 側で split("\t") するので、値に
    // タブがあればそこで切れる(bash 版どおり)。
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut in_jobs = false;
    let mut job = String::new();
    let mut wf_seen = false;
    for line in text.split('\n') {
        if !in_jobs && !wf_seen {
            if let Some(rest) = line.strip_prefix("name:") {
                let v = strip_quotes(strip_ws_tab(rest));
                wf_seen = true;
                records.push(
                    format!("WFNAME\t{v}")
                        .split('\t')
                        .map(String::from)
                        .collect(),
                );
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("jobs:") {
            if rest.chars().all(|c| c == ' ' || c == '\t') {
                in_jobs = true;
                continue;
            }
        }
        if in_jobs {
            if let Some(rest) = line.strip_prefix("  ") {
                if let Some((id, after)) = rest.split_once(':') {
                    if is_job_id(id) && after.chars().all(|c| c == ' ' || c == '\t') {
                        job = id.to_string();
                        records.push(vec!["JOB".into(), job.clone()]);
                        continue;
                    }
                }
            }
        }
        if in_jobs && !job.is_empty() {
            if let Some(rest) = line.strip_prefix("    name:") {
                let v = strip_quotes(strip_ws_tab(rest));
                records.push(
                    format!("NAME\t{job}\t{v}")
                        .split('\t')
                        .map(String::from)
                        .collect(),
                );
                continue;
            }
        }
        if in_jobs && job == "ci-passed" {
            if let Some(rest) = line.strip_prefix("    needs:") {
                if let Some(v) = strip_ws_tab(rest).strip_prefix('[') {
                    let v = v.split(']').next().unwrap_or("");
                    records.push(
                        format!("NEEDS\t{v}")
                            .split('\t')
                            .map(String::from)
                            .collect(),
                    );
                    continue;
                }
            }
        }
    }

    let field = |r: &Vec<String>, i: usize| r.get(i).cloned();
    let workflow_name = records
        .iter()
        .find(|r| r[0] == "WFNAME")
        .and_then(|r| field(r, 1));
    let job_ids: Vec<String> = records
        .iter()
        .filter(|r| r[0] == "JOB")
        .filter_map(|r| field(r, 1))
        .collect();
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    for r in records.iter().filter(|r| r[0] == "NAME") {
        let Some(k) = field(r, 1) else { continue };
        let v = field(r, 2);
        if let Some(slot) = names.iter_mut().find(|(ek, _)| *ek == k) {
            slot.1 = v;
        } else {
            names.push((k, v));
        }
    }
    let needs_raw = records
        .iter()
        .find(|r| r[0] == "NEEDS")
        .and_then(|r| field(r, 1));
    CiYaml {
        workflow_name,
        jobs: job_ids
            .into_iter()
            .map(|id| CiJob {
                name: names
                    .iter()
                    .find(|(k, _)| *k == id)
                    .and_then(|(_, v)| v.clone()),
                id,
            })
            .collect(),
        ci_passed_needs: needs_raw.map(|raw| {
            raw.split(',')
                .map(|s| s.trim_matches([' ', '\t']).to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }),
    }
}

fn j_to_value(j: &J) -> Value {
    serde_json::from_str(&j.compact()).unwrap_or(Value::Null)
}

/// `jq -c '[.rules[]? | select(.type == "required_status_checks") |
/// .parameters.required_status_checks[]?] | sort_by(.context)'`。キー順は
/// 入力のまま(比較は出力文字列で行う — bash 版どおり)。jq がエラーになる
/// 入力は `None`。
pub fn quality_status_checks(text: &str) -> Option<String> {
    let doc = parse_ordered(text)?;
    let iter = |v: Option<&J>| -> Vec<J> {
        match v {
            Some(J::Arr(a)) => a.clone(),
            Some(J::Obj(o)) => o.iter().map(|(_, v)| v.clone()).collect(),
            _ => Vec::new(),
        }
    };
    let rules = match &doc {
        J::Obj(_) => iter(j_get(&doc, "rules")),
        J::Null => Vec::new(),
        _ => return None,
    };
    let mut items: Vec<J> = Vec::new();
    for r in &rules {
        let ty = match r {
            J::Obj(_) => j_get(r, "type"),
            J::Null => None,
            _ => return None,
        };
        if !matches!(ty, Some(J::Str(s)) if s == "required_status_checks") {
            continue;
        }
        let params = j_get(r, "parameters");
        let checks = match params {
            Some(p @ J::Obj(_)) => j_get(p, "required_status_checks"),
            None | Some(J::Null) => None,
            _ => return None,
        };
        items.extend(iter(checks));
    }
    let mut keyed: Vec<(Value, J)> = Vec::new();
    for it in items {
        let key = match &it {
            J::Obj(_) => j_get(&it, "context").map(j_to_value).unwrap_or(Value::Null),
            J::Null => Value::Null,
            _ => return None,
        };
        keyed.push((key, it));
    }
    keyed.sort_by(|a, b| jq::cmp(&a.0, &b.0));
    Some(J::Arr(keyed.into_iter().map(|(_, j)| j).collect()).compact())
}

fn starts_lower(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

fn is_kebab(stem: &str) -> bool {
    !stem.is_empty()
        && stem.split('-').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// 行のどこかに `uses:[[:space:]]*<target>` があるか(grep -E の非アンカー一致)。
pub(crate) fn line_uses(line: &str, target: &str, tail: impl Fn(&str) -> bool) -> bool {
    let mut s = line;
    while let Some(i) = s.find("uses:") {
        let after = s[i + 5..].trim_start_matches(jq::is_space);
        if let Some(rest) = after.strip_prefix(target) {
            if tail(rest) {
                return true;
            }
        }
        s = &s[i + 5..];
    }
    false
}

/// bash: `judge_workflows`。`canonical_quality_file` は quality.json の単一
/// 正本(読めなければ quality-json-not-canonical の検査を飛ばす)。
pub fn judge_workflows(gql: &RepoGql, canonical_quality_file: &Path) -> Finding {
    let entries = gql.workflow_entries();
    let mut missing: Vec<String> = Vec::new();

    // 基準 7: titles/renovate/rulesets と違い not-applicable にしない —
    // workflow を 1 本も持たない repo も ci.yml を持つべき。
    let has_ci = gql.has_workflow_file("ci.yml");
    if !has_ci {
        missing.push("ci-yml-missing".into());
    }
    // 基準 5: 予約名 .yml / kebab-case のファイル名。
    // bash 版は `while read f; [[ -n $f ]]` で読むので、空の名前・空の
    // stem(".yml")は出さない。
    for name in entries.iter().filter_map(|e| e.name.as_deref()) {
        if !name.is_empty() && !name.ends_with(".yml") {
            missing.push(format!("file-not-yml:{name}"));
        }
    }
    for name in entries.iter().filter_map(|e| e.name.as_deref()) {
        if let Some(stem) = name.strip_suffix(".yml") {
            if !stem.is_empty() && !is_kebab(stem) {
                missing.push(format!("file-not-kebab:{stem}.yml"));
            }
        }
    }

    if has_ci {
        let ci_text = blob_text(&gql.decl_ci);
        if ci_text.is_empty() {
            missing.push("ci-yml-unreadable".into());
        } else {
            let parsed = parse_ci_yaml(&ci_text);
            match &parsed.workflow_name {
                None => missing.push("workflow-name-missing".into()),
                Some(n) if starts_lower(n) => missing.push("workflow-name-lowercase".into()),
                Some(_) => {}
            }
            let ci_passed_present = parsed.jobs.iter().any(|j| j.id == "ci-passed");
            if !ci_passed_present {
                missing.push("ci-passed-job-missing".into());
            }
            for j in &parsed.jobs {
                match j.name.as_deref() {
                    None | Some("") | Some("null") => {
                        missing.push(format!("job-name-missing:{}", j.id));
                    }
                    Some(n) if starts_lower(n) => {
                        missing.push(format!("job-name-lowercase:{}", j.id));
                    }
                    Some(_) => {}
                }
            }
            if ci_passed_present {
                let needs = parsed.ci_passed_needs.clone().unwrap_or_default();
                let others: Vec<&String> = parsed
                    .jobs
                    .iter()
                    .filter(|j| j.id != "ci-passed")
                    .map(|j| &j.id)
                    .collect();
                if !others.is_empty() && needs.is_empty() {
                    missing.push("ci-passed-needs-missing".into());
                } else {
                    for j in others.into_iter().filter(|j| !needs.contains(j)) {
                        if !j.is_empty() {
                            missing.push(format!("ci-passed-needs-incomplete:{j}"));
                        }
                    }
                }
            }
        }
    }

    // quality.json ≡ 単一正本(D12: repo-governance-common テンプレートを実行時に
    // 読む)。quality.json 自体が無い repo は rulesets の
    // rulesets-declaration-missing が既に拾うので二重に報告しない。
    if let Ok(canonical_text) = std::fs::read_to_string(canonical_quality_file) {
        let canonical = quality_status_checks(&canonical_text).unwrap_or_else(|| "[]".to_string());
        let quality_text = blob_text(&gql.decl_quality);
        if !quality_text.is_empty() {
            let repo_quality =
                quality_status_checks(&quality_text).unwrap_or_else(|| "null".to_string());
            if repo_quality != canonical {
                missing.push("quality-json-not-canonical".into());
            }
        }
    }

    // legacy の reusable pr-title.yml 呼び出し形(D4/Stage 6)。
    let pr_title_text = blob_text(&gql.decl_pr_title);
    if !pr_title_text.is_empty()
        && pr_title_text.split('\n').any(|l| {
            line_uses(
                l,
                "tarotene/dotfiles/.github/workflows/pr-title.yml@",
                |_| true,
            )
        })
    {
        missing.push("legacy-reusable-pr-title-call".into());
    }

    Finding::ok_or_drifted(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let p = parse_ci_yaml(
            "name: \"CI\"\njobs:\n  a:\n    name: A\n  ci-passed:\n    name: CI passed\n    needs: [a, b ]\n",
        );
        assert_eq!(p.workflow_name.as_deref(), Some("CI"));
        assert_eq!(p.jobs.len(), 2);
        assert_eq!(
            p.ci_passed_needs,
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn quality_key_order_matters() {
        let a = quality_status_checks(
            r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"b"},{"context":"a","integration_id":1}]}}]}"#,
        );
        assert_eq!(
            a.as_deref(),
            Some(r#"[{"context":"a","integration_id":1},{"context":"b"}]"#)
        );
        assert_eq!(quality_status_checks("[]"), None);
        assert_eq!(quality_status_checks("{}").as_deref(), Some("[]"));
    }
}
