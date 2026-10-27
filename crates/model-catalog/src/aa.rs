//! Artificial Analysis(AA)の `/data/llms/models` の応答から、カタログの測定値(生成ブロック)を作る。
//!
//! 突き合わせは AA の行 ID(uuid)で行う。AA 公式が、slug や名前は変わりうるので ID を主キーに
//! するよう推奨している(<https://artificialanalysis.ai/api-reference>、取得 2026-10-27)。
//! 取り込むのは、カタログの `[benchmark.*]` に定義済みのベンチだけで、値が null のものは載せない。

use crate::{AaMeta, Catalog, Measurement};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const ENDPOINT: &str = "https://artificialanalysis.ai/api/v2/data/llms/models";
pub const ATTRIBUTION: &str = "Artificial Analysis (https://artificialanalysis.ai/)";
pub const BEGIN: &str = "# BEGIN GENERATED (model-catalog fetch-aa) — do not edit by hand";
pub const END: &str = "# END GENERATED";

#[derive(Debug, PartialEq)]
pub struct Missing {
    pub model: String,
    pub effort: String,
    pub aa_id: String,
}

#[derive(Debug, PartialEq)]
pub struct Unmapped {
    pub creator: String,
    pub name: String,
    pub slug: String,
    pub id: String,
    pub release_date: Option<String>,
}

#[derive(Debug)]
pub struct Report {
    pub meta: AaMeta,
    pub measurements: Vec<Measurement>,
    /// 対応表にあるのに、応答に無い行。
    pub missing: Vec<Missing>,
    /// 追跡中のモデルの行が 1 つも応答に無い(引退の候補)。
    pub retire_candidates: Vec<String>,
    /// 追跡中の提供元の行で、対応表に無く、追跡中の最も古い行より新しいもの(新モデルの候補)。
    pub unmapped: Vec<Unmapped>,
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

pub fn collect(catalog: &Catalog, json: &str, retrieved: &str) -> Result<Report, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("AA の応答が JSON でない: {e}"))?;
    let data = root
        .get("data")
        .and_then(Value::as_array)
        .ok_or("AA の応答に data の配列が無い")?;
    let by_id: BTreeMap<&str, &Value> = data
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();

    let mut measurements = Vec::new();
    let mut missing = Vec::new();
    let mut retire_candidates = Vec::new();
    let mut mapped_ids = BTreeSet::new();
    let mut oldest_by_creator: BTreeMap<String, String> = BTreeMap::new();

    for model in &catalog.models {
        if model.aa_ids.is_empty() {
            continue;
        }
        let mut found = 0;
        for effort in &model.efforts {
            let Some(aa_id) = model.aa_ids.get(effort) else {
                continue;
            };
            mapped_ids.insert(aa_id.as_str());
            let Some(row) = by_id.get(aa_id.as_str()) else {
                missing.push(Missing {
                    model: model.id.clone(),
                    effort: effort.clone(),
                    aa_id: aa_id.clone(),
                });
                continue;
            };
            found += 1;
            let evals = row.get("evaluations");
            let scores: BTreeMap<String, f64> = catalog
                .benchmarks
                .keys()
                .filter_map(|bench| Some((bench.clone(), number(evals?.get(bench)?)?)))
                .collect();
            let release = row
                .get("release_date")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let (Some(creator), Some(date)) = (
                row.get("model_creator")
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str),
                &release,
            ) {
                let slot = oldest_by_creator
                    .entry(creator.to_owned())
                    .or_insert_with(|| date.clone());
                if date < slot {
                    *slot = date.clone();
                }
            }
            measurements.push(Measurement {
                model: model.id.clone(),
                effort: effort.clone(),
                aa_name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("AA の行 {aa_id} に name が無い"))?
                    .to_owned(),
                aa_slug: row
                    .get("slug")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("AA の行 {aa_id} に slug が無い"))?
                    .to_owned(),
                release_date: release,
                median_output_tokens_per_second: row
                    .get("median_output_tokens_per_second")
                    .and_then(number),
                median_time_to_first_token_seconds: row
                    .get("median_time_to_first_token_seconds")
                    .and_then(number),
                scores,
            });
        }
        if found == 0 {
            retire_candidates.push(model.id.clone());
        }
    }

    let tracked_creators: BTreeSet<&str> = catalog
        .providers
        .iter()
        .filter(|p| {
            catalog
                .models
                .iter()
                .any(|m| m.provider == p.id && !m.aa_ids.is_empty())
        })
        .filter_map(|p| p.aa_creator.as_deref())
        .collect();
    let mut unmapped = Vec::new();
    for row in data {
        let creator = row
            .get("model_creator")
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str);
        let id = row.get("id").and_then(Value::as_str);
        let (Some(creator), Some(id)) = (creator, id) else {
            continue;
        };
        if !tracked_creators.contains(creator) || mapped_ids.contains(id) {
            continue;
        }
        let date = row.get("release_date").and_then(Value::as_str);
        // 追跡中の最も古い行より古い行は、既に世代が替わった旧モデルなので候補にしない。
        if let (Some(floor), Some(date)) = (oldest_by_creator.get(creator), date) {
            if date < floor.as_str() {
                continue;
            }
        }
        unmapped.push(Unmapped {
            creator: creator.to_owned(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            slug: row
                .get("slug")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            id: id.to_owned(),
            release_date: date.map(str::to_owned),
        });
    }
    unmapped.sort_by(|a, b| (&a.creator, &a.name).cmp(&(&b.creator, &b.name)));

    let opts = root.get("prompt_options");
    Ok(Report {
        meta: AaMeta {
            retrieved: retrieved.to_owned(),
            source: ENDPOINT.to_owned(),
            attribution: ATTRIBUTION.to_owned(),
            prompt_length_tokens: opts
                .and_then(|o| o.get("prompt_length"))
                .and_then(Value::as_u64),
            parallel_queries: opts
                .and_then(|o| o.get("parallel_queries"))
                .and_then(Value::as_u64),
        },
        measurements,
        missing,
        retire_candidates,
        unmapped,
    })
}

fn s(v: &str) -> String {
    toml::Value::String(v.to_owned()).to_string()
}

/// 浮動小数は Debug 形式で書く(整数値にも `.0` が付き、TOML の浮動小数として読める)。
fn f(v: f64) -> String {
    format!("{v:?}")
}

pub fn render_block(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(BEGIN);
    out.push('\n');
    out.push_str("[aa]\n");
    out.push_str(&format!("retrieved = {}\n", s(&report.meta.retrieved)));
    out.push_str(&format!("source = {}\n", s(&report.meta.source)));
    out.push_str(&format!("attribution = {}\n", s(&report.meta.attribution)));
    if let Some(n) = report.meta.prompt_length_tokens {
        out.push_str(&format!("prompt_length_tokens = {n}\n"));
    }
    if let Some(n) = report.meta.parallel_queries {
        out.push_str(&format!("parallel_queries = {n}\n"));
    }
    for m in &report.measurements {
        out.push_str("\n[[measurement]]\n");
        out.push_str(&format!("model = {}\n", s(&m.model)));
        out.push_str(&format!("effort = {}\n", s(&m.effort)));
        out.push_str(&format!("aa_name = {}\n", s(&m.aa_name)));
        out.push_str(&format!("aa_slug = {}\n", s(&m.aa_slug)));
        if let Some(d) = &m.release_date {
            out.push_str(&format!("release_date = {}\n", s(d)));
        }
        if let Some(v) = m.median_output_tokens_per_second {
            out.push_str(&format!("median_output_tokens_per_second = {}\n", f(v)));
        }
        if let Some(v) = m.median_time_to_first_token_seconds {
            out.push_str(&format!("median_time_to_first_token_seconds = {}\n", f(v)));
        }
        out.push_str("\n[measurement.scores]\n");
        for (bench, v) in &m.scores {
            out.push_str(&format!("{bench} = {}\n", f(*v)));
        }
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// ファイルの生成ブロック(BEGIN〜END の行)を `block` に置き換える。
pub fn splice(file: &str, block: &str) -> Result<String, String> {
    let begin = file
        .find(BEGIN)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの開始行が無い: {BEGIN}"))?;
    let end_at = file[begin..]
        .find(END)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの終了行が無い: {END}"))?
        + begin;
    let mut tail = &file[end_at + END.len()..];
    tail = tail.strip_prefix('\n').unwrap_or(tail);
    Ok(format!("{}{}{}", &file[..begin], block, tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "status": 200,
      "prompt_options": {"parallel_queries": 1, "prompt_length": 1000},
      "data": [
        {"id":"05776db7-f5c0-40f7-b824-079160f8cfa8","name":"Claude Fable 5.1 (Low)","slug":"claude-fable-5-1-low",
         "release_date":"2026-09-01","model_creator":{"name":"Anthropic"},
         "median_output_tokens_per_second": 80.0,"median_time_to_first_token_seconds": 2,
         "evaluations":{"terminalbench_v4_0":0.5,"scicode":null,"lcr":0.25,"unknown_bench":0.9,"hle":0.1}},
        {"id":"ffffffff-0000-0000-0000-000000000001","name":"Claude Fable 6 (Low)","slug":"claude-fable-6-low",
         "release_date":"2026-10-20","model_creator":{"name":"Anthropic"},"evaluations":{}},
        {"id":"ffffffff-0000-0000-0000-000000000002","name":"Claude Old","slug":"claude-old",
         "release_date":"2025-01-01","model_creator":{"name":"Anthropic"},"evaluations":{}},
        {"id":"ffffffff-0000-0000-0000-000000000003","name":"Gemini X","slug":"gemini-x",
         "release_date":"2026-10-01","model_creator":{"name":"Google"},"evaluations":{}}
      ]}"#;

    fn catalog() -> Catalog {
        Catalog::parse(crate::CATALOG_TOML).unwrap()
    }

    #[test]
    fn collects_only_defined_benchmarks_and_skips_null() {
        let r = collect(&catalog(), FIXTURE, "2026-10-27").unwrap();
        assert_eq!(r.measurements.len(), 1);
        let m = &r.measurements[0];
        assert_eq!(
            (m.model.as_str(), m.effort.as_str()),
            ("claude-fable-5-1", "low")
        );
        let got: Vec<_> = m.scores.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(
            got,
            [("hle", 0.1), ("lcr", 0.25), ("terminalbench_v4_0", 0.5)]
        );
        assert_eq!(m.median_time_to_first_token_seconds, Some(2.0));
        assert_eq!(r.meta.prompt_length_tokens, Some(1000));
    }

    #[test]
    fn reports_missing_rows_and_retire_candidates() {
        let r = collect(&catalog(), FIXTURE, "2026-10-27").unwrap();
        // fable は low だけ応答にある。他の model は 1 行も無い。
        assert!(!r.retire_candidates.contains(&"claude-fable-5-1".to_owned()));
        for id in ["gpt-6.1-sol", "claude-opus-5-5", "claude-sonnet-5-5"] {
            assert!(r.retire_candidates.contains(&id.to_owned()), "{id}");
        }
        assert_eq!(
            r.missing
                .iter()
                .filter(|m| m.model == "claude-fable-5-1")
                .count(),
            4
        );
    }

    #[test]
    fn unmapped_only_newer_rows_of_tracked_creators() {
        let r = collect(&catalog(), FIXTURE, "2026-10-27").unwrap();
        let slugs: Vec<_> = r.unmapped.iter().map(|u| u.slug.as_str()).collect();
        // 旧世代(2025-01-01)と、追跡していない提供元(Google)は候補にしない。
        assert_eq!(slugs, ["claude-fable-6-low"]);
    }

    #[test]
    fn rendered_block_spliced_back_parses_to_the_same_measurements() {
        let c = catalog();
        let r = collect(&c, FIXTURE, "2026-10-27").unwrap();
        let block = render_block(&r);
        let text = splice(crate::CATALOG_TOML, &block).unwrap();
        let back = Catalog::parse(&text).unwrap();
        assert_eq!(back.measurements, r.measurements);
        assert_eq!(back.aa.unwrap(), r.meta);
        // 2 回目も同じ文字列になる(生成ブロックの置き換えは冪等)。
        assert_eq!(splice(&text, &block).unwrap(), text);
    }

    #[test]
    fn splice_requires_both_markers() {
        assert!(splice("nothing", "x").unwrap_err().contains("開始行"));
        assert!(splice(&format!("{BEGIN}\n"), "x")
            .unwrap_err()
            .contains("終了行"));
    }

    #[test]
    fn integral_floats_keep_a_decimal_point() {
        assert_eq!(f(1.0), "1.0");
        assert_eq!(f(0.565656565656566), "0.565656565656566");
    }

    #[test]
    fn rejects_non_catalog_json() {
        assert!(collect(&catalog(), "[]", "2026-10-27")
            .unwrap_err()
            .contains("data"));
        assert!(collect(&catalog(), "not json", "2026-10-27")
            .unwrap_err()
            .contains("JSON"));
    }
}
