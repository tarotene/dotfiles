//! 同等性の判定。能力は TOST(差の 90% 区間が ±δ の内側)、速さは点推定の比。
//!
//! 「差が有意でない」ことは「同等」の証拠にならない(Lakens 2017)。区間が作れないものは、
//! 同等とは言わず「データ不足」にする。総合点は作らず、役ごとに、判定に使うベンチすべてを見る。

use crate::lens::Lens;
use crate::stats::{newcombe_diff, Z90};
use model_catalog::{Benchmark, Catalog, Fit, Measurement, Unit};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// 差の区間が ±δ の内側に収まる(速さは比が範囲内)。
    Equivalent,
    /// 差の区間全体が −δ より下に外れる。
    Weaker,
    /// 差の区間全体が +δ より上に外れる。
    Stronger,
    /// 区間が ±δ をまたぐ、または区間を作れない。同等とも違うとも言えない。
    Insufficient,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Equivalent => "同等",
            Verdict::Weaker => "弱い分野あり",
            Verdict::Stronger => "上回る",
            Verdict::Insufficient => "データ不足",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Margins {
    pub capability_points: f64,
    pub speed_ratio: f64,
}

impl Margins {
    pub fn from_catalog(c: &Catalog) -> Margins {
        Margins {
            capability_points: c.equivalence.capability_margin_points,
            speed_ratio: c.equivalence.speed_margin_ratio,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedMetric {
    OutputTokensPerSecond,
    TimeToFirstToken,
}

impl SpeedMetric {
    pub fn id(self) -> &'static str {
        match self {
            SpeedMetric::OutputTokensPerSecond => "median_output_tokens_per_second",
            SpeedMetric::TimeToFirstToken => "median_time_to_first_token_seconds",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SpeedMetric::OutputTokensPerSecond => "出力速度",
            SpeedMetric::TimeToFirstToken => "最初のトークンまでの時間",
        }
    }

    pub fn value(self, m: &Measurement) -> Option<f64> {
        match self {
            SpeedMetric::OutputTokensPerSecond => m.median_output_tokens_per_second,
            SpeedMetric::TimeToFirstToken => m.median_time_to_first_token_seconds,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Judgement {
    pub metric: String,
    pub label: String,
    pub verdict: Verdict,
    pub reference: Option<f64>,
    pub candidate: Option<f64>,
    /// 差の 90% 区間(能力のみ。単位はベンチの値と同じ)。
    pub interval: Option<(f64, f64)>,
    /// 許容幅(能力は値と同じ単位、速さは比)。
    pub margin: f64,
    /// 履歴の内訳から見て、このベンチが仕事そのものを測るか(direct)、代理か(proxy)。
    pub fit: Option<&'static str>,
    /// このベンチの分解能(点)。基準と同じ点数の候補でも、差の区間は ±この値まで広がる。
    /// 許容幅がこれより狭いと、差が無くても「同等」と言えない。
    pub resolution_points: Option<f64>,
    pub basis: String,
}

/// 点数 `p`(0..1)・課題数 `n` のベンチの分解能(点)。同じ点数どうしの差の 90% 区間の半幅で、
/// 点数が同じ候補でも差の区間はここまで広がる。地図の距離(`map`)も同じ式で、ベンチの尺度を決める。
pub fn resolution_points(p: f64, n: f64) -> f64 {
    let (lo, hi) = newcombe_diff(p, n, p, n, Z90);
    (hi - lo) / 2.0 * 100.0
}

/// 差の区間 `[lo, hi]` を許容幅 `margin` で 4 値に分ける。同等は、区間が ±margin の内側(境界を含まない)。
pub fn classify_interval(lo: f64, hi: f64, margin: f64) -> Verdict {
    if lo > -margin && hi < margin {
        Verdict::Equivalent
    } else if hi < -margin {
        Verdict::Weaker
    } else if lo > margin {
        Verdict::Stronger
    } else {
        Verdict::Insufficient
    }
}

pub fn capability(
    id: &str,
    bench: &Benchmark,
    reference: Option<f64>,
    candidate: Option<f64>,
    margin_points: f64,
) -> Judgement {
    let mut j = Judgement {
        metric: id.to_owned(),
        label: bench.name.clone(),
        verdict: Verdict::Insufficient,
        reference,
        candidate,
        interval: None,
        margin: margin_points,
        fit: None,
        resolution_points: None,
        basis: String::new(),
    };
    let (Some(r), Some(c)) = (reference, candidate) else {
        j.basis = "基準か候補のどちらかに、このベンチの値が無い。".into();
        return j;
    };
    if bench.unit != Unit::Fraction {
        j.basis = "割合でない指数なので、二項分布の区間を作れない。表示だけにする。".into();
        return j;
    }
    let (Some(n), Some(unit)) = (bench.n, bench.n_unit.as_deref()) else {
        j.basis = "課題数が公開されておらず、区間を作れない。".into();
        return j;
    };
    let margin = margin_points / 100.0;
    let (lo, hi) = newcombe_diff(c, f64::from(n), r, f64::from(n), Z90);
    j.interval = Some((lo * 100.0, hi * 100.0));
    let resolution = resolution_points(r, f64::from(n));
    j.resolution_points = Some(resolution);
    j.verdict = classify_interval(lo, hi, margin);
    let where_ = match j.verdict {
        Verdict::Equivalent => "の内側に収まる",
        Verdict::Weaker => "より下に外れる",
        Verdict::Stronger => "より上に外れる",
        Verdict::Insufficient => "をまたぐ",
    };
    j.basis = format!(
        "差 {:+.1}pt の 90% 区間 [{:+.1}, {:+.1}]pt が ±{:.1}pt {where_}。n={n} {unit} の独立 2 標本として \
         Newcombe(1998)の区間で計算(同じ課題での対応を無視するので、同等を言い過ぎない側に倒れる)。このベンチの分解能は ±{resolution:.1}pt で、許容幅がこれより狭いと、差が無くても同等と言えない。",
        (c - r) * 100.0,
        lo * 100.0,
        hi * 100.0,
        margin_points,
    );
    j
}

/// 候補の「良さ」の基準に対する比(1 より大きいほど候補が良い)。出力速度は速いほど、待ちは短いほど良い。
pub fn goodness_ratio(metric: SpeedMetric, reference: f64, candidate: f64) -> f64 {
    match metric {
        SpeedMetric::OutputTokensPerSecond => candidate / reference,
        SpeedMetric::TimeToFirstToken => reference / candidate,
    }
}

pub fn speed(
    metric: SpeedMetric,
    reference: Option<f64>,
    candidate: Option<f64>,
    margin_ratio: f64,
) -> Judgement {
    let mut j = Judgement {
        metric: metric.id().to_owned(),
        label: metric.label().to_owned(),
        verdict: Verdict::Insufficient,
        reference,
        candidate,
        interval: None,
        margin: margin_ratio,
        fit: None,
        resolution_points: None,
        basis: String::new(),
    };
    let (Some(r), Some(c)) = (reference, candidate) else {
        j.basis = "基準か候補のどちらかに、この速さの値が無い。".into();
        return j;
    };
    if !(r > 0.0 && c > 0.0) {
        j.basis = "値が 0 以下で、比を作れない。".into();
        return j;
    }
    // g > 1 が「候補の方が良い」になるように向きをそろえる(出力速度は速いほど、待ちは短いほど良い)。
    let g = goodness_ratio(metric, r, c);
    let up = 1.0 + margin_ratio;
    let down = 1.0 / up;
    j.verdict = if g < down {
        Verdict::Weaker
    } else if g > up {
        Verdict::Stronger
    } else {
        Verdict::Equivalent
    };
    let shown = match metric {
        SpeedMetric::OutputTokensPerSecond => format!("候補は基準の {:.2} 倍の速さ", c / r),
        SpeedMetric::TimeToFirstToken => format!("候補の待ちは基準の {:.2} 倍", c / r),
    };
    j.basis = format!(
        "{shown}(点推定のみ。AA は区間を公開していない)。±{:.0}% は良さの比が {down:.2}〜{up:.2} 倍。",
        margin_ratio * 100.0
    );
    j
}

/// 役の判定への束ね方: 弱い分野が 1 つでもあれば弱い、無ければデータ不足が 1 つでもあれば不足、
/// 全部が「上回る」なら上回る、それ以外(同等と上回るの混在を含む)は同等。判定が 0 件なら不足。
pub fn combine(verdicts: &[Verdict]) -> Verdict {
    if verdicts.is_empty() {
        Verdict::Insufficient
    } else if verdicts.contains(&Verdict::Weaker) {
        Verdict::Weaker
    } else if verdicts.contains(&Verdict::Insufficient) {
        Verdict::Insufficient
    } else if verdicts.iter().all(|v| *v == Verdict::Stronger) {
        Verdict::Stronger
    } else {
        Verdict::Equivalent
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReferenceValue {
    pub benchmark: String,
    pub label: String,
    pub reference: Option<f64>,
    pub candidate: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Uncovered {
    pub category: String,
    pub name: String,
    pub share: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoleJudgement {
    pub role: String,
    pub verdict: Verdict,
    pub judgements: Vec<Judgement>,
    /// 判定には使わず、表示だけするベンチ。
    pub references: Vec<ReferenceValue>,
    /// 履歴の内訳で一定以上を占めるのに、現行モデルに値のある公開ベンチが無い種類。
    pub uncovered: Vec<Uncovered>,
    pub note: Option<String>,
}

fn fit_str(f: Fit) -> &'static str {
    match f {
        Fit::Direct => "direct",
        Fit::Proxy => "proxy",
    }
}

/// 判定に使うベンチ(ID と、仕事を直接測るか代理かの別)と、ベンチの無い種類。
pub type Judged = (Vec<(String, Option<&'static str>)>, Vec<Uncovered>);

/// 役が判定に使うベンチと、履歴で一定以上を占めるのにベンチの無い種類を返す。
pub fn judged_benchmarks(
    catalog: &Catalog,
    role_id: &str,
    lens: Option<&Lens>,
) -> Result<Judged, String> {
    let role = catalog
        .roles
        .get(role_id)
        .ok_or_else(|| format!("未知の役: {role_id}"))?;
    let Some(lens) = lens.filter(|_| role.follows_history) else {
        return Ok((
            role.benchmarks.iter().map(|b| (b.clone(), None)).collect(),
            vec![],
        ));
    };
    let min = catalog.equivalence.history_share_min;
    let mut linked: Vec<(&str, Fit)> = Vec::new();
    let mut uncovered = Vec::new();
    for (id, cat) in &catalog.categories {
        let share = lens.share(id);
        if share < min {
            continue;
        }
        if cat.benchmarks.is_empty() {
            uncovered.push(Uncovered {
                category: id.clone(),
                name: cat.name.clone(),
                share,
            });
        }
        for b in &cat.benchmarks {
            linked.push((b.id.as_str(), b.fit));
        }
    }
    uncovered.sort_by(|a, b| b.share.total_cmp(&a.share));
    let wanted: BTreeSet<&str> = linked.iter().map(|(b, _)| *b).collect();
    let judged = role
        .benchmarks
        .iter()
        .filter(|b| wanted.contains(b.as_str()))
        .map(|b| {
            // 仕事そのものを測る種類が 1 つでもあれば direct、無ければ proxy。
            let direct = linked.iter().any(|(id, f)| id == b && *f == Fit::Direct);
            (
                b.clone(),
                Some(if direct {
                    fit_str(Fit::Direct)
                } else {
                    fit_str(Fit::Proxy)
                }),
            )
        })
        .collect();
    Ok((judged, uncovered))
}

pub fn judge_role(
    catalog: &Catalog,
    role_id: &str,
    reference: &Measurement,
    candidate: &Measurement,
    lens: Option<&Lens>,
    margins: Margins,
) -> Result<RoleJudgement, String> {
    let role = catalog
        .roles
        .get(role_id)
        .ok_or_else(|| format!("未知の役: {role_id}"))?;
    let (judged, uncovered) = judged_benchmarks(catalog, role_id, lens)?;
    let mut judgements = Vec::new();
    for (id, fit) in &judged {
        let bench = catalog
            .benchmarks
            .get(id)
            .ok_or_else(|| format!("未定義のベンチ: {id}"))?;
        let mut j = capability(
            id,
            bench,
            reference.scores.get(id).copied(),
            candidate.scores.get(id).copied(),
            margins.capability_points,
        );
        j.fit = *fit;
        judgements.push(j);
    }
    for metric in [
        SpeedMetric::OutputTokensPerSecond,
        SpeedMetric::TimeToFirstToken,
    ] {
        judgements.push(speed(
            metric,
            metric.value(reference),
            metric.value(candidate),
            margins.speed_ratio,
        ));
    }
    let references = role
        .reference_benchmarks
        .iter()
        .map(|id| ReferenceValue {
            benchmark: id.clone(),
            label: catalog
                .benchmarks
                .get(id)
                .map_or(id.clone(), |b| b.name.clone()),
            reference: reference.scores.get(id).copied(),
            candidate: candidate.scores.get(id).copied(),
        })
        .collect();
    let capability_count = judged.len();
    let note = (capability_count == 0).then(|| {
        "履歴の内訳に対応するベンチが、この役の判定に使うベンチに 1 つも無い。能力は判定できない。".to_owned()
    });
    let mut verdicts: Vec<Verdict> = judgements.iter().map(|j| j.verdict).collect();
    if capability_count == 0 {
        verdicts.push(Verdict::Insufficient);
    }
    Ok(RoleJudgement {
        role: role_id.to_owned(),
        verdict: combine(&verdicts),
        judgements,
        references,
        uncovered,
        note,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Candidate {
    pub model: String,
    pub model_name: String,
    pub retired: bool,
    pub effort: String,
    pub judgement: RoleJudgement,
}

/// 基準 `(model, effort)` に対する、ほかのすべての (モデル × エフォート) の判定。
pub fn compare(
    catalog: &Catalog,
    role_id: &str,
    reference: (&str, &str),
    lens: Option<&Lens>,
    margins: Margins,
) -> Result<Vec<Candidate>, String> {
    let base = catalog
        .measurement(reference.0, reference.1)
        .ok_or_else(|| format!("基準 {} / {} の測定値が無い", reference.0, reference.1))?;
    let mut out = Vec::new();
    for m in &catalog.measurements {
        if (m.model.as_str(), m.effort.as_str()) == reference {
            continue;
        }
        let model = catalog
            .model(&m.model)
            .ok_or_else(|| format!("未知の model {}", m.model))?;
        out.push(Candidate {
            model: m.model.clone(),
            model_name: model.name.clone(),
            retired: model.status == model_catalog::Status::Retired,
            effort: m.effort.clone(),
            judgement: judge_role(catalog, role_id, base, m, lens, margins)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_catalog::data;
    use std::collections::BTreeMap;

    fn tb() -> Benchmark {
        data().benchmarks["terminalbench_v4_0"].clone()
    }

    #[test]
    fn interval_classes_follow_tost_with_strict_inner_bounds() {
        assert_eq!(classify_interval(-0.04, 0.04, 0.05), Verdict::Equivalent);
        assert_eq!(
            classify_interval(-0.05, 0.04, 0.05),
            Verdict::Insufficient,
            "境界ちょうどは内側と言わない"
        );
        assert_eq!(classify_interval(-0.30, -0.06, 0.05), Verdict::Weaker);
        assert_eq!(
            classify_interval(-0.30, -0.04, 0.05),
            Verdict::Insufficient,
            "下側にまたがる"
        );
        assert_eq!(classify_interval(0.06, 0.30, 0.05), Verdict::Stronger);
        assert_eq!(classify_interval(0.04, 0.30, 0.05), Verdict::Insufficient);
        assert_eq!(classify_interval(-0.2, 0.2, 0.05), Verdict::Insufficient);
    }

    #[test]
    fn identical_scores_are_not_equivalent_at_66_tasks_with_a_5_point_margin() {
        // 同じ点数でも、66 課題の区間は ±5pt に収まらない。「差が無い」を「同等」と言わない。
        let j = capability("terminalbench_v4_0", &tb(), Some(0.5), Some(0.5), 5.0);
        assert_eq!(j.verdict, Verdict::Insufficient, "{}", j.basis);
        let (lo, hi) = j.interval.unwrap();
        assert!(hi - lo > 20.0, "{lo} {hi}");
    }

    #[test]
    fn a_large_gap_is_weaker_or_stronger() {
        let weak = capability("terminalbench_v4_0", &tb(), Some(0.9), Some(0.2), 5.0);
        assert_eq!(weak.verdict, Verdict::Weaker, "{}", weak.basis);
        let strong = capability("terminalbench_v4_0", &tb(), Some(0.2), Some(0.9), 5.0);
        assert_eq!(strong.verdict, Verdict::Stronger, "{}", strong.basis);
    }

    #[test]
    fn resolution_follows_the_sample_size_and_says_when_a_margin_is_too_narrow() {
        let c = data();
        let res = |id: &str| {
            capability(id, &c.benchmarks[id], Some(0.5), Some(0.5), 5.0)
                .resolution_points
                .unwrap()
        };
        // 同じ点数 0.5 でも、66 課題は ±14pt、100 問は ±12pt、2158 問は ±2.5pt まで区間が広がる。
        assert!(
            (res("terminalbench_v4_0") - 14.0).abs() < 0.5,
            "{}",
            res("terminalbench_v4_0")
        );
        assert!((res("lcr") - 11.5).abs() < 0.8, "{}", res("lcr"));
        assert!((res("hle") - 2.5).abs() < 0.3, "{}", res("hle"));
        // 許容幅が分解能より狭いと、点数が同じでも同等にならず、根拠欄がその理由を言う。
        let narrow = capability("hle", &c.benchmarks["hle"], Some(0.5), Some(0.5), 1.0);
        assert_eq!(narrow.verdict, Verdict::Insufficient);
        assert!(narrow.basis.contains("分解能は"), "{}", narrow.basis);
        let enough = capability("hle", &c.benchmarks["hle"], Some(0.5), Some(0.5), 3.0);
        assert_eq!(enough.verdict, Verdict::Equivalent, "{}", enough.basis);
    }

    #[test]
    fn a_wide_margin_makes_close_scores_equivalent() {
        let j = capability("terminalbench_v4_0", &tb(), Some(0.5), Some(0.51), 40.0);
        assert_eq!(j.verdict, Verdict::Equivalent, "{}", j.basis);
    }

    #[test]
    fn missing_values_and_missing_n_are_insufficient_with_a_reason() {
        let j = capability("terminalbench_v4_0", &tb(), None, Some(0.5), 5.0);
        assert_eq!(j.verdict, Verdict::Insufficient);
        assert!(j.basis.contains("値が無い"));
        let index = &data().benchmarks["artificial_analysis_intelligence_index"];
        let j = capability(
            "artificial_analysis_intelligence_index",
            index,
            Some(50.0),
            Some(50.0),
            5.0,
        );
        assert_eq!(j.verdict, Verdict::Insufficient);
        assert!(j.basis.contains("指数"), "{}", j.basis);
        let mut no_n = tb();
        no_n.n = None;
        no_n.n_unit = None;
        let j = capability("x", &no_n, Some(0.5), Some(0.5), 5.0);
        assert!(j.basis.contains("課題数"), "{}", j.basis);
    }

    #[test]
    fn speed_is_a_symmetric_ratio_and_ttft_direction_is_flipped() {
        let tps = |c| {
            speed(
                SpeedMetric::OutputTokensPerSecond,
                Some(100.0),
                Some(c),
                0.2,
            )
            .verdict
        };
        assert_eq!(tps(100.0), Verdict::Equivalent);
        assert_eq!(tps(120.0), Verdict::Equivalent);
        assert_eq!(tps(121.0), Verdict::Stronger);
        assert_eq!(tps(84.0), Verdict::Equivalent, "1/1.2 = 0.833 の内側");
        assert_eq!(tps(80.0), Verdict::Weaker);
        let ttft = |c| speed(SpeedMetric::TimeToFirstToken, Some(10.0), Some(c), 0.2).verdict;
        assert_eq!(ttft(10.0), Verdict::Equivalent);
        assert_eq!(ttft(5.0), Verdict::Stronger, "待ちが短いのは良い");
        assert_eq!(ttft(466.0), Verdict::Weaker, "待ちが長いのは弱い");
        assert_eq!(
            speed(SpeedMetric::TimeToFirstToken, Some(10.0), None, 0.2).verdict,
            Verdict::Insufficient
        );
        assert_eq!(
            speed(SpeedMetric::TimeToFirstToken, Some(0.0), Some(1.0), 0.2).verdict,
            Verdict::Insufficient
        );
    }

    #[test]
    fn combine_follows_the_worst_first_rule() {
        use Verdict::*;
        assert_eq!(combine(&[]), Insufficient);
        assert_eq!(combine(&[Equivalent, Weaker, Insufficient]), Weaker);
        assert_eq!(combine(&[Equivalent, Insufficient]), Insufficient);
        assert_eq!(combine(&[Equivalent, Stronger]), Equivalent);
        assert_eq!(combine(&[Stronger, Stronger]), Stronger);
        assert_eq!(combine(&[Equivalent, Equivalent]), Equivalent);
    }

    fn lens_with(counts: &[(&str, u64)]) -> Lens {
        let counts: BTreeMap<String, u64> =
            counts.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
        Lens {
            generated: "2026-10-27".into(),
            days: 365,
            total: counts.values().sum(),
            counts,
        }
    }

    #[test]
    fn execute_role_follows_history_and_plan_role_does_not() {
        let c = data();
        let r = c.measurement("claude-opus-5-5", "high").unwrap();
        let k = c.measurement("gpt-6.1-sol", "high").unwrap();
        let m = Margins::from_catalog(c);
        // 文書だけの履歴: 実行役の判定ベンチは、文書が対応する lcr だが、実行役の対象(TB・SciCode)に無い。
        let docs_only = lens_with(&[("docs", 10)]);
        // 基準と同じ行を候補にして、速さが必ず同等になる状態で、能力の判定が無いことだけを見る。
        let exec = judge_role(c, "execute", r, r, Some(&docs_only), m).unwrap();
        assert!(exec.note.is_some());
        assert_eq!(
            exec.verdict,
            Verdict::Insufficient,
            "能力を判定できないなら、速さが同等でも同等と言わない"
        );
        assert_eq!(exec.judgements.len(), 2, "速さ 2 本だけ");
        // Plan 役は履歴に従わず、lcr と hle を常に判定する。
        let plan = judge_role(c, "plan", r, k, Some(&docs_only), m).unwrap();
        let ids: Vec<_> = plan.judgements.iter().map(|j| j.metric.as_str()).collect();
        assert_eq!(&ids[..2], ["lcr", "hle"]);
        assert_eq!(
            plan.references[0].benchmark,
            "artificial_analysis_intelligence_index"
        );
    }

    #[test]
    fn history_selects_benchmarks_marks_fit_and_reports_uncovered_shares() {
        let c = data();
        let r = c.measurement("claude-opus-5-5", "high").unwrap();
        let k = c.measurement("gpt-6.1-sol", "high").unwrap();
        let m = Margins::from_catalog(c);
        // 60% 機能追加(代理)・30% インフラ(直接)・10% その他(ベンチ無し、しきい値ちょうど)。
        let lens = lens_with(&[("feature", 60), ("config", 30), ("other", 10)]);
        let exec = judge_role(c, "execute", r, k, Some(&lens), m).unwrap();
        let by: BTreeMap<_, _> = exec
            .judgements
            .iter()
            .map(|j| (j.metric.as_str(), j.fit))
            .collect();
        assert_eq!(
            by["terminalbench_v4_0"],
            Some("direct"),
            "インフラが直接測る"
        );
        assert_eq!(by["scicode"], Some("proxy"));
        assert_eq!(exec.uncovered.len(), 1);
        assert_eq!(exec.uncovered[0].category, "other");
        // 履歴が無ければ、役の全ベンチを判定する。
        let none = judge_role(c, "execute", r, k, None, m).unwrap();
        assert_eq!(none.judgements.len(), 4);
    }

    #[test]
    fn compare_lists_every_other_effort_pair_and_rejects_unknown_reference() {
        let c = data();
        let m = Margins::from_catalog(c);
        let all = compare(c, "plan", ("claude-opus-5-5", "high"), None, m).unwrap();
        assert_eq!(all.len(), c.measurements.len() - 1);
        assert!(all
            .iter()
            .all(|x| !(x.model == "claude-opus-5-5" && x.effort == "high")));
        assert!(compare(c, "plan", ("nope", "high"), None, m)
            .unwrap_err()
            .contains("測定値が無い"));
        assert!(compare(c, "nope", ("claude-opus-5-5", "high"), None, m)
            .unwrap_err()
            .contains("未知の役"));
    }

    #[test]
    fn real_catalog_gives_the_expected_headline_for_a_known_pair() {
        // Opus 5.5 high と GPT-6.1 Sol high の最初のトークンまでの時間は 13.0 秒と 24.2 秒(AA、2026-10-27)。
        // 待ちは 1.86 倍で、±20% に入らない。
        let c = data();
        let r = c.measurement("claude-opus-5-5", "high").unwrap();
        let k = c.measurement("gpt-6.1-sol", "high").unwrap();
        let plan = judge_role(c, "plan", r, k, None, Margins::from_catalog(c)).unwrap();
        let ttft = plan
            .judgements
            .iter()
            .find(|j| j.metric == "median_time_to_first_token_seconds")
            .unwrap();
        assert_eq!(ttft.verdict, Verdict::Weaker, "{}", ttft.basis);
        assert_eq!(plan.verdict, Verdict::Weaker);
    }
}
