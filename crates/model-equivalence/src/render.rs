//! 単一の HTML ページ(データとスクリプトを埋め込む)の生成。
//!
//! 許容幅 δ はページ上のスライダーで動かすので、判定の規則(境界の比較と役の束ね)は JS にも要る。
//! 重い計算(Newcombe の区間、速さの比)は Rust で事前に済ませて埋め込み、JS は比較と束ねだけを行う。
//! 二重化した規則がずれないよう、Rust が出した判定(2 通りの許容幅)を `golden` として埋め込み、
//! ページが読み込み時に自分の判定と突き合わせて、ずれたら警告を出す。

use crate::judge::{
    capability, compare, goodness_ratio, judged_benchmarks, Margins, SpeedMetric, Verdict,
};
use crate::lens::Lens;
use model_catalog::Catalog;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const PAGE: &str = include_str!("../page.html");

/// 突き合わせに使う 2 通り目の許容幅(既定とは別の領域の境界を通すため)。
pub const GOLDEN_ALT: Margins = Margins {
    capability_points: 15.0,
    speed_ratio: 0.5,
};

pub struct RenderOptions {
    /// 起動時に選んでおく基準(`MODEL:EFFORT`)。役ごと。
    pub plan_reference: String,
    pub execute_reference: String,
}

impl Default for RenderOptions {
    fn default() -> Self {
        // opusplan の現行設定: Plan 役 = Opus 5.5 high、実行役 = Sonnet 5.5 high。
        RenderOptions {
            plan_reference: "claude-opus-5-5:high".into(),
            execute_reference: "claude-sonnet-5-5:high".into(),
        }
    }
}

fn key(model: &str, effort: &str) -> String {
    format!("{model}:{effort}")
}

fn verdict_str(v: Verdict) -> Value {
    serde_json::to_value(v).expect("Verdict は文字列に直せる")
}

fn split_ref(s: &str) -> Result<(&str, &str), String> {
    s.split_once(':')
        .ok_or_else(|| format!("基準は MODEL:EFFORT の形: {s}"))
}

pub fn build_data(
    catalog: &Catalog,
    lens: Option<&Lens>,
    opts: &RenderOptions,
) -> Result<Value, String> {
    let margins = Margins::from_catalog(catalog);

    // 役と、判定に使うベンチ(履歴に従う役は絞り込み済み)。
    let mut roles = Vec::new();
    let mut cap_benches: BTreeSet<String> = BTreeSet::new();
    let mut shown_benches: BTreeSet<String> = BTreeSet::new();
    let mut ordered: Vec<_> = catalog.roles.iter().collect();
    ordered.sort_by_key(|(id, r)| (r.order, (*id).clone()));
    for (id, role) in ordered {
        let (judged, uncovered) = judged_benchmarks(catalog, id, lens)?;
        for (b, _) in &judged {
            cap_benches.insert(b.clone());
        }
        // 判定に使うベンチは、履歴で絞る前のもの全部の値を載せる(ページ側で絞り込みの説明に使う)。
        for b in role.benchmarks.iter().chain(&role.reference_benchmarks) {
            shown_benches.insert(b.clone());
            cap_benches.insert(b.clone());
        }
        let default_ref = match id.as_str() {
            "plan" => &opts.plan_reference,
            "execute" => &opts.execute_reference,
            _ => &opts.plan_reference,
        };
        let (m, e) = split_ref(default_ref)?;
        if catalog.measurement(m, e).is_none() {
            return Err(format!("役 {id} の基準 {default_ref} の測定値が無い"));
        }
        roles.push(json!({
            "id": id,
            "name": role.name,
            "note": role.note,
            "follows_history": role.follows_history,
            "judged": judged.iter().map(|(b, fit)| json!({"id": b, "fit": fit})).collect::<Vec<_>>(),
            "all_benchmarks": role.benchmarks,
            "reference_benchmarks": role.reference_benchmarks,
            "uncovered": uncovered.iter().map(|u| json!({"id": u.category, "name": u.name, "share": u.share})).collect::<Vec<_>>(),
            "default_reference": default_ref,
        }));
    }
    // 判定に使うベンチの値を、判定対象でないものも含めて pairs に載せる必要は無い。
    cap_benches.retain(|b| catalog.roles.values().any(|r| r.benchmarks.contains(b)));

    let mut benchmarks = Map::new();
    for id in &shown_benches {
        let b = &catalog.benchmarks[id];
        benchmarks.insert(
            id.clone(),
            json!({
                "name": b.name, "unit": match b.unit { model_catalog::Unit::Fraction => "fraction", model_catalog::Unit::IndexPoints => "index_points" }, "n": b.n, "n_unit": b.n_unit,
                "harness": b.harness, "trials": b.trials, "grader": b.grader, "note": b.note,
                "source": b.source.as_ref().map(|s| json!({
                    "author": s.author, "title": s.title, "url": s.url, "retrieved": s.retrieved
                })),
            }),
        );
    }

    let pricing = catalog.pricing();
    let mut models = Vec::new();
    for m in catalog.models.iter().filter(|m| !m.efforts.is_empty()) {
        let provider = catalog
            .providers
            .iter()
            .find(|p| p.id == m.provider)
            .map_or(m.provider.clone(), |p| p.name.clone());
        let price = pricing.iter().find(|p| p.id == m.id);
        models.push(json!({
            "id": m.id, "name": m.name, "provider": provider, "retired": m.status == model_catalog::Status::Retired,
            "efforts": m.efforts,
            "price": price.map(|p| json!({"input": p.api.input, "output": p.api.output, "cache_read": p.api.cache_read})),
        }));
    }

    let mut rows = Map::new();
    for r in &catalog.measurements {
        rows.insert(
            key(&r.model, &r.effort),
            json!({
                "aa_name": r.aa_name, "tps": r.median_output_tokens_per_second,
                "ttft": r.median_time_to_first_token_seconds, "scores": r.scores,
            }),
        );
    }

    // 順序つきの全組(基準, 候補)について、許容幅に依らない事前計算値を持つ。
    let mut pairs = Map::new();
    for rf in &catalog.measurements {
        let mut per_ref = Map::new();
        for cd in &catalog.measurements {
            if (&rf.model, &rf.effort) == (&cd.model, &cd.effort) {
                continue;
            }
            let mut cap = Map::new();
            for id in &cap_benches {
                let bench = &catalog.benchmarks[id];
                let j = capability(
                    id,
                    bench,
                    rf.scores.get(id).copied(),
                    cd.scores.get(id).copied(),
                    margins.capability_points,
                );
                cap.insert(
                    id.clone(),
                    match (j.interval, j.resolution_points, j.reference, j.candidate) {
                        (Some((lo, hi)), Some(res), Some(r), Some(c)) => {
                            json!({"d": (c - r) * 100.0, "lo": lo, "hi": hi, "res": res})
                        }
                        _ => json!({"why": j.basis}),
                    },
                );
            }
            let spd = |metric: SpeedMetric| -> Value {
                match (metric.value(rf), metric.value(cd)) {
                    (Some(r), Some(c)) if r > 0.0 && c > 0.0 => json!(goodness_ratio(metric, r, c)),
                    _ => Value::Null,
                }
            };
            per_ref.insert(
                key(&cd.model, &cd.effort),
                json!({
                    "cap": cap,
                    "tps": spd(SpeedMetric::OutputTokensPerSecond),
                    "ttft": spd(SpeedMetric::TimeToFirstToken),
                }),
            );
        }
        pairs.insert(key(&rf.model, &rf.effort), Value::Object(per_ref));
    }

    // Rust の判定(既定と別の許容幅の 2 通り)。ページの JS が自分の判定と突き合わせる。
    let mut golden = Map::new();
    for role in catalog.roles.keys() {
        let mut per_role = Map::new();
        for rf in &catalog.measurements {
            let mut per_ref = Map::new();
            let a = compare(catalog, role, (&rf.model, &rf.effort), lens, margins)?;
            let b = compare(catalog, role, (&rf.model, &rf.effort), lens, GOLDEN_ALT)?;
            for (x, y) in a.iter().zip(&b) {
                per_ref.insert(
                    key(&x.model, &x.effort),
                    json!([
                        verdict_str(x.judgement.verdict),
                        verdict_str(y.judgement.verdict)
                    ]),
                );
            }
            per_role.insert(key(&rf.model, &rf.effort), Value::Object(per_ref));
        }
        golden.insert(role.clone(), Value::Object(per_role));
    }

    let lens_json = lens.map(|l| {
        let cats: Vec<Value> = catalog
            .categories
            .iter()
            .map(|(id, c)| {
                json!({
                    "id": id, "name": c.name, "note": c.note,
                    "count": l.counts.get(id).copied().unwrap_or(0), "share": l.share(id),
                    "benchmarks": c.benchmarks.iter().map(|b| json!({"id": b.id, "fit": match b.fit {
                        model_catalog::Fit::Direct => "direct", model_catalog::Fit::Proxy => "proxy" }})).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({"total": l.total, "days": l.days, "generated": l.generated, "categories": cats})
    });

    let native: Vec<Value> = catalog
        .native_harness
        .iter()
        .map(|h| {
            json!({
                "model": h.model, "effort": h.effort, "harness": h.harness, "benchmark": h.benchmark,
                "score": h.score, "ci95_points": h.ci95_points, "measured": h.measured,
                "fixed": catalog.measurement(&h.model, &h.effort).and_then(|m| m.scores.get(&h.benchmark)),
                "source": {"title": h.source.title, "url": h.source.url, "retrieved": h.source.retrieved, "author": h.source.author},
            })
        })
        .collect();

    Ok(json!({
        "generated": model_catalog::date::today_utc(),
        "aa": catalog.aa,
        "margins": {"capability_points": margins.capability_points, "speed_ratio": margins.speed_ratio},
        "alt_margins": {"capability_points": GOLDEN_ALT.capability_points, "speed_ratio": GOLDEN_ALT.speed_ratio},
        "history_share_min": catalog.equivalence.history_share_min,
        "roles": roles, "benchmarks": benchmarks, "models": models, "rows": rows,
        "pairs": pairs, "golden": golden, "lens": lens_json, "native_harness": native,
    }))
}

/// データを埋め込んだページの断片(`<title>`・`<style>`・本文・`<script>`)。`</script>` で埋め込み先を
/// 閉じられないよう、`</` は `<\/` にする。Artifact として公開するときは、公開側が文書の枠(doctype・
/// html・head・body)を付けるので、この断片をそのまま渡す。
pub fn render_fragment(data: &Value) -> String {
    let json = serde_json::to_string(data)
        .expect("Value は文字列に直せる")
        .replace("</", "<\\/");
    PAGE.replace("__DATA__", &json)
}

/// ローカルのブラウザで直接開ける完全な文書(断片に doctype・charset・viewport を付ける)。
pub fn render_document(data: &Value) -> String {
    format!(
        "<!doctype html>\n<html lang=\"ja\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n{}\n</html>\n",
        render_fragment(data)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_catalog::data;

    fn lens() -> Lens {
        Lens::build(
            ["feat: a", "feat: b", "fix: c", "ci: d", "docs: e", "x"],
            "2026-10-27",
            365,
        )
    }

    #[test]
    fn data_has_every_ordered_pair_and_the_golden_verdicts() {
        let c = data();
        let v = build_data(c, Some(&lens()), &RenderOptions::default()).unwrap();
        let n = c.measurements.len();
        let pairs = v["pairs"].as_object().unwrap();
        assert_eq!(pairs.len(), n);
        assert!(pairs
            .values()
            .all(|p| p.as_object().unwrap().len() == n - 1));
        let g = &v["golden"]["execute"]["claude-sonnet-5-5:high"]["claude-sonnet-5-5:low"];
        assert_eq!(g.as_array().unwrap().len(), 2);
        assert_eq!(v["roles"].as_array().unwrap().len(), 2);
        assert_eq!(v["models"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn golden_equals_the_rust_judgement_at_both_margin_sets() {
        let c = data();
        let l = lens();
        let v = build_data(c, Some(&l), &RenderOptions::default()).unwrap();
        let m = Margins::from_catalog(c);
        let got = compare(c, "plan", ("claude-opus-5-5", "high"), Some(&l), m).unwrap();
        let alt = compare(c, "plan", ("claude-opus-5-5", "high"), Some(&l), GOLDEN_ALT).unwrap();
        for (x, y) in got.iter().zip(&alt) {
            let g = &v["golden"]["plan"]["claude-opus-5-5:high"][key(&x.model, &x.effort)];
            assert_eq!(g[0], verdict_str(x.judgement.verdict));
            assert_eq!(g[1], verdict_str(y.judgement.verdict));
        }
    }

    #[test]
    fn precomputed_interval_matches_the_judgement_and_failures_carry_a_reason() {
        let c = data();
        let v = build_data(c, None, &RenderOptions::default()).unwrap();
        let p = &v["pairs"]["claude-opus-5-5:high"]["claude-sonnet-5-5:xhigh"];
        let tb = &p["cap"]["terminalbench_v4_0"];
        let (lo, hi) = (tb["lo"].as_f64().unwrap(), tb["hi"].as_f64().unwrap());
        assert!(lo < tb["d"].as_f64().unwrap() && tb["d"].as_f64().unwrap() < hi);
        assert!(tb["res"].as_f64().unwrap() > 10.0);
        assert!(p["ttft"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn html_embeds_data_without_closing_the_script_tag_early() {
        let c = data();
        let mut v = build_data(c, None, &RenderOptions::default()).unwrap();
        v["injected"] = json!("</script><b>x</b>");
        let html = render_fragment(&v);
        assert!(!html.contains("__DATA__"));
        assert!(!html.contains("</script><b>"));
        assert!(html.contains("<\\/script><b>"));
    }

    #[test]
    fn fragment_has_no_document_frame_and_document_adds_it() {
        let c = data();
        let v = build_data(c, None, &RenderOptions::default()).unwrap();
        let frag = render_fragment(&v);
        for tag in ["<!doctype", "<html", "<head", "<body"] {
            assert!(!frag.to_ascii_lowercase().contains(tag), "{tag}");
        }
        assert!(
            frag.trim_start().starts_with("<title>"),
            "公開側は先頭 8KB から title を読む"
        );
        let doc = render_document(&v);
        assert!(doc.starts_with("<!doctype html>") && doc.contains("<meta name=\"viewport\""));
        assert!(doc.contains(&frag));
    }

    #[test]
    fn rejects_an_unknown_default_reference() {
        let c = data();
        let o = RenderOptions {
            plan_reference: "nope:high".into(),
            ..RenderOptions::default()
        };
        assert!(build_data(c, None, &o)
            .unwrap_err()
            .contains("測定値が無い"));
        let o = RenderOptions {
            plan_reference: "no-colon".into(),
            ..RenderOptions::default()
        };
        assert!(build_data(c, None, &o)
            .unwrap_err()
            .contains("MODEL:EFFORT"));
    }

    #[test]
    fn embedded_data_keeps_no_pr_titles() {
        let c = data();
        let html =
            render_fragment(&build_data(c, Some(&lens()), &RenderOptions::default()).unwrap());
        assert!(!html.contains("feat: a"));
    }
}
