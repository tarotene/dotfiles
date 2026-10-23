//! workflows の判定(CI workflow 命名基準、grill セッション由来 ADR-591)。
//!
//! `scripts/github-audit` の `judge_workflows` と、それを `source` していた
//! `scripts/workflow-naming-check` の単一正本。入力は素の値だけの純関数で、
//! `crates/github-audit` の workflows ドメインも同じ関数を使う(ADR-0035 D1
//! 「単一正本 > 複写+同期」)。
//!
//! ci.yml の job 構造は汎用 YAML パーサではなく行ベースの走査で読む(ADR-543
//! Q1: rulesets / github-audit 系はどれも yq に依存しておらず、汎用パーサは
//! この狭い検査に要る以上の仕事をしている)。対象は自リポジトリのテンプレートで
//! 書式を統制できる ci.yml に限り、2 スペース刻み・`jobs:` が末尾という前提を
//! 置く(崩れれば、ここのテストが壊れて検出する)。
//!
//! quality.json の「required_status_checks を context 順に並べた JSON 文字列」を作る
//! 部分だけは、呼び出し側が関数として渡す([`judge_workflows`] の第 2 引数)。
//! この crate は JSON を自前で読まない(依存ゼロ): CLI は `jq` を呼び、
//! `github-audit` は自前の順序保持パーサを使う。

/// glibc の `iswspace`(UTF-8 ロケールの `[[:space:]]`)。bash のパターン・`=~`・grep -E は
/// UTF-8 ロケールで U+3000 なども空白に数える。
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' '
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

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

/// 行のどこかに `uses:[[:space:]]*<target>` があるか(grep -E の非アンカー一致)。
pub fn line_uses(line: &str, target: &str, tail: impl Fn(&str) -> bool) -> bool {
    let mut s = line;
    while let Some(i) = s.find("uses:") {
        let after = s[i + 5..].trim_start_matches(is_space);
        if let Some(rest) = after.strip_prefix(target) {
            if tail(rest) {
                return true;
            }
        }
        s = &s[i + 5..];
    }
    false
}

/// [`judge_workflows`] の入力。取得できなかったファイルは空文字(`""`)。
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// `.github/workflows/` 直下の各エントリの名前(ファイルだけでよい)。
    pub file_names: &'a [String],
    pub ci_text: &'a str,
    pub pr_title_text: &'a str,
    /// 対象リポジトリの `.github/rulesets/quality.json` の中身。
    pub quality_text: &'a str,
    /// quality.json の単一正本(repo-governance-common のテンプレート)の中身。
    /// 読めなければ `None` — `quality-json-not-canonical` の検査を飛ばす。
    pub canonical_quality_text: Option<&'a str>,
}

/// drift の code(`missing`)。空なら適合。`quality_status_checks` は
/// quality.json の文字列から、required_status_checks の各要素を `context` 順に並べた
/// compact な JSON 文字列を作る(jq の `[.rules[]? | select(.type == "required_status_checks")
/// | .parameters.required_status_checks[]?] | sort_by(.context)` と同じ。jq がエラーに
/// なる入力は `None`)。比較は出力文字列で行うので、キーの並びも違いに数える。
pub fn judge_workflows(
    i: &Inputs<'_>,
    quality_status_checks: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();

    // 基準 7: titles/renovate/rulesets と違い not-applicable にしない —
    // workflow を 1 本も持たない repo も ci.yml を持つべき。
    let has_ci = i.file_names.iter().any(|n| n == "ci.yml");
    if !has_ci {
        missing.push("ci-yml-missing".into());
    }
    // 基準 5: 予約名 .yml / kebab-case のファイル名。
    // bash 版は `while read f; [[ -n $f ]]` で読むので、空の名前・空の
    // stem(".yml")は出さない。
    for name in i.file_names {
        if !name.is_empty() && !name.ends_with(".yml") {
            missing.push(format!("file-not-yml:{name}"));
        }
    }
    for name in i.file_names {
        if let Some(stem) = name.strip_suffix(".yml") {
            if !stem.is_empty() && !is_kebab(stem) {
                missing.push(format!("file-not-kebab:{stem}.yml"));
            }
        }
    }

    if has_ci {
        if i.ci_text.is_empty() {
            missing.push("ci-yml-unreadable".into());
        } else {
            let parsed = parse_ci_yaml(i.ci_text);
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
    if let Some(canonical_text) = i.canonical_quality_text {
        let canonical = quality_status_checks(canonical_text).unwrap_or_else(|| "[]".to_string());
        if !i.quality_text.is_empty() {
            let repo_quality =
                quality_status_checks(i.quality_text).unwrap_or_else(|| "null".to_string());
            if repo_quality != canonical {
                missing.push("quality-json-not-canonical".into());
            }
        }
    }

    // legacy の reusable pr-title.yml 呼び出し形(D4/Stage 6)。
    if !i.pr_title_text.is_empty()
        && i.pr_title_text.split('\n').any(|l| {
            line_uses(
                l,
                "tarotene/dotfiles/.github/workflows/pr-title.yml@",
                |_| true,
            )
        })
    {
        missing.push("legacy-reusable-pr-title-call".into());
    }

    missing
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

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const CI_OK: &str = "name: CI\njobs:\n  gitleaks:\n    runs-on: ubuntu-latest\n    name: Secret scan (gitleaks)\n    steps:\n      - run: echo hi\n  ci-passed:\n    runs-on: ubuntu-latest\n    name: CI passed\n    needs: [gitleaks]\n    if: always()\n    steps:\n      - run: echo done\n";

    /// quality.json の中身の違いだけを見る、テスト用の置き換え(JSON は読まない)。
    fn passthrough(s: &str) -> Option<String> {
        Some(s.trim().to_string())
    }

    fn judge(files: &[&str], ci: &str, pt: &str, q: &str, canon: Option<&str>) -> Vec<String> {
        let files = names(files);
        judge_workflows(
            &Inputs {
                file_names: &files,
                ci_text: ci,
                pr_title_text: pt,
                quality_text: q,
                canonical_quality_text: canon,
            },
            &passthrough,
        )
    }

    #[test]
    fn compliant_fixture_is_clean() {
        assert!(judge(&["ci.yml", "pr-title.yml"], CI_OK, "", "Q", Some("Q")).is_empty());
    }

    #[test]
    fn no_workflows_dir_is_ci_yml_missing() {
        assert_eq!(judge(&[], "", "", "", None), ["ci-yml-missing"]);
    }

    #[test]
    fn bad_names_are_reported_per_job() {
        let ci = "name: ci\njobs:\n  gitleaks:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n";
        let m = judge(&["ci.yml"], ci, "", "", None);
        for want in [
            "workflow-name-lowercase",
            "ci-passed-job-missing",
            "job-name-missing:gitleaks",
        ] {
            assert!(m.iter().any(|c| c == want), "{want} in {m:?}");
        }
    }

    #[test]
    fn file_name_rules() {
        let m = judge(
            &["ci.yml", "Build_All.yml", "notes.txt"],
            CI_OK,
            "",
            "",
            None,
        );
        assert!(m.contains(&"file-not-kebab:Build_All.yml".to_string()));
        assert!(m.contains(&"file-not-yml:notes.txt".to_string()));
    }

    #[test]
    fn legacy_reusable_call_is_detected() {
        let pt =
            "jobs:\n  check:\n    uses: tarotene/dotfiles/.github/workflows/pr-title.yml@main\n";
        let m = judge(&["ci.yml", "pr-title.yml"], CI_OK, pt, "Q", Some("Q"));
        assert_eq!(m, ["legacy-reusable-pr-title-call"]);
    }

    #[test]
    fn quality_differing_from_canonical_is_reported_only_when_both_exist() {
        let files = ["ci.yml"];
        assert_eq!(
            judge(&files, CI_OK, "", "A", Some("B")),
            ["quality-json-not-canonical"]
        );
        // 対象リポジトリに quality.json が無い、または単一正本を読めない: 検査しない。
        assert!(judge(&files, CI_OK, "", "", Some("B")).is_empty());
        assert!(judge(&files, CI_OK, "", "A", None).is_empty());
    }

    #[test]
    fn line_uses_matches_anywhere_in_the_line() {
        assert!(line_uses("  - uses:   a/b@v1 # x", "a/b@", |_| true));
        assert!(!line_uses("  - run: echo uses a/b@", "a/b@", |_| true));
    }
}
