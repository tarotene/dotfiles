//! 単一の HTML ページ(データとスクリプトを埋め込む)の生成。
//!
//! 許容幅 δ はページ上のスライダーで動かすので、判定の規則は JS にも要る。数百行の測定値では、
//! 全組合せ(基準 × 候補)の区間を事前計算して埋め込む方式は破綻する(30 行で 560KB、組合せは
//! 行数の 2 乗)ので、事前計算は持たない。ページ(JS)が、選んだ基準 1 点 × 全候補の Newcombe 区間と
//! 速さの比を都度計算する。
//!
//! 二重化した規則がずれないよう、Rust が出した判定を `golden` として埋め込み、ページが読み込み時に
//! 自分の計算と突き合わせて、ずれたら警告を出す。突き合わせるのは、役ごとの既定の基準 1 点に対する
//! 全候補の、(1) 2 通りの許容幅での役の判定、(2) 判定に使うベンチごとの差の区間 `[下限, 上限]`。
//! 埋め込みは行数に比例する(組合せの数には比例しない)。
//!
//! 近さの地図(`map`)の座標は、役ごと・尺度(許容幅の組)ごとに Rust が事前計算して埋め込む。座標は行数 ×
//! 2 の数値だけで、組合せの数には比例しない。距離の式は JS にも要る(選んだ基準との距離を出すため)ので、
//! 役ごとの既定の基準 1 点 × 全候補の距離も `golden.dist` として埋め込み、ページが突き合わせる。

use crate::judge::{
    capability, compare, judged_benchmarks, role_default_margins, role_resolution, Margins, Verdict,
};
use crate::lens::Lens;
use crate::map;
use model_catalog::aa::{DEFAULT_EFFORT, EFFORT_VOCABULARY};
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

fn split_ref(s: &str) -> Result<(&str, &str), String> {
    s.split_once(':')
        .ok_or_else(|| format!("基準は MODEL:EFFORT の形: {s}"))
}

/// 判定の頭文字(`golden` の文字列に使う)。e = 同等、s = 上回る、w = 弱い分野あり、i = データ不足。
fn verdict_letter(v: Verdict) -> char {
    match v {
        Verdict::Equivalent => 'e',
        Verdict::Stronger => 's',
        Verdict::Weaker => 'w',
        Verdict::Insufficient => 'i',
    }
}

/// 区間の突き合わせに使う丸め(点、小数 4 桁)。ページが許す誤差はこの丸めより大きい。
pub const INTERVAL_DECIMALS: f64 = 1e4;

fn round_points(v: f64) -> f64 {
    (v * INTERVAL_DECIMALS).round() / INTERVAL_DECIMALS
}

/// ページに出すエフォートの列の並び。語彙の段階順 → `default` → カタログにだけある段階(出現順)。
fn effort_order(catalog: &Catalog) -> Vec<String> {
    let used: BTreeSet<&str> = catalog
        .models
        .iter()
        .flat_map(|m| m.efforts.iter().map(String::as_str))
        .collect();
    let mut out: Vec<String> = EFFORT_VOCABULARY
        .iter()
        .chain(std::iter::once(&DEFAULT_EFFORT))
        .filter(|e| used.contains(**e))
        .map(|e| (*e).to_owned())
        .collect();
    for m in &catalog.models {
        for e in &m.efforts {
            if !out.contains(e) {
                out.push(e.clone());
            }
        }
    }
    out
}

pub fn build_data(
    catalog: &Catalog,
    lens: Option<&Lens>,
    opts: &RenderOptions,
) -> Result<Value, String> {
    // `[equivalence]` の値。役の既定の許容幅の下限で、ページの「既定に戻す」は役ごとの既定に戻す。
    let margins = Margins::from_catalog(catalog);

    // 役と、判定に使うベンチ(履歴に従う役は絞り込み済み)。
    let mut roles = Vec::new();
    // 役ごとの (役 ID, 既定の基準, 判定に使うベンチ) — golden の元。
    let mut golden_specs: Vec<(String, String, Vec<String>, Margins)> = Vec::new();
    let mut shown_benches: BTreeSet<String> = BTreeSet::new();
    let mut ordered: Vec<_> = catalog.roles.iter().collect();
    ordered.sort_by_key(|(id, r)| (r.order, (*id).clone()));
    for (id, role) in ordered {
        let (judged, uncovered) = judged_benchmarks(catalog, id, lens)?;
        // 判定に使うベンチは、履歴で絞る前のもの全部の値を載せる(ページ側で絞り込みの説明に使う)。
        for b in role.benchmarks.iter().chain(&role.reference_benchmarks) {
            shown_benches.insert(b.clone());
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
        // 役の既定の許容幅（#1150）: 判定に使うベンチの分解能の最大値が下限。
        let role_margins = role_default_margins(catalog, id, lens)?;
        let resolution_max = role_resolution(catalog, id, lens)?;
        golden_specs.push((
            id.clone(),
            default_ref.clone(),
            judged.iter().map(|(b, _)| b.clone()).collect(),
            role_margins,
        ));
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
            "margins": {"capability_points": role_margins.capability_points, "speed_ratio": role_margins.speed_ratio},
            "resolution_max": resolution_max,
        }));
    }

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
                "aa_name": r.aa_name, "retrieved": r.retrieved, "tps": r.median_output_tokens_per_second,
                "ttft": r.median_time_to_first_token_seconds, "scores": r.scores,
            }),
        );
    }
    // golden は、この並び(`rows` のキーの並び。ページは `Object.keys(D.rows)`)に揃えた配列で持つ。
    let keys: Vec<String> = rows.keys().cloned().collect();

    // Rust の判定を、役ごとの既定の基準 1 点 × 全候補で埋め込む。ページの JS が自分の計算と突き合わせる。
    //   v:  候補ごとに 2 文字(既定の許容幅・別の許容幅)の判定。基準自身は "--"。
    //   iv: 判定に使うベンチごとに、候補ごとの差の区間 [下限, 上限](点)。作れないものは null。
    let mut golden = Map::new();
    // 近さの地図(役ごと)。座標は、既定の許容幅と別の許容幅の 2 通りの尺度で事前計算する(スライダーでは作り直さない)。
    let by_key: std::collections::BTreeMap<String, &model_catalog::Measurement> = catalog
        .measurements
        .iter()
        .map(|r| (key(&r.model, &r.effort), r))
        .collect();
    let meas: Vec<&model_catalog::Measurement> = keys.iter().map(|k| by_key[k]).collect();
    let mut map_roles = Map::new();
    for (role, default_ref, judged, role_margins) in &golden_specs {
        let map_sets = [*role_margins, GOLDEN_ALT];
        let (m, e) = split_ref(default_ref)?;
        let a = compare(catalog, role, (m, e), lens, *role_margins)?;
        let b = compare(catalog, role, (m, e), lens, GOLDEN_ALT)?;
        let mut by_key: std::collections::BTreeMap<String, String> = Default::default();
        for (x, y) in a.iter().zip(&b) {
            by_key.insert(
                key(&x.model, &x.effort),
                format!(
                    "{}{}",
                    verdict_letter(x.judgement.verdict),
                    verdict_letter(y.judgement.verdict)
                ),
            );
        }
        let v: String = keys
            .iter()
            .map(|k| by_key.get(k).map_or("--", String::as_str))
            .collect();
        let base = catalog.measurement(m, e).expect("基準の存在は確認済み");
        let mut iv = Map::new();
        for id in judged {
            let bench = &catalog.benchmarks[id];
            let mut flat = Vec::with_capacity(keys.len() * 2);
            for k in &keys {
                let cand = catalog
                    .measurements
                    .iter()
                    .find(|r| &key(&r.model, &r.effort) == k)
                    .expect("rows のキーは測定値から作った");
                let interval = if k == default_ref {
                    None
                } else {
                    capability(
                        id,
                        bench,
                        base.scores.get(id).copied(),
                        cand.scores.get(id).copied(),
                        role_margins.capability_points,
                    )
                    .interval
                };
                match interval {
                    Some((lo, hi)) => {
                        flat.push(json!(round_points(lo)));
                        flat.push(json!(round_points(hi)));
                    }
                    None => {
                        flat.push(Value::Null);
                        flat.push(Value::Null);
                    }
                }
            }
            iv.insert(id.clone(), Value::Array(flat));
        }
        let axes = map::axes(catalog, judged)?;
        let vals: Vec<_> = meas.iter().map(|m| map::values(&axes, m)).collect();
        let ref_idx = keys
            .iter()
            .position(|k| k == default_ref)
            .expect("基準の存在は確認済み");
        let (role_map, dist) = map::role_json(&axes, &vals, &map_sets, ref_idx);
        map_roles.insert(role.clone(), role_map);
        golden.insert(
            role.clone(),
            json!({"ref": default_ref, "v": v, "iv": Value::Object(iv), "dist": dist}),
        );
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
        "effort_order": effort_order(catalog),
        "roles": roles, "benchmarks": benchmarks, "models": models, "rows": rows,
        "golden": golden, "lens": lens_json, "native_harness": native,
        "map": {"min_common": map::MIN_COMMON, "stress_warn": map::STRESS_WARN, "roles": map_roles},
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
    use model_catalog::{data, CATALOG_TOML};
    use std::process::Command;

    fn lens() -> Lens {
        Lens::build(
            data(),
            ["feat: a", "feat: b", "fix: c", "ci: d", "docs: e", "x"],
            "2026-10-27",
            365,
        )
    }

    fn letters(v: &Value) -> Vec<char> {
        v.as_str().unwrap().chars().collect()
    }

    /// 数百行の測定値を持つカタログ(AA の応答の代わりに、決定的に作った行を `fetch-aa` の取り込みに通す)。
    /// 33 社 × 2 モデル × 5 エフォート = 330 行を手登録の 30 行に足す。一部のベンチは値を欠く。
    fn big_catalog() -> Catalog {
        let efforts = ["Low", "Medium", "High", "XHigh", "Max"];
        let mut rows = Vec::new();
        let mut n = 0u64;
        for p in 0..33u64 {
            for m in 0..2u64 {
                for e in efforts {
                    n += 1;
                    let frac = |salt: u64| ((n * 7919 + salt * 104_729) % 1000) as f64 / 1000.0;
                    let sci = if n.is_multiple_of(7) {
                        "null".to_owned()
                    } else {
                        frac(3).to_string()
                    };
                    rows.push(format!(
                        r#"{{"id":"00000000-0000-0000-0000-{n:012}","name":"Model {p}-{m} ({e})","slug":"model-{p}-{m}-{e}",
                        "release_date":"2026-09-15","model_creator":{{"name":"Maker {p}"}},
                        "median_output_tokens_per_second":{tps},"median_time_to_first_token_seconds":{ttft},
                        "evaluations":{{"terminalbench_v4_0":{tb},"scicode":{sci},"lcr":{lcr},"hle":{hle},
                        "artificial_analysis_intelligence_index":{idx}}}}}"#,
                        tps = 20.0 + frac(1) * 100.0,
                        ttft = 1.0 + frac(2) * 30.0,
                        tb = frac(4),
                        lcr = frac(5),
                        hle = frac(6) * 0.6,
                        idx = frac(7) * 60.0,
                    ));
                }
            }
        }
        let json = format!(
            r#"{{"status":200,"prompt_options":{{"parallel_queries":1,"prompt_length":1000}},"data":[{}]}}"#,
            rows.join(",")
        );
        let (_, text) = model_catalog::aa::run(CATALOG_TOML, &json, "2026-10-28").unwrap();
        Catalog::parse(&text).unwrap()
    }

    /// ページの JS の核(`CORE BEGIN`〜`CORE END`)を node で実行し、埋め込んだ golden と突き合わせる。
    /// node が無い環境では None(ブラウザが読み込み時に同じ突き合わせをする)。
    fn run_page_core(data: &Value) -> Option<Value> {
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("node が無いので、JS の核の突き合わせは省略(ページが読み込み時に行う)");
            return None;
        }
        let begin = PAGE.find("// ==== CORE BEGIN").expect("核の開始行");
        let end = PAGE.find("// ==== CORE END ====").expect("核の終了行");
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("data.json");
        let script = dir.path().join("check.js");
        std::fs::write(&json, serde_json::to_string(data).unwrap()).unwrap();
        std::fs::write(
            &script,
            format!(
                "\"use strict\";\nconst D = JSON.parse(require('fs').readFileSync(process.argv[2], 'utf8'));\n{}\nconsole.log(JSON.stringify(selfCheckCore()));\n",
                &PAGE[begin..end]
            ),
        )
        .unwrap();
        let out = Command::new("node")
            .arg(&script)
            .arg(&json)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(serde_json::from_slice(&out.stdout).unwrap())
    }

    #[test]
    fn data_embeds_no_precomputed_pairs_and_one_golden_per_role() {
        let c = data();
        let v = build_data(c, Some(&lens()), &RenderOptions::default()).unwrap();
        assert!(v.get("pairs").is_none(), "全組合せの区間は埋め込まない");
        let n = c.measurements.len();
        assert_eq!(v["rows"].as_object().unwrap().len(), n);
        let g = v["golden"].as_object().unwrap();
        assert_eq!(g.len(), 2);
        for (role, want_ref) in [
            ("plan", "claude-opus-5-5:high"),
            ("execute", "claude-sonnet-5-5:high"),
        ] {
            assert_eq!(g[role]["ref"], want_ref);
            assert_eq!(g[role]["v"].as_str().unwrap().len(), 2 * n);
        }
        assert_eq!(v["roles"].as_array().unwrap().len(), 2);
        assert_eq!(v["models"].as_array().unwrap().len(), 6);
        assert!(v["rows"]["claude-opus-5-5:high"]["retrieved"].is_string());
    }

    #[test]
    fn golden_equals_the_rust_judgement_at_both_margin_sets() {
        let c = data();
        let l = lens();
        let v = build_data(c, Some(&l), &RenderOptions::default()).unwrap();
        let m = role_default_margins(c, "plan", Some(&l)).unwrap();
        let keys: Vec<&String> = v["rows"].as_object().unwrap().keys().collect();
        let got = compare(c, "plan", ("claude-opus-5-5", "high"), Some(&l), m).unwrap();
        let alt = compare(c, "plan", ("claude-opus-5-5", "high"), Some(&l), GOLDEN_ALT).unwrap();
        let ls = letters(&v["golden"]["plan"]["v"]);
        for (x, y) in got.iter().zip(&alt) {
            let i = keys
                .iter()
                .position(|k| **k == key(&x.model, &x.effort))
                .unwrap();
            assert_eq!(ls[2 * i], verdict_letter(x.judgement.verdict));
            assert_eq!(ls[2 * i + 1], verdict_letter(y.judgement.verdict));
        }
        let me = keys
            .iter()
            .position(|k| *k == "claude-opus-5-5:high")
            .unwrap();
        assert_eq!(
            (ls[2 * me], ls[2 * me + 1]),
            ('-', '-'),
            "基準自身は判定しない"
        );
    }

    #[test]
    fn golden_intervals_equal_the_rust_interval_and_failures_are_null() {
        let c = data();
        let v = build_data(c, None, &RenderOptions::default()).unwrap();
        let keys: Vec<&String> = v["rows"].as_object().unwrap().keys().collect();
        let i = keys
            .iter()
            .position(|k| *k == "claude-sonnet-5-5:xhigh")
            .unwrap();
        let iv = &v["golden"]["execute"]["iv"]["terminalbench_v4_0"];
        let want = capability(
            "terminalbench_v4_0",
            &c.benchmarks["terminalbench_v4_0"],
            c.measurement("claude-sonnet-5-5", "high")
                .unwrap()
                .scores
                .get("terminalbench_v4_0")
                .copied(),
            c.measurement("claude-sonnet-5-5", "xhigh")
                .unwrap()
                .scores
                .get("terminalbench_v4_0")
                .copied(),
            5.0,
        )
        .interval
        .unwrap();
        assert_eq!(iv[2 * i], json!(round_points(want.0)));
        assert_eq!(iv[2 * i + 1], json!(round_points(want.1)));
        // 基準自身の区間は無い。
        let me = keys
            .iter()
            .position(|k| *k == "claude-sonnet-5-5:high")
            .unwrap();
        assert!(iv[2 * me].is_null() && iv[2 * me + 1].is_null());
    }

    #[test]
    fn effort_columns_follow_the_vocabulary_then_default_then_the_rest() {
        let c = big_catalog();
        let o = effort_order(&c);
        assert_eq!(o, ["low", "medium", "high", "xhigh", "max"]);
        // 語彙に無い段階はその後ろ、default は語彙の段階の後ろ。
        let mut c = c;
        c.models[0].efforts = vec!["ultra".into(), "default".into(), "minimal".into()];
        let o = effort_order(&c);
        assert_eq!(
            o,
            ["minimal", "low", "medium", "high", "xhigh", "max", "default", "ultra"]
        );
    }

    #[test]
    fn hundreds_of_rows_stay_within_a_few_hundred_kb_and_golden_grows_linearly() {
        let c = big_catalog();
        let n = c.measurements.len();
        assert!(n >= 360, "{n}");
        let v = build_data(&c, Some(&lens()), &RenderOptions::default()).unwrap();
        let html = render_document(&v);
        let kb = html.len() / 1024;
        eprintln!("{n} 行のページ: {kb} KB");
        assert!(kb < 400, "数百 KB に収まる: {kb} KB");
        assert!(v.get("pairs").is_none());
        for role in ["plan", "execute"] {
            assert_eq!(v["golden"][role]["v"].as_str().unwrap().len(), 2 * n);
        }
        // 近さの地図: 座標は行数 × 2 の数値だけ。500KB を超えない。
        assert!(kb < 500, "地図を含めても 500KB 未満: {kb} KB");
        let map_bytes = serde_json::to_string(&v["map"]).unwrap().len();
        eprintln!("地図の座標: {} KB", map_bytes / 1024);
        assert!(map_bytes < 100 * 1024, "{map_bytes} bytes");
        for role in ["plan", "execute"] {
            for set in v["map"]["roles"][role]["sets"].as_array().unwrap() {
                let stress = set["stress"].as_f64().unwrap();
                eprintln!(
                    "{n} 行 {role} {}: stress {stress}、載せた点 {}、補った組 {}",
                    set["margins"], set["n_points"], set["filled_pairs"]
                );
                assert!((0.0..=1.0).contains(&stress));
                assert_eq!(set["pts"].as_array().unwrap().len(), 2 * n);
            }
        }
        // 事前計算していた頃の、組合せ数(行数の 2 乗)に比例した埋め込みが無いこと。
        let golden = serde_json::to_string(&v["golden"]).unwrap();
        assert!(golden.len() < 90 * n * 4, "{} bytes", golden.len());
    }

    #[test]
    fn map_embeds_a_coordinate_pair_per_row_for_each_role_and_scale() {
        let c = data();
        let v = build_data(c, Some(&lens()), &RenderOptions::default()).unwrap();
        let n = c.measurements.len();
        assert_eq!(v["map"]["min_common"], map::MIN_COMMON);
        let roles = v["map"]["roles"].as_object().unwrap();
        assert_eq!(roles.len(), 2);
        for (role, m) in roles {
            let sets = m["sets"].as_array().unwrap();
            assert_eq!(sets.len(), 2, "既定と別の許容幅の 2 通り");
            assert_ne!(sets[0]["margins"], sets[1]["margins"]);
            for set in sets {
                let pts = set["pts"].as_array().unwrap();
                assert_eq!(pts.len(), 2 * n, "{role}: 行ごとに x と y");
                let placed = pts.chunks(2).filter(|p| p[0].is_number()).count();
                assert_eq!(placed as u64, set["n_points"].as_u64().unwrap());
                assert!(placed >= 8, "{role}: 地図に載る点が少なすぎる: {placed}");
                let stress = set["stress"].as_f64().unwrap();
                eprintln!("{role} {}: stress {stress}", set["margins"]);
                assert!((0.0..1.0).contains(&stress), "{role}: {stress}");
            }
            // 座標は、役の判定に使う成分(ベンチ + 速さ 2 つ)から作る。
            let axes = m["axes"].as_array().unwrap();
            assert!(axes.len() >= 3, "{role}: {axes:?}");
        }
        // 基準自身の距離は無く、ほかの行の距離が埋め込まれる。
        for role in ["plan", "execute"] {
            assert_eq!(v["golden"][role]["dist"].as_array().unwrap().len(), 2 * n);
        }
    }

    #[test]
    fn the_map_is_deterministic() {
        let c = data();
        let l = lens();
        let a = build_data(c, Some(&l), &RenderOptions::default()).unwrap();
        let b = build_data(c, Some(&l), &RenderOptions::default()).unwrap();
        assert_eq!(a["map"], b["map"], "同じ入力は同じ座標");
        assert_eq!(render_document(&a), render_document(&b));
    }

    #[test]
    fn the_page_embeds_the_map_and_its_script_draws_it() {
        let c = data();
        let v = build_data(c, None, &RenderOptions::default()).unwrap();
        let html = render_document(&v);
        // 座標を埋め込んでいる(JSON のキーと、先頭の行の座標)。
        assert!(html.contains("\"map\":{"), "map のデータ");
        let first = &v["map"]["roles"]["plan"]["sets"][0]["pts"];
        let x = serde_json::to_string(
            first
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p.is_number())
                .unwrap(),
        )
        .unwrap();
        assert!(html.contains(&x), "座標 {x}");
        // 描画する側。
        for needle in [
            "id=\"mapsvg\"",
            "function renderMap()",
            "D.map.roles[S.role]",
            "mapDistance(",
        ] {
            assert!(PAGE.contains(needle), "{needle}");
        }
        // 役ごとの地図と、スマートフォン幅で操作できる大きさの当たり判定・キーボード操作。
        assert!(PAGE.contains("role=\"button\"") && PAGE.contains("tabindex=\"0\""));
        assert!(
            PAGE.contains("viewBox=\"0 0 360 360\"") && PAGE.contains("setAttribute(\"viewBox\"")
        );
    }

    #[test]
    fn the_page_core_notices_a_tampered_distance() {
        let c = data();
        let mut v = build_data(c, None, &RenderOptions::default()).unwrap();
        let d = v["golden"]["plan"]["dist"].as_array_mut().unwrap();
        let i = d.iter().position(|x| x.is_number()).unwrap();
        d[i] = json!(d[i].as_f64().unwrap() + 0.5);
        if let Some(r) = run_page_core(&v) {
            assert_eq!(r["bad"].as_array().unwrap().len(), 1, "{r}");
        }
    }

    #[test]
    fn the_page_core_agrees_with_rust_on_the_embedded_catalog() {
        let c = data();
        let v = build_data(c, Some(&lens()), &RenderOptions::default()).unwrap();
        if let Some(r) = run_page_core(&v) {
            assert_eq!(r["bad"].as_array().unwrap().len(), 0, "{r}");
            // 2 役 ×(29 候補 ×(判定 2 + 区間 2))= 232 件ほど。
            assert!(r["n"].as_u64().unwrap() > 200, "{r}");
        }
    }

    #[test]
    fn the_page_core_agrees_with_rust_for_many_references_at_hundreds_of_rows() {
        let c = big_catalog();
        let l = lens();
        // 既定の基準を、行を等間隔に 12 点選んで変える(埋め込む golden は基準ごとに作り直される)。
        // 手登録の行・自動登録の行・応答から消えた行の持ち越しを含め、基準 × 全候補を突き合わせる。
        let keys: Vec<String> = c
            .measurements
            .iter()
            .map(|r| key(&r.model, &r.effort))
            .collect();
        let step = keys.len() / 12;
        let mut compared = 0;
        for k in keys.iter().step_by(step) {
            let opts = RenderOptions {
                plan_reference: k.clone(),
                execute_reference: keys[keys.len() - 1].clone(),
            };
            let v = build_data(&c, Some(&l), &opts).unwrap();
            let Some(r) = run_page_core(&v) else { return };
            assert_eq!(r["bad"].as_array().unwrap().len(), 0, "{k}: {r}");
            compared += r["n"].as_u64().unwrap();
        }
        assert!(compared > 12 * 2000, "{compared}");
    }

    #[test]
    fn the_page_core_notices_a_tampered_golden() {
        let c = data();
        let mut v = build_data(c, None, &RenderOptions::default()).unwrap();
        // 判定を 1 文字、区間を 1 つ書き換える。
        let mut s: Vec<char> = letters(&v["golden"]["plan"]["v"]);
        let i = s.iter().position(|c| *c == 'w').expect("弱い候補がある");
        s[i] = 'e';
        v["golden"]["plan"]["v"] = json!(s.into_iter().collect::<String>());
        let iv = v["golden"]["execute"]["iv"]["terminalbench_v4_0"]
            .as_array_mut()
            .unwrap();
        let j = iv.iter().position(|x| x.is_number()).unwrap();
        iv[j] = json!(iv[j].as_f64().unwrap() + 1.0);
        if let Some(r) = run_page_core(&v) {
            assert_eq!(r["bad"].as_array().unwrap().len(), 2, "{r}");
        }
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
