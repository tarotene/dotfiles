//! workflows ドメイン(CI workflow 命名基準、grill セッション由来 ADR-591)。
//!
//! 判定そのものは `crates/pr-contract-check` の [`shared::judge_workflows`] が単一正本
//! (他リポジトリの required check `workflow-naming-check` も同じ関数を使う、ADR-0035 D1
//! 「単一正本 > 複写+同期」)。ci.yml の行ベースの読み取り([`parse_ci_yaml`])もそちら。
//! ここに残るのは、GraphQL のモデルからの入力の組み立てと、quality.json を
//! 「jq が保つ挿入順のまま」読む部分([`quality_status_checks`])だけ。

use crate::jq::{self, j_get, parse_ordered};
use crate::model::{blob_text, Finding, RepoGql};
use hook_io::jqfmt::J;
use pr_contract_check::workflows::{self as shared, Inputs};
use serde_json::Value;
use std::path::Path;

pub use shared::{line_uses, parse_ci_yaml, CiJob, CiYaml};

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

/// bash: `judge_workflows`。`canonical_quality_file` は quality.json の単一
/// 正本(読めなければ quality-json-not-canonical の検査を飛ばす)。
pub fn judge_workflows(gql: &RepoGql, canonical_quality_file: &Path) -> Finding {
    let file_names: Vec<String> = gql
        .workflow_entries()
        .iter()
        .filter_map(|e| e.name.clone())
        .collect();
    let ci_text = blob_text(&gql.decl_ci);
    let pr_title_text = blob_text(&gql.decl_pr_title);
    let quality_text = blob_text(&gql.decl_quality);
    let canonical = std::fs::read_to_string(canonical_quality_file).ok();
    Finding::ok_or_drifted(shared::judge_workflows(
        &Inputs {
            file_names: &file_names,
            ci_text: &ci_text,
            pr_title_text: &pr_title_text,
            quality_text: &quality_text,
            canonical_quality_text: canonical.as_deref(),
        },
        &quality_status_checks,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
