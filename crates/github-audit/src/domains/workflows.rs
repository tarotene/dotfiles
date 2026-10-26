//! workflows ドメイン(CI workflow 命名基準、grill セッション由来 ADR-591)。
//!
//! 判定そのものは `crates/pr-contract-check` の [`shared::judge_workflows`] が単一正本
//! (他リポジトリの required check `workflow-naming-check` も同じ関数を使う、ADR-0035 D1
//! 「単一正本 > 複写+同期」)。ci.yml の行ベースの読み取り([`parse_ci_yaml`])もそちら。
//! ここに残るのは、GraphQL のモデルからの入力の組み立てと、quality.json を
//! 「jq が保つ挿入順のまま」読む部分([`quality_status_checks`])だけ。
//!
//! ADR-1005 の、ci.yml の外の workflow への検出もここに置く(監査だけで、PR ごとの検査には
//! 入れない): `advisory-workflow-no-push:<file>`(D3)と `default-branch-run-failed:<file>`
//! (D4)。前者は workflow ファイルの `on:` の行ベースの読み取り([`parse_triggers`])、
//! 後者は [`crate::gh::Gh::fetch_default_branch_failed_workflows`] の結果を受け取る。

use crate::jq::{self, j_get, parse_ordered};
use crate::model::{blob_text, Finding, RepoGql};
use jq_json_compat::jqfmt::J;
use pr_contract_check::workflows::{self as shared, Inputs};
use serde_json::Value;
use std::path::Path;

pub use shared::{ci_gate_empty, line_uses, parse_ci_yaml, CiJob, CiYaml};

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

/// 既定ブランチについての入力(ADR-1005 D3・D4)。既定値(名前が空)は「情報なし」で、
/// 何も報告しない(既定ブランチの無い空のリポジトリ、CI の無いリポジトリ、取得失敗)。
#[derive(Debug, Clone, Default)]
pub struct DefaultBranch<'a> {
    /// 既定ブランチの名前。
    pub name: &'a str,
    /// 既定ブランチで、workflow ごとの最新の完了 run が失敗だった workflow のファイル名。
    pub failed_workflows: &'a [String],
}

/// bash: `judge_workflows`。`canonical_quality_file` は quality.json の単一
/// 正本(読めなければ quality-json-not-canonical の検査を飛ばす)。
pub fn judge_workflows(
    gql: &RepoGql,
    canonical_quality_file: &Path,
    default_branch: &DefaultBranch<'_>,
) -> Finding {
    let file_names: Vec<String> = gql
        .workflow_entries()
        .iter()
        .filter_map(|e| e.name.clone())
        .collect();
    let ci_text = blob_text(&gql.decl_ci);
    let pr_title_text = blob_text(&gql.decl_pr_title);
    let quality_text = blob_text(&gql.decl_quality);
    let canonical = std::fs::read_to_string(canonical_quality_file).ok();
    let mut missing = shared::judge_workflows(
        &Inputs {
            file_names: &file_names,
            ci_text: &ci_text,
            pr_title_text: &pr_title_text,
            quality_text: &quality_text,
            canonical_quality_text: canonical.as_deref(),
        },
        &quality_status_checks,
    );
    // 監査だけの finding。PR ごとの検査(workflow-naming-check)には入れない — gate job を
    // まだ持たないリポジトリの PR が全部止まるため(ADR の D7)。
    if ci_gate_empty(&ci_text) {
        missing.push("ci-gate-empty".into());
    }
    if !default_branch.name.is_empty() {
        for e in gql.workflow_entries() {
            let Some(name) = e.name.as_deref() else {
                continue;
            };
            if RESERVED_WORKFLOWS.contains(&name) || !name.ends_with(".yml") {
                continue;
            }
            let text = blob_text(&e.object);
            let t = parse_triggers(&text, default_branch.name);
            if t.pr_on_open_or_sync && !t.runs_on_default_branch && !uses_pr_context(&text) {
                missing.push(format!("advisory-workflow-no-push:{name}"));
            }
        }
        for f in default_branch.failed_workflows {
            // いま `.github/workflows/` にあるファイルの run だけ(消した workflow の
            // 古い赤は直しようが無い)。
            if gql.has_workflow_file(f) {
                missing.push(format!("default-branch-run-failed:{f}"));
            }
        }
    }
    Finding::ok_or_drifted(missing)
}

/// `ci.yml`(blocking、集約 job を持つ)と `pr-title.yml`(PR の title の検査で、PR のイベント
/// だけで走るのが設計)は、D3 の対象にしない。
const RESERVED_WORKFLOWS: [&str; 2] = ["ci.yml", "pr-title.yml"];

/// workflow の `on:` から、D3 の判定に要る事実だけを読んだもの。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Triggers {
    /// `pull_request` があり、`types` が省略されているか `opened` / `synchronize` を含む
    /// (`closed` だけで走る workflow は検査ではない)。
    pub pr_on_open_or_sync: bool,
    /// 既定ブランチで run を起こす: `schedule`、または既定ブランチに当たる `push`。
    /// `workflow_dispatch` は人が起動するもので、数えない。
    pub runs_on_default_branch: bool,
}

/// PR のコンテキスト(`github.event.pull_request` / `github.event.number` / `gh pr `)を使うか。
/// 使う workflow(プレビュー・plan のコメント・PR 本文のリンク検査など)は PR が無い push では
/// 意味のある実行ができないので、D3 の対象(main で走らせて失敗を拾う検査)にしない。
/// 実アカウントの 2026-10-25 の走査で、この除外が無いと 5 件すべてがこの形の誤報だった。
fn uses_pr_context(text: &str) -> bool {
    ["github.event.pull_request", "github.event.number", "gh pr "]
        .iter()
        .any(|k| {
            text.lines()
                .any(|l| !l.trim_start().starts_with('#') && l.contains(k))
        })
}

/// コメント(`#` 以降、ただし `#` の前が空白か行頭のとき)を落として、前後の空白を除く。
fn strip_comment(s: &str) -> &str {
    let mut prev_ws = true;
    for (i, c) in s.char_indices() {
        if c == '#' && prev_ws {
            return s[..i].trim();
        }
        prev_ws = c.is_whitespace();
    }
    s.trim()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `[a, "b"]` / `{ push, pull_request }` の中身を値の列にする。
fn inline_list(v: &str) -> Vec<String> {
    v.trim_matches(|c| matches!(c, '[' | ']' | '{' | '}'))
        .split(',')
        .map(|x| unquote(x).to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// 1 つの trigger の本体(`push:` の下の行)から、`key:` の値の列を読む。インラインの
/// `[a, b]`、`- item` の列、単一の値の 3 形。`key:` が無ければ `None`。
fn list_values(body: &[&str], key: &str) -> Option<Vec<String>> {
    let prefix = format!("{key}:");
    let (idx, line) = body
        .iter()
        .enumerate()
        .find(|(_, l)| strip_comment(l).starts_with(&prefix))?;
    let rest = strip_comment(strip_comment(line).strip_prefix(&prefix)?);
    if !rest.is_empty() {
        return Some(if rest.starts_with('[') || rest.starts_with('{') {
            inline_list(rest)
        } else {
            vec![unquote(rest).to_string()]
        });
    }
    let key_indent = indent_of(line);
    let mut out = Vec::new();
    for l in &body[idx + 1..] {
        if is_blank_or_comment(l) {
            continue;
        }
        if indent_of(l) < key_indent {
            break;
        }
        match strip_comment(l).strip_prefix('-') {
            Some(item) => out.push(unquote(strip_comment(item)).to_string()),
            None => break,
        }
    }
    Some(out)
}

/// GitHub の branch filter の glob(`*` は `/` を越えず、`**` は越える)。`?`・`+`・`[]` は読まない。
fn glob_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[u8], n: &[u8]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => (0..=n.len()).any(|i| go(&p[2..], &n[i..])),
            Some(b'*') => {
                let upto = n.iter().position(|&c| c == b'/').unwrap_or(n.len());
                (0..=upto).any(|i| go(&p[1..], &n[i..]))
            }
            Some(&c) => n.first() == Some(&c) && go(&p[1..], &n[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

/// workflow ファイルの `on:` を、D3 の判定に要る範囲だけ行ベースで読む(2 スペース刻み。
/// ADR-591 の `parse_ci_yaml` と同じく、汎用の YAML パーサを入れない)。
pub fn parse_triggers(text: &str, default_branch: &str) -> Triggers {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| {
        let t = l.trim_end();
        ["on:", "\"on\":", "'on':"].iter().any(|k| t.starts_with(k))
    }) else {
        return Triggers::default();
    };
    let head = lines[start];
    let rest = strip_comment(&head[head.find(':').map_or(0, |i| i + 1)..]);
    if !rest.is_empty() {
        // `on: push` / `on: [push, pull_request]`: filter を持てない形。
        let names = if rest.starts_with('[') || rest.starts_with('{') {
            inline_list(rest)
        } else {
            vec![unquote(rest).to_string()]
        };
        let has = |k: &str| names.iter().any(|n| n == k);
        return Triggers {
            pr_on_open_or_sync: has("pull_request"),
            runs_on_default_branch: has("push") || has("schedule"),
        };
    }
    let block: Vec<&str> = lines[start + 1..]
        .iter()
        .copied()
        .take_while(|l| is_blank_or_comment(l) || indent_of(l) > 0)
        .collect();
    let Some(key_indent) = block
        .iter()
        .find(|l| !is_blank_or_comment(l))
        .map(|l| indent_of(l))
    else {
        return Triggers::default();
    };
    let mut out = Triggers::default();
    let mut i = 0;
    while i < block.len() {
        let line = block[i];
        i += 1;
        if is_blank_or_comment(line) || indent_of(line) != key_indent {
            continue;
        }
        let key_line = strip_comment(line);
        let Some((name, _)) = key_line.split_once(':') else {
            continue;
        };
        let body_start = i;
        while i < block.len() && (is_blank_or_comment(block[i]) || indent_of(block[i]) > key_indent)
        {
            i += 1;
        }
        let body = &block[body_start..i];
        match unquote(name) {
            "pull_request" => {
                let types = list_values(body, "types");
                if types
                    .as_ref()
                    .is_none_or(|t| t.iter().any(|x| x == "opened" || x == "synchronize"))
                {
                    out.pr_on_open_or_sync = true;
                }
            }
            "schedule" => out.runs_on_default_branch = true,
            "push" => {
                let branches = list_values(body, "branches");
                let ignore = list_values(body, "branches-ignore");
                let tags = list_values(body, "tags");
                let hit = match (&branches, &ignore) {
                    (Some(bs), _) => bs
                        .iter()
                        .filter(|b| !b.starts_with('!'))
                        .any(|b| glob_match(b, default_branch)),
                    (None, Some(ig)) => !ig.iter().any(|b| glob_match(b, default_branch)),
                    // filter が無い `push:` は全ブランチ。`tags:` だけなら tag の push だけ。
                    (None, None) => tags.is_none(),
                };
                if hit {
                    out.runs_on_default_branch = true;
                }
            }
            _ => {}
        }
    }
    out
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

    fn t(text: &str) -> Triggers {
        parse_triggers(text, "main")
    }

    #[test]
    fn block_form_with_push_and_pull_request() {
        let wf = "name: X\n\non:\n  push:\n    branches: [ main, \"feat/**\" ] # c\n  pull_request:\n    types: [opened, synchronize, reopened, labeled]\n  workflow_dispatch:\n\njobs:\n  a: {}\n";
        assert_eq!(
            t(wf),
            Triggers {
                pr_on_open_or_sync: true,
                runs_on_default_branch: true
            }
        );
    }

    #[test]
    fn pull_request_only_does_not_run_on_the_default_branch() {
        let wf = "name: X\non:\n  pull_request:\n    types: [opened, synchronize, reopened]\njobs:\n  a: {}\n";
        assert_eq!(
            t(wf),
            Triggers {
                pr_on_open_or_sync: true,
                runs_on_default_branch: false
            }
        );
    }

    #[test]
    fn closed_only_pull_request_is_not_a_check() {
        let wf = "on:\n  pull_request:\n    types: [closed]\njobs: {}\n";
        assert!(!t(wf).pr_on_open_or_sync);
    }

    #[test]
    fn omitted_types_default_to_opened_and_synchronize() {
        assert!(t("on:\n  pull_request:\njobs: {}\n").pr_on_open_or_sync);
        assert!(t("on: pull_request\njobs: {}\n").pr_on_open_or_sync);
        assert!(t("on: [push, pull_request]\njobs: {}\n").runs_on_default_branch);
    }

    #[test]
    fn push_filters() {
        let push = |body: &str| {
            t(&format!(
                "on:\n  pull_request:\n  push:\n{body}jobs: {{}}\n"
            ))
        };
        // filter 無しは全ブランチ。
        assert!(push("").runs_on_default_branch);
        // 他のブランチだけ、または tag だけなら既定ブランチでは走らない。
        assert!(!push("    branches: [develop]\n").runs_on_default_branch);
        assert!(!push("    tags: ['v*']\n").runs_on_default_branch);
        // 列(`- item`)形と glob。
        assert!(push("    branches:\n      - main\n").runs_on_default_branch);
        assert!(push("    branches:\n      - '**'\n").runs_on_default_branch);
        assert!(!push("    branches:\n      - 'release/*'\n").runs_on_default_branch);
        // paths だけの filter は、その path を触った push で走る。
        assert!(push("    paths: ['a/**']\n").runs_on_default_branch);
        assert!(!push("    branches-ignore: [main]\n").runs_on_default_branch);
    }

    #[test]
    fn schedule_runs_on_the_default_branch_and_dispatch_does_not() {
        let wf = "on:\n  pull_request:\n  schedule:\n    - cron: '0 0 * * 1'\njobs: {}\n";
        assert!(t(wf).runs_on_default_branch);
        let wf = "on:\n  pull_request:\n  workflow_dispatch:\njobs: {}\n";
        assert!(!t(wf).runs_on_default_branch);
    }

    #[test]
    fn pr_context_is_read_outside_comments_only() {
        assert!(uses_pr_context("run: gh pr comment 1 --body x\n"));
        assert!(uses_pr_context(
            "  PR: ${{ github.event.pull_request.number }}\n"
        ));
        assert!(!uses_pr_context(
            "# gh pr comment is not used\nrun: echo hi\n"
        ));
        assert!(!uses_pr_context("run: cargo test\n"));
    }

    #[test]
    fn no_on_key_is_default() {
        assert_eq!(t("jobs: {}\n"), Triggers::default());
    }

    #[test]
    fn glob_semantics() {
        assert!(glob_match("main", "main"));
        assert!(glob_match("*", "main"));
        assert!(!glob_match("*", "feat/x"));
        assert!(glob_match("**", "feat/x"));
        assert!(glob_match("ma*", "main"));
        assert!(!glob_match("main", "mainline"));
    }
}
