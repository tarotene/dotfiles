//! rulesets ドメイン(#130、ADR-0020、ADR-0021、ADR-503、#349)。
//!
//! default branch の ruleset を、ruleset の名前・数ではなく「active な
//! rule type の和集合」で判定する(実アカウント監査 2026-09-10 で、名前ベース
//! だと Copilot review を独立 ruleset に分ける repo や catch-all 1 本の repo で
//! 偽陽性が出ると分かったため。docs/github-audit.md)。

use crate::gh::Gh;
use crate::jq;
use crate::model::{Declaration, Detail, Finding, RepoGql, ReviewLayer, Verdict};
use serde_json::{Map, Value};

/// core layer(常に必須)の rule type。
pub const BASELINE_RULE_TYPES: [&str; 6] = [
    "deletion",
    "non_fast_forward",
    "required_signatures",
    "required_linear_history",
    "required_status_checks",
    "pull_request",
];

/// review layer(ADR-0021、opt-in の addin)の rule type。
pub const REVIEW_LAYER_RULE_TYPE: &str = "copilot_code_review";

fn rules(rs: &Value) -> Vec<&Value> {
    match rs.get("rules") {
        Some(Value::Array(a)) => a.iter().collect(),
        _ => Vec::new(),
    }
}

fn rule_type(r: &Value) -> Option<&str> {
    r.get("type").and_then(Value::as_str)
}

/// `[.[].rules[]? | select(.type=="required_status_checks") |
/// .parameters.required_status_checks[]?.context] | unique`。
/// titles ドメインも同じ集計を使う。
pub fn required_contexts(arr: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for rs in arr {
        for r in rules(rs) {
            if rule_type(r) != Some("required_status_checks") {
                continue;
            }
            if let Some(Value::Array(checks)) = r.pointer("/parameters/required_status_checks") {
                out.extend(
                    checks
                        .iter()
                        .filter_map(|c| c.get("context").and_then(Value::as_str))
                        .map(str::to_string),
                );
            }
        }
    }
    jq::unique_strings(out)
}

/// `{name,target,enforcement,conditions,bypass_actors,rules:(.rules|sort_by(.type))}`。
/// jq がエラーになる入力(オブジェクトでない、rules が配列でない等)は `None`。
fn normalize(v: &Value) -> Option<Value> {
    let Value::Object(m) = v else {
        return None;
    };
    let Some(Value::Array(rs)) = m.get("rules") else {
        return None;
    };
    if rs
        .iter()
        .any(|r| !matches!(r, Value::Object(_) | Value::Null))
    {
        return None;
    }
    let mut sorted = rs.clone();
    let key = |r: &Value| r.get("type").cloned().unwrap_or(Value::Null);
    sorted.sort_by(|a, b| jq::cmp(&key(a), &key(b)));
    let mut out = Map::new();
    for k in [
        "name",
        "target",
        "enforcement",
        "conditions",
        "bypass_actors",
    ] {
        out.insert(k.to_string(), m.get(k).cloned().unwrap_or(Value::Null));
    }
    out.insert("rules".to_string(), Value::Array(sorted));
    Some(Value::Object(out))
}

/// bash: `judge_rulesets`。
///
/// - `has_workflows`: `.github/workflows` の有無。無ければ
///   `required_status_checks` の代わりに `ci-absent` を出す(ADR-0020 — 黙って
///   免除せず、triage に見せ続ける)。
/// - `repo_gql`: 宣言(`.github/rulesets/*.json`)のテキスト。省略時は
///   `RepoGql::default()`(宣言なし)。
/// - `job_names`: 最新 PR の head が報告した job 名。空なら
///   `required-context-unreportable` を判定しない(PR が無い = 検証不能)。
pub fn judge_rulesets(
    gh: &Gh,
    repo: &str,
    has_workflows: bool,
    repo_gql: &RepoGql,
    job_names: &[String],
) -> Finding {
    judge_rulesets_with(
        &gh.default_branch_rulesets(repo),
        has_workflows,
        repo_gql,
        job_names,
    )
}

/// `judge_rulesets` の純粋部分(ruleset の詳細を受け取る)。
pub fn judge_rulesets_with(
    arr: &[Value],
    has_workflows: bool,
    repo_gql: &RepoGql,
    job_names: &[String],
) -> Finding {
    if arr.is_empty() {
        // ADR-503 の宣言・unreportable 検査は比較対象の live ruleset が要る
        // ので governed(count>0)の経路だけ。完全に ungoverned な repo は
        // この ADR 以前と同じ "ungoverned" のまま。
        return Finding {
            verdict: Verdict::Ungoverned,
            missing: Vec::new(),
            detail: Detail::Rulesets {
                status_checks: Vec::new(),
                review_layer: ReviewLayer::Absent,
                declaration: Declaration::NotJudged,
            },
        };
    }
    let mut missing: Vec<String> = Vec::new();

    // ADR-503: 3 つの core 宣言(review.json は任意、ADR-0021)。
    let has_declaration = repo_gql.decl_security.is_some()
        && repo_gql.decl_quality.is_some()
        && repo_gql.decl_workflow.is_some();
    if !has_declaration {
        missing.push("rulesets-declaration-missing".into());
    }

    let types: Vec<&str> = arr
        .iter()
        .flat_map(|rs| rules(rs).into_iter().filter_map(rule_type))
        .collect();
    let has_type = |t: &str| types.contains(&t);

    for t in BASELINE_RULE_TYPES {
        if t == "required_status_checks" && !has_workflows {
            continue;
        }
        if !has_type(t) {
            missing.push(t.into());
        }
    }
    if !has_workflows {
        missing.push("ci-absent".into());
    }

    // pull_request rule は複数の active ruleset に現れうる(core と review の
    // 分割、ADR-0021)。GitHub は同種 rule をパラメータごとに最も厳しい値で
    // 集約する(GitHub Docs "About rulesets" の rule layering)ので、thread
    // resolution は OR、allowed_merge_methods は積集合。
    let pr_params: Vec<Value> = arr
        .iter()
        .flat_map(rules)
        .filter(|r| rule_type(r) == Some("pull_request"))
        .map(|r| r.get("parameters").cloned().unwrap_or(Value::Null))
        .collect();
    let mut thread_ok = false;
    if !pr_params.is_empty() {
        thread_ok = pr_params.iter().any(|p| {
            p.get("required_review_thread_resolution")
                .and_then(Value::as_bool)
                == Some(true)
        });
        let lists: Vec<&Value> = pr_params
            .iter()
            .filter_map(|p| p.get("allowed_merge_methods"))
            .filter(|v| !v.is_null())
            .collect();
        let squash_ok = match lists.split_first() {
            None => false,
            Some((first, rest)) => {
                let mut acc: Vec<Value> = match first {
                    Value::Array(a) => a.clone(),
                    _ => Vec::new(),
                };
                for l in rest {
                    let l: &[Value] = match l {
                        Value::Array(a) => a,
                        _ => &[],
                    };
                    acc.retain(|x| l.contains(x));
                }
                acc == [Value::String("squash".into())]
            }
        };
        if !squash_ok {
            missing.push("pull_request.allowed_merge_methods".into());
        }
    }

    // review layer(ADR-0021): opt-in、どちらか半分でも在れば判定する。
    let review_layer_present = has_type(REVIEW_LAYER_RULE_TYPE) || thread_ok;
    let mut review_layer = ReviewLayer::Absent;
    if review_layer_present {
        let mut review_missing = Vec::new();
        if !has_type(REVIEW_LAYER_RULE_TYPE) {
            review_missing.push("review_layer.copilot_code_review".to_string());
        }
        if !thread_ok {
            review_missing.push("review_layer.required_review_thread_resolution".to_string());
        }
        if review_missing.is_empty() {
            review_layer = ReviewLayer::Complete;
        } else {
            review_layer = ReviewLayer::PartialDrift;
            missing.extend(review_missing);
        }
    }

    // ruleset レイアウト検査(#349): 名前ではなく「同じ rule type が複数の
    // active ruleset に重複している」ことだけを見る(名前ベースの判定は
    // 2026-09-10 の実アカウント監査で偽陽性を出すと分かっている)。dotfiles
    // 自身の実例(legacy 名 "Ephemeral Initial" が Workflow と同じ
    // pull_request を重複保持)はこれで拾える。pull_request だけは review
    // layer が opt-in されている間は core + review の 2 本が設計どおりなので
    // 判定しない。重複の単位は ruleset の id(name の無い fixture・同名の
    // 実リポジトリでも正しく数える)、表示ラベルは name があれば name。
    for t in BASELINE_RULE_TYPES
        .iter()
        .copied()
        .chain([REVIEW_LAYER_RULE_TYPE])
    {
        if t == "pull_request" && review_layer_present {
            continue;
        }
        let m: Vec<&Value> = arr
            .iter()
            .filter(|rs| rules(rs).iter().any(|r| rule_type(r) == Some(t)))
            .collect();
        if m.len() > 1 {
            let names: Vec<String> = m
                .iter()
                .map(|rs| match rs.get("name") {
                    Some(n) if jq::truthy(Some(n)) => jq::raw(n),
                    _ => jq::raw(rs.get("id").unwrap_or(&Value::Null)),
                })
                .collect();
            missing.push(format!("duplicate-ruleset:{t}:{}", names.join(",")));
        }
    }

    let checks = required_contexts(arr);

    // ADR-503: 宣言と live の差分(Security/Quality/Workflow/Review の名前ごと)。
    // 両側を {name,target,enforcement,conditions,bypass_actors,rules(type 順)}
    // に正規化して比べる — live にはサーバ側の付加フィールド(id, source,
    // created_at, ...)がある。粗い正規化なので、実アカウント監査で偽陽性が
    // 出れば調整が要る(GitHub が宣言の省略したパラメータを既定値で埋める)。
    if has_declaration {
        for (decl_name, blob) in [
            ("Security", &repo_gql.decl_security),
            ("Quality", &repo_gql.decl_quality),
            ("Workflow", &repo_gql.decl_workflow),
            ("Review", &repo_gql.decl_review),
        ] {
            let text = crate::model::blob_text(blob);
            if text.is_empty() {
                continue; // review.json は任意(ADR-0021)— 無いことは drift でない
            }
            let drift = format!("rulesets-declaration-drift:{decl_name}");
            // 読めない宣言も drift として数える。
            let Some(norm_decl) = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| normalize(&v))
            else {
                missing.push(drift);
                continue;
            };
            // 宣言はあるが同名の live ruleset が無い。
            let Some(live) = arr
                .iter()
                .find(|rs| rs.get("name").and_then(Value::as_str) == Some(decl_name))
            else {
                missing.push(drift);
                continue;
            };
            // bash 版では live の正規化が jq エラーになると監査全体が落ちた
            // (set -e)。ここでは drift として数える。
            if normalize(live).as_ref() != Some(&norm_decl) {
                missing.push(drift);
            }
        }
    }

    // ADR-503: 最新 PR の head のどの job も報告しない required context
    // (#337 の「永久に Expected」)。job_names が空 = PR がまだ無い(検証
    // 不能であって報告不能ではない)ので飛ばす。
    if !job_names.is_empty() {
        for ctx in checks.iter().filter(|c| !job_names.contains(c)) {
            if !ctx.is_empty() {
                missing.push(format!("required-context-unreportable:{ctx}"));
            }
        }
    }

    let declaration = if has_declaration {
        Declaration::Present
    } else {
        Declaration::Missing
    };
    Finding {
        verdict: if missing.is_empty() {
            Verdict::Ok
        } else {
            Verdict::Drifted
        },
        missing,
        detail: Detail::Rulesets {
            status_checks: checks,
            review_layer,
            declaration,
        },
    }
}
