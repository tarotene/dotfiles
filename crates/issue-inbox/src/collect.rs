//! `/my-cleanup` の収集・差分の確認と、裁定ページの入力の生成(#1421)。
//!
//! 毎回その場で `jq` を組み直していた次の作業を、決まった入口にする。
//!
//! - **収集**(`--collect <dir>`): open な Issue(`blocked-by-upstream` を除く)と、wrap-up inbox の open な
//!   気づきを、`<dir>/population.json` と `<dir>/findings.json` に保存する。
//! - **差分**(`--collect-diff <dir>`): 同じ取得をやり直し、保存した結果との差分(番号・更新日時・件数・
//!   気づきの題と再発の回数)だけを出す。`ExitPlanMode` の前に、母集団が動いていないことを確かめる。
//! - **裁定ページの入力**(`--adjudication-items <dir>`): 保存した 2 つの JSON から、
//!   `inbox-adjudication` の `reference/verdict-page/` の `items.json` を作る。推奨・根拠は、判断なので、
//!   `--rec <file>`(項目の id から `{rec_option, rec, why, when}` への写し)で渡す。
//!
//! この module は純粋関数だけを持つ(取得と書き込みは `lib.rs` が行う)。

use serde_json::{json, Value};

/// 裁定ページの入力の版(`items.schema.json` の `format_version` の `const`)。試験が、読み戻して一致を見る。
pub const FORMAT_VERSION: u64 = 6;

/// 収集から外すラベル(外部の事情で止まっている Issue。`/my-cleanup` §1)。
pub const EXCLUDED_LABEL: &str = "blocked-by-upstream";

/// 母集団の Issue 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pop {
    pub number: u64,
    pub title: String,
    pub updated_at: String,
}

/// `gh issue list --json number,title,labels,updatedAt` の応答を、除外ラベルを除いて、番号順の母集団にする。
pub fn parse_population(json_text: &str) -> Result<Vec<Pop>, String> {
    let v: Value =
        serde_json::from_str(json_text).map_err(|e| format!("Issue の一覧を読めない: {e}"))?;
    let arr = v.as_array().ok_or("Issue の一覧が配列でない")?;
    let mut out: Vec<Pop> = arr
        .iter()
        .filter(|i| {
            !i["labels"]
                .as_array()
                .is_some_and(|ls| ls.iter().any(|l| l["name"] == EXCLUDED_LABEL))
        })
        .filter_map(|i| {
            Some(Pop {
                number: i["number"].as_u64()?,
                title: i["title"].as_str()?.to_string(),
                updated_at: i["updatedAt"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    out.sort_by_key(|p| p.number);
    Ok(out)
}

/// 保存した母集団を JSON にする。
pub fn population_json(pop: &[Pop]) -> Value {
    Value::Array(
        pop.iter()
            .map(|p| json!({"number": p.number, "title": p.title, "updatedAt": p.updated_at}))
            .collect(),
    )
}

/// 保存した母集団を読み戻す。
pub fn read_population(v: &Value) -> Result<Vec<Pop>, String> {
    let arr = v.as_array().ok_or("population.json が配列でない")?;
    arr.iter()
        .map(|i| {
            Ok(Pop {
                number: i["number"].as_u64().ok_or("number が無い")?,
                title: i["title"].as_str().ok_or("title が無い")?.to_string(),
                updated_at: i["updatedAt"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// 2 つの母集団の差分(出力の行)。増えた `+`、消えた `-`、更新日時が変わった `~`。同じなら空。
pub fn diff_population(old: &[Pop], new: &[Pop]) -> Vec<String> {
    let mut out = Vec::new();
    for n in new {
        match old.iter().find(|o| o.number == n.number) {
            None => out.push(format!("+ #{} {}", n.number, n.title)),
            Some(o) if o.updated_at != n.updated_at => out.push(format!(
                "~ #{} {} ({} -> {})",
                n.number, n.title, o.updated_at, n.updated_at
            )),
            Some(_) => {}
        }
    }
    for o in old {
        if !new.iter().any(|n| n.number == o.number) {
            out.push(format!("- #{} {}", o.number, o.title));
        }
    }
    out
}

/// 気づき 1 件(題・repo・再発の回数)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub repo: String,
    pub count: u64,
    pub note: String,
    pub rows: Vec<String>,
}

/// `--list-open --json` の配列を気づきにする。
pub fn read_findings(v: &Value) -> Result<Vec<Finding>, String> {
    let arr = v.as_array().ok_or("findings.json が配列でない")?;
    arr.iter()
        .map(|f| {
            Ok(Finding {
                title: f["title"].as_str().ok_or("title が無い")?.to_string(),
                repo: f["repo"].as_str().unwrap_or("").to_string(),
                count: f["count"].as_u64().unwrap_or(0),
                note: f["note"].as_str().unwrap_or("").to_string(),
                rows: f["rows"]
                    .as_array()
                    .map(|r| {
                        r.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// 気づきの差分(題で対応づける。再発の回数の変化も出す)。
pub fn diff_findings(old: &[Finding], new: &[Finding]) -> Vec<String> {
    let mut out = Vec::new();
    for n in new {
        match old.iter().find(|o| o.title == n.title && o.repo == n.repo) {
            None => out.push(format!("+ 気づき {} (再発 {} 回)", n.title, n.count)),
            Some(o) if o.count != n.count => out.push(format!(
                "~ 気づき {} (再発 {} -> {} 回)",
                n.title, o.count, n.count
            )),
            Some(_) => {}
        }
    }
    for o in old {
        if !new.iter().any(|n| n.title == o.title && n.repo == o.repo) {
            out.push(format!("- 気づき {}", o.title));
        }
    }
    out
}

/// 裁定ページの項目の鍵に使う、気づきの短い名前(題と repo から決まる)。
fn finding_slug(f: &Finding) -> String {
    vault_note::short_hash(&format!("{}\0{}", f.repo, f.title))
}

/// `items.json`(裁定ページの入力)。母集団の Issue と気づきを、種類 `task` の項目にする。
/// `recs` は、項目の id(Issue は `i<番号>`、気づきは `f<短い名前>`)から
/// `{rec_option, rec, why, when}` への写し(推奨と根拠は判断なので、呼び出し側が渡す)。
/// 渡さない項目は、推奨の印が付かず、`rec` は「未起草」になる。
pub fn items_json(
    run_label: &str,
    pop: &[Pop],
    findings: &[Finding],
    recs: Option<&Value>,
) -> Value {
    let rec_of = |id: &str| recs.and_then(|r| r.get(id));
    let mut items: Vec<Value> = Vec::new();
    for f in findings {
        let slug = finding_slug(f);
        let id = format!("f{slug}");
        let r = rec_of(&id);
        items.push(item(
            &id,
            &format!("none:finding-{slug}/inbox"),
            &f.title,
            &format!("気づき(再発 {} 回)", f.count),
            r,
            &f.rows.join("\n"),
        ));
    }
    for p in pop {
        let id = format!("i{}", p.number);
        let r = rec_of(&id);
        items.push(item(
            &id,
            &format!("none:issue-{}/gh", p.number),
            &format!("#{} {}", p.number, p.title),
            "Issue",
            r,
            &format!("更新: {}", p.updated_at),
        ));
    }
    json!({"format_version": FORMAT_VERSION, "run_label": run_label, "items": items})
}

fn item(
    id: &str,
    key: &str,
    title: &str,
    from: &str,
    r: Option<&Value>,
    default_why: &str,
) -> Value {
    let text = |k: &str| r.and_then(|r| r[k].as_str());
    let mut v = json!({
        "id": id,
        "key": key,
        "kind": "task",
        "when": text("when").unwrap_or("未定"),
        "title": title,
        "from": from,
        "rec": text("rec").unwrap_or("未起草"),
        "why": text("why").filter(|w| !w.is_empty()).unwrap_or(default_why),
    });
    if let Some(opt) = text("rec_option") {
        v["rec_option"] = json!(opt);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
      {"number": 12, "title": "b", "labels": [], "updatedAt": "2026-11-01T00:00:00Z"},
      {"number": 5, "title": "a", "labels": [{"name": "ball:me"}], "updatedAt": "2026-11-02T00:00:00Z"},
      {"number": 9, "title": "held", "labels": [{"name": "blocked-by-upstream"}], "updatedAt": "x"}
    ]"#;

    #[test]
    fn population_excludes_the_upstream_label_and_sorts_by_number() {
        let p = parse_population(LIST).unwrap();
        let n: Vec<u64> = p.iter().map(|x| x.number).collect();
        assert_eq!(n, [5, 12]);
    }

    #[test]
    fn population_diff_reports_added_removed_and_changed() {
        let old = parse_population(LIST).unwrap();
        let mut new = old.clone();
        new[0].updated_at = "2026-11-09T00:00:00Z".into();
        new.remove(1);
        new.push(Pop {
            number: 20,
            title: "c".into(),
            updated_at: "u".into(),
        });
        let d = diff_population(&old, &new);
        assert_eq!(d.len(), 3, "{d:?}");
        assert!(d.iter().any(|l| l.starts_with("~ #5 ")));
        assert!(d.iter().any(|l| l.starts_with("- #12 ")));
        assert!(d.iter().any(|l| l.starts_with("+ #20 ")));
        assert!(diff_population(&old, &old).is_empty());
    }

    #[test]
    fn population_survives_a_save_and_reload() {
        let p = parse_population(LIST).unwrap();
        assert_eq!(read_population(&population_json(&p)).unwrap(), p);
    }

    fn findings() -> Vec<Finding> {
        read_findings(&json!([
            {"title": "t1", "repo": "o/r", "count": 2, "note": "inbox/t1.md", "rows": ["- 2026-11-09 a", "- 2026-11-10 b"]},
            {"title": "t2", "repo": "o/r", "count": 1, "note": "inbox/t2.md", "rows": ["- 2026-11-09 c"]}
        ]))
        .unwrap()
    }

    #[test]
    fn findings_diff_reports_new_gone_and_recurrence() {
        let old = findings();
        let mut new = old.clone();
        new[0].count = 3;
        new.remove(1);
        new.push(Finding {
            title: "t3".into(),
            repo: "o/r".into(),
            count: 1,
            note: String::new(),
            rows: vec![],
        });
        let d = diff_findings(&old, &new);
        assert_eq!(d.len(), 3, "{d:?}");
        assert!(d.iter().any(|l| l.contains("t1") && l.contains("2 -> 3")));
        assert!(d.iter().any(|l| l.starts_with("- 気づき t2")));
        assert!(d.iter().any(|l| l.starts_with("+ 気づき t3")));
    }

    #[test]
    fn items_json_makes_one_valid_looking_item_per_finding_and_issue() {
        let pop = parse_population(LIST).unwrap();
        let f = findings();
        let v = items_json("my-cleanup 2026-11-10", &pop, &f, None);
        assert_eq!(v["format_version"], FORMAT_VERSION);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        let mut ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4, "id が重複している");
        for i in items {
            let key = i["key"].as_str().unwrap();
            // items.schema.json の鍵の形: <媒体>:<スレッド>/<受信>
            assert!(
                key.starts_with("none:") && key.matches('/').count() == 1,
                "{key}"
            );
            assert!(
                key[5..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_=-/".contains(&b)),
                "{key}"
            );
            assert_eq!(i["kind"], "task");
            assert_eq!(i["rec"], "未起草");
            assert!(i.get("rec_option").is_none());
        }
    }

    #[test]
    fn recs_fill_the_judgement_and_the_recommendation_mark() {
        let pop = parse_population(LIST).unwrap();
        let recs = json!({"i5": {"rec_option": "do-agent", "rec": "着手する", "why": "理由", "when": "今回"}});
        let v = items_json("x", &pop, &[], Some(&recs));
        let i5 = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "i5")
            .unwrap();
        assert_eq!(i5["rec_option"], "do-agent");
        assert_eq!(i5["rec"], "着手する");
        assert_eq!(i5["why"], "理由");
        assert_eq!(i5["when"], "今回");
    }

    /// 入力の版が、`items.schema.json` の `const` と一致する(ページ形式を上げたのに、ここを直し忘れない)。
    #[test]
    fn format_version_matches_the_items_schema() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../config/agents/skills/inbox-adjudication/reference/verdict-page/items.schema.json",
        );
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            schema["properties"]["format_version"]["const"],
            FORMAT_VERSION
        );
    }
}
