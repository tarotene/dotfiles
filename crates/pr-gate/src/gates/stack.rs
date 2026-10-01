//! G_stack: chain 検出と GitHub stack リンク判定(ADR-0027、docs/claude/pr-gate.md
//! 「G_stack」節)。
//!
//! セッション内の複数 PR は常に作成順の単一チェーンに積む(uncertainty-first
//! stacking)。現在の PR を起点に open PR の base チェーンを両方向(祖先・子孫)へ
//! たどり連結成分(chain)を求める — chain が自分だけ(size 1)なら stacked PR で
//! はないので沈黙する。chain が 2 以上あるのに GitHub 上の stack
//! (`gh api repos/<nwo>/stacks`)へリンクされていなければ block する。
//! `gh-stack` 拡張が無い、または `stacks` API が取得できない(機能撤収・
//! ネットワーク障害)場合は判定不能として advisory に降格する — base
//! チェーンの正しさ自体は作成時の stack-base-guard(docs/claude/
//! stack-base-guard.md)が別途・全環境で強制しているため、この降格でも
//! orphan PR(base 宣言の不整合)自体は発生しない。

use crate::jqv::{self, JqError};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// open PR 一覧の 1 行(number, headRefName, baseRefName)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub number: String,
    pub head: String,
    pub base: String,
}

/// `gh pr list --json number,headRefName,baseRefName` の応答を行に直す
/// (bash: `jq -r '.[] | [(.number|tostring), .headRefName, .baseRefName] | @tsv'`)。
/// パースできなければ `None`。
pub fn parse_rows(prs_json: &str) -> Option<Vec<Row>> {
    let v: Value = serde_json::from_str(prs_json).ok()?;
    let cell = |x: Value| -> String {
        match x {
            Value::Null => String::new(),
            other => jqv::raw(&other),
        }
    };
    let mut rows = Vec::new();
    for item in jqv::iter(&v).ok()? {
        rows.push(Row {
            number: cell(jqv::field(item, "number").ok()?),
            head: cell(jqv::field(item, "headRefName").ok()?),
            base: cell(jqv::field(item, "baseRefName").ok()?),
        });
    }
    Some(rows)
}

/// 現在の PR を起点に、base チェーンの連結成分を bottom→top の PR 番号列で返す。
/// head branch が一覧に見当たらない(想定外)場合は `None`。
///
/// bash 版は祖先方向のループに循環検出を持たず、base が循環した open PR 群
/// (A の base が B、B の base が A)に当たると無限ループした。ここでは既に
/// chain に入っている PR に戻った時点で打ち切る(終了しない hook は Stop の
/// timeout 600 秒まで会話を止めるだけで、得るものが無いため)。
pub fn chain(rows: &[Row], pr_num: &str, head_branch: &str, base: &str) -> Option<Vec<String>> {
    if rows.is_empty() || head_branch.is_empty() {
        return None;
    }
    let mut num_of_head: HashMap<&str, &str> = HashMap::new();
    let mut base_of_num: HashMap<&str, &str> = HashMap::new();
    let mut head_of_num: HashMap<&str, &str> = HashMap::new();
    for r in rows.iter().filter(|r| !r.number.is_empty()) {
        num_of_head.insert(&r.head, &r.number);
        base_of_num.insert(&r.number, &r.base);
        head_of_num.insert(&r.number, &r.head);
    }
    num_of_head.get(head_branch)?;

    // 祖先方向: base が別の open PR の head と一致する限りさかのぼる。
    let mut chain: VecDeque<String> = VecDeque::from([pr_num.to_string()]);
    let mut cur_base = base.to_string();
    while !cur_base.is_empty() {
        let Some(anc) = num_of_head.get(cur_base.as_str()) else {
            break;
        };
        if chain.iter().any(|c| c == anc) {
            break;
        }
        chain.push_front(anc.to_string());
        cur_base = base_of_num.get(anc).copied().unwrap_or("").to_string();
    }

    // 子孫方向: base がこの PR(またはその子孫)の head と一致する PR を幅優先で拾う。
    let mut queue: VecDeque<String> = VecDeque::from([pr_num.to_string()]);
    while let Some(cn) = queue.pop_front() {
        let ch = head_of_num.get(cn.as_str()).copied().unwrap_or("");
        for r in rows.iter().filter(|r| !r.number.is_empty()) {
            if r.base != ch {
                continue;
            }
            if !chain.iter().any(|c| *c == r.number) {
                chain.push_back(r.number.clone());
                queue.push_back(r.number.clone());
            }
        }
    }
    Some(chain.into_iter().collect())
}

/// stacks API の応答に、chain の PR をすべて含む open な stack があるか
/// (bash: `any(.[]?; (.open == true) and (($nums - [(.pull_requests[]?.number)])
/// | length == 0))`)。JSON として読めない・jq がエラーになる形は「リンク
/// されていない」(block 側)に倒す — bash 版で jq が失敗したときと同じ。
pub fn is_linked(stacks_json: &str, nums: &[String]) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(stacks_json) else {
        return false;
    };
    let want: Vec<Value> = nums
        .iter()
        .filter_map(|n| n.parse::<u64>().ok())
        .map(Value::from)
        .collect();
    let stacks = jqv::iter(&v).unwrap_or_default();
    for s in stacks {
        let Ok(open) = jqv::field(s, "open") else {
            return false;
        };
        if open != Value::Bool(true) {
            continue;
        }
        let Ok(prs) = jqv::field(s, "pull_requests") else {
            return false;
        };
        let mut have = Vec::new();
        for p in jqv::iter(&prs).unwrap_or_default() {
            match jqv::field(p, "number") {
                Ok(n) => have.push(n),
                Err(JqError) => return false,
            }
        }
        if jqv::subtract(&want, &have).is_empty() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(spec: &[(&str, &str, &str)]) -> Vec<Row> {
        spec.iter()
            .map(|(n, h, b)| Row {
                number: n.to_string(),
                head: h.to_string(),
                base: b.to_string(),
            })
            .collect()
    }

    #[test]
    fn chain_both_directions() {
        let r = rows(&[("38", "s1", "main"), ("39", "s2", "s1"), ("40", "s3", "s2")]);
        assert_eq!(chain(&r, "39", "s2", "s1").unwrap(), ["38", "39", "40"]);
        assert_eq!(chain(&r, "38", "s1", "main").unwrap(), ["38", "39", "40"]);
        assert_eq!(chain(&r, "1", "other", "main"), None);
    }

    #[test]
    fn chain_cycle_terminates() {
        let r = rows(&[("1", "a", "b"), ("2", "b", "a")]);
        assert_eq!(chain(&r, "1", "a", "b").unwrap(), ["2", "1"]);
    }

    #[test]
    fn linked_forms() {
        let nums = vec!["38".to_string(), "39".to_string()];
        assert!(is_linked(
            r#"[{"open":true,"pull_requests":[{"number":38},{"number":39}]}]"#,
            &nums
        ));
        assert!(!is_linked(
            r#"[{"open":false,"pull_requests":[{"number":38},{"number":39}]}]"#,
            &nums
        ));
        assert!(!is_linked("[]", &nums));
        assert!(!is_linked("not json", &nums));
    }

    #[test]
    fn parse_rows_tostring() {
        let r = parse_rows(r#"[{"number":38,"headRefName":"s1","baseRefName":"main"}]"#).unwrap();
        assert_eq!(r, rows(&[("38", "s1", "main")]));
        assert_eq!(parse_rows("x"), None);
    }
}
