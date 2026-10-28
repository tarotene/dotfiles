//! 近さの地図。モデル同士の相対的な近さを距離で定義し、古典的 MDS で 2 次元に落とす。
//!
//! 判定(`judge`)は「基準 1 点に対する閾値判定」で、基準を選ばないと全体の配置が見えない。ここは、
//! 判定が持つ区間と分解能の考え方を流用して距離を定義し、(モデル × エフォート)の配置を地図にする。
//! 総合点は作らない(ベンチごとの尺度も誤差も違い、どのベンチで負けているかが消える)ので、距離は
//! 成分ごとの差を尺度で割った値の最大(L∞)で、役の束ね(1 つでも外れたら弱い)に合わせている。
//!
//! 距離(成分 = 役の判定に使うベンチ + 出力速度 + 最初のトークンまでの時間):
//!
//! - 能力のベンチ: `|差(点)| / max(許容幅δ, そのベンチの分解能)`。分解能は、2 点の中点の点数で、
//!   課題数 `n` のときの差の 90% 区間の半幅(`judge::resolution_points`)。基準側の点数で測る判定の
//!   分解能と違い、2 点に対して対称にするため中点を使う。課題数の無いベンチ・割合でない指数は、尺度が δ。
//! - 速さ: `|ln(候補 / 基準)| / ln(1 + 許容比)`。出力速度も待ちも、比の向きに依らず同じ値になる。
//! - 値の欠けた成分は距離から外す。両方に値のある成分が [`MIN_COMMON`] 本未満の組は、距離を出さない。
//!
//! 2 次元への射影は古典的 MDS(距離行列の二重中心化 → 上位 2 固有値・固有ベクトル)。外部の線形代数
//! crate は使わず、固有ベクトルは直交化つきの累乗法で求める。距離を出せない組は、距離のある組だけを辺と
//! みなした最短経路で補う(補った組の数は開示し、stress は直接の距離がある組だけで測る)。
//!
//! 先行例は `docs/model-equivalence.md` の「近さの地図」の節に書く。MDS の原典は未取得。

use crate::judge::{resolution_points, Margins, SpeedMetric};
use model_catalog::{Catalog, Measurement, Unit};
use serde_json::{json, Value};

/// 両方に値のある成分がこの本数に満たない組は、距離を出さない(判定の根拠が薄すぎる)。
pub const MIN_COMMON: usize = 3;

/// stress がこれを超えたら、ページは地図を信用しないよう警告する。このページ独自の目安
/// (Kruskal 1964 の解釈の表は未取得で、そこから採った値ではない)。
pub const STRESS_WARN: f64 = 0.2;

#[derive(Clone, Debug, PartialEq)]
pub enum AxisKind {
    /// 能力のベンチ。値は点(割合は 100 倍)で持つ。課題数 `n` があれば尺度に分解能を使う。
    Capability { n: Option<f64>, fraction: bool },
    /// 速さ。値は対数で持つ。
    Speed(SpeedMetric),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub id: String,
    pub label: String,
    pub kind: AxisKind,
}

/// 役の判定に使うベンチ(`judged`)と、2 つの速さから成分を作る。
pub fn axes(catalog: &Catalog, judged: &[String]) -> Result<Vec<Axis>, String> {
    let mut out = Vec::new();
    for id in judged {
        let b = catalog
            .benchmarks
            .get(id)
            .ok_or_else(|| format!("未定義のベンチ: {id}"))?;
        let fraction = b.unit == Unit::Fraction;
        out.push(Axis {
            id: id.clone(),
            label: b.name.clone(),
            kind: AxisKind::Capability {
                n: b.n.filter(|_| fraction).map(f64::from),
                fraction,
            },
        });
    }
    for metric in [
        SpeedMetric::OutputTokensPerSecond,
        SpeedMetric::TimeToFirstToken,
    ] {
        out.push(Axis {
            id: metric.id().to_owned(),
            label: metric.label().to_owned(),
            kind: AxisKind::Speed(metric),
        });
    }
    Ok(out)
}

/// 測定値 1 行を、成分ごとの値(点・対数)にする。無い・使えない値は `None`。
pub fn values(axes: &[Axis], m: &Measurement) -> Vec<Option<f64>> {
    axes.iter()
        .map(|a| match &a.kind {
            AxisKind::Capability { fraction, .. } => {
                m.scores
                    .get(&a.id)
                    .map(|v| if *fraction { v * 100.0 } else { *v })
            }
            AxisKind::Speed(metric) => metric.value(m).filter(|v| *v > 0.0).map(f64::ln),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    /// 共通の成分が [`MIN_COMMON`] 本以上のときだけ値がある。
    pub value: Option<f64>,
    /// 両方に値のある成分の本数。
    pub common: usize,
}

/// 2 行の距離(L∞)。対称で、自分自身との距離は 0。
pub fn distance(
    axes: &[Axis],
    a: &[Option<f64>],
    b: &[Option<f64>],
    margins: Margins,
    min_common: usize,
) -> Distance {
    let mut common = 0;
    let mut max = 0.0_f64;
    for (i, axis) in axes.iter().enumerate() {
        let (Some(x), Some(y)) = (a[i], b[i]) else {
            continue;
        };
        common += 1;
        let unit = match &axis.kind {
            AxisKind::Capability { n: Some(n), .. } => {
                let mid = ((x + y) / 200.0).clamp(0.0, 1.0);
                margins.capability_points.max(resolution_points(mid, *n))
            }
            AxisKind::Capability { n: None, .. } => margins.capability_points,
            AxisKind::Speed(_) => (1.0 + margins.speed_ratio).ln(),
        };
        max = max.max((x - y).abs() / unit);
    }
    Distance {
        value: (common >= min_common).then_some(max),
        common,
    }
}

/// 対称な距離行列(n×n、行優先)を、二重中心化して上位 2 固有値・固有ベクトルで 2 次元にする。
#[derive(Clone, Debug, PartialEq)]
pub struct Mds {
    pub coords: Vec<[f64; 2]>,
    pub eigenvalues: [f64; 2],
}

const POWER_MAX_ITER: usize = 4000;
const POWER_TOL: f64 = 1e-10;

/// 決定的な擬似乱数(線形合同法)。累乗法の初期ベクトルに使う。
fn seed_vector(n: usize, salt: u64) -> Vec<f64> {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15 ^ salt.wrapping_mul(0xD1B5_4A32_D192_ED03);
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((s >> 11) as f64) / ((1u64 << 53) as f64) - 0.5
        })
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize(v: &mut [f64]) -> f64 {
    let norm = dot(v, v).sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    norm
}

fn mat_vec(b: &[f64], n: usize, v: &[f64], out: &mut [f64]) {
    for i in 0..n {
        out[i] = dot(&b[i * n..(i + 1) * n], v);
    }
}

/// `against`(単位ベクトル)に直交する部分空間での、最大固有値(代数的に最大)と固有ベクトル。
/// 絶対値が最大の固有値が負のときは、`B + sI` が半正定値になる `s` で累乗法をやり直す。
fn top_eigenpair(b: &[f64], n: usize, against: Option<&[f64]>, salt: u64) -> (f64, Vec<f64>) {
    // 収束は、残差が行列の大きさ(Frobenius ノルム)に対して十分小さいこと。固有値が 0 に近い軸では、
    // 直交化の丸め誤差が固有値 0 の方向を押しのけるので、固有値の大きさに対する相対では判定しない。
    let norm = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    let run = |shift: f64| -> (f64, Vec<f64>) {
        let mut v = seed_vector(n, salt);
        let project = |v: &mut [f64]| {
            if let Some(u) = against {
                let c = dot(v, u);
                v.iter_mut().zip(u).for_each(|(x, y)| *x -= c * y);
            }
        };
        project(&mut v);
        normalize(&mut v);
        let mut w = vec![0.0; n];
        let mut lambda = 0.0;
        for _ in 0..POWER_MAX_ITER {
            mat_vec(b, n, &v, &mut w);
            lambda = dot(&v, &w);
            let resid: f64 = w
                .iter()
                .zip(&v)
                .map(|(wi, vi)| (wi - lambda * vi).powi(2))
                .sum::<f64>()
                .sqrt();
            if resid <= POWER_TOL * norm {
                break;
            }
            for (wi, vi) in w.iter_mut().zip(&v) {
                *wi += shift * vi;
            }
            project(&mut w);
            if normalize(&mut w) == 0.0 {
                break;
            }
            std::mem::swap(&mut v, &mut w);
        }
        (lambda, v)
    };
    let (lambda, v) = run(0.0);
    if lambda >= 0.0 {
        return (lambda, v);
    }
    // 絶対値最大の固有値が負: 全固有値を非負にずらして、代数的に最大の固有ベクトルを求め直す。
    let (_, v) = run(-lambda * (1.0 + 1e-9));
    let mut w = vec![0.0; n];
    mat_vec(b, n, &v, &mut w);
    (dot(&v, &w), v)
}

/// 固有ベクトルの符号の規則: 絶対値が最大の成分を正にする(同じ大きさなら添字の小さい方)。
fn fix_sign(v: &mut [f64]) {
    let max = v.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if let Some(i) = v.iter().position(|x| x.abs() >= max * (1.0 - 1e-9)) {
        if v[i] < 0.0 {
            v.iter_mut().for_each(|x| *x = -*x);
        }
    }
}

pub fn classical_mds(d: &[f64], n: usize) -> Mds {
    assert_eq!(d.len(), n * n, "距離行列は n×n");
    match n {
        0 => {
            return Mds {
                coords: vec![],
                eigenvalues: [0.0; 2],
            }
        }
        1 => {
            return Mds {
                coords: vec![[0.0; 2]],
                eigenvalues: [0.0; 2],
            }
        }
        _ => {}
    }
    let sq: Vec<f64> = d.iter().map(|x| x * x).collect();
    let row_mean: Vec<f64> = (0..n)
        .map(|i| sq[i * n..(i + 1) * n].iter().sum::<f64>() / n as f64)
        .collect();
    let grand = row_mean.iter().sum::<f64>() / n as f64;
    let mut b = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..n {
            b[i * n + j] = -0.5 * (sq[i * n + j] - row_mean[i] - row_mean[j] + grand);
        }
    }
    let scale = b.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    let mut coords = vec![[0.0; 2]; n];
    let mut eigenvalues = [0.0; 2];
    if scale == 0.0 {
        return Mds {
            coords,
            eigenvalues,
        };
    }
    let (l1, mut v1) = top_eigenpair(&b, n, None, 1);
    fix_sign(&mut v1);
    let (l2, mut v2) = top_eigenpair(&b, n, Some(&v1), 2);
    fix_sign(&mut v2);
    for (axis, (l, v)) in [(l1, &v1), (l2, &v2)].into_iter().enumerate() {
        // 固有値が (相対的に) 0 以下の軸は、その方向に広がりが無いので座標を 0 にする。
        if l > scale * 1e-9 {
            eigenvalues[axis] = l;
            let s = l.sqrt();
            for i in 0..n {
                coords[i][axis] = s * v[i];
            }
        }
    }
    Mds {
        coords,
        eigenvalues,
    }
}

/// Kruskal の stress-1: `sqrt(Σ(d − d̂)² / Σ d²)`。`d` は与えた距離、`d̂` は 2 次元上のユークリッド距離。
/// `d` が `None` の組(距離を出せない組)は数えない。0 に近いほど、地図が距離を保っている。
pub fn stress1(d: &[Option<f64>], n: usize, coords: &[[f64; 2]]) -> f64 {
    let (mut num, mut den) = (0.0, 0.0);
    for i in 0..n {
        for j in (i + 1)..n {
            let Some(dij) = d[i * n + j] else { continue };
            let e = (coords[i][0] - coords[j][0]).hypot(coords[i][1] - coords[j][1]);
            num += (dij - e).powi(2);
            den += dij * dij;
        }
    }
    if den > 0.0 {
        (num / den).sqrt()
    } else {
        0.0
    }
}

/// 1 つの尺度(許容幅の組)での地図。
#[derive(Clone, Debug, PartialEq)]
pub struct MapSet {
    pub margins: Margins,
    /// 行ごとの座標(`values` の行の並び)。成分が [`MIN_COMMON`] 本に満たない行や、ほかの行と
    /// つながらない行は載せず `None`。
    pub points: Vec<Option<[f64; 2]>>,
    pub stress: f64,
    pub eigenvalues: [f64; 2],
    /// 載せなかった行の数。
    pub excluded: usize,
    /// 載せた行どうしの組のうち、距離を直接出せた数。
    pub direct_pairs: usize,
    /// 載せた行どうしの組のうち、共通の成分が [`MIN_COMMON`] 本に満たず、最短経路で補った数。
    pub filled_pairs: usize,
}

pub fn build_set(
    axes: &[Axis],
    rows: &[Vec<Option<f64>>],
    margins: Margins,
    min_common: usize,
) -> MapSet {
    let n = rows.len();
    let eligible: Vec<usize> = (0..n)
        .filter(|&i| rows[i].iter().flatten().count() >= min_common)
        .collect();
    // 距離を直接出せる組を辺とみなし、最大の連結成分だけを載せる(同数なら添字の小さい方を含む側)。
    let m0 = eligible.len();
    let mut direct = vec![None; m0 * m0];
    for a in 0..m0 {
        for b in (a + 1)..m0 {
            let d = distance(
                axes,
                &rows[eligible[a]],
                &rows[eligible[b]],
                margins,
                min_common,
            )
            .value;
            direct[a * m0 + b] = d;
            direct[b * m0 + a] = d;
        }
    }
    let mut comp = vec![usize::MAX; m0];
    let mut sizes: Vec<usize> = Vec::new();
    for s in 0..m0 {
        if comp[s] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut stack = vec![s];
        comp[s] = id;
        let mut size = 0;
        while let Some(u) = stack.pop() {
            size += 1;
            for v in 0..m0 {
                if comp[v] == usize::MAX && direct[u * m0 + v].is_some() {
                    comp[v] = id;
                    stack.push(v);
                }
            }
        }
        sizes.push(size);
    }
    let best = sizes
        .iter()
        .enumerate()
        .fold(None::<(usize, usize)>, |acc, (i, &s)| match acc {
            Some((_, bs)) if bs >= s => acc,
            _ => Some((i, s)),
        })
        .map(|(i, _)| i);
    let kept: Vec<usize> = (0..m0).filter(|&a| Some(comp[a]) == best).collect();
    let m = kept.len();
    let mut dist = vec![None; m * m];
    for (x, &a) in kept.iter().enumerate() {
        for (y, &b) in kept.iter().enumerate() {
            dist[x * m + y] = if x == y {
                Some(0.0)
            } else {
                direct[a * m0 + b]
            };
        }
    }
    let direct_pairs = dist
        .iter()
        .enumerate()
        .filter(|(k, v)| v.is_some() && k / m < k % m)
        .count();
    let total_pairs = m * m.saturating_sub(1) / 2;
    let filled_pairs = total_pairs - direct_pairs;
    // 最短経路で補う(Floyd–Warshall)。直接の距離が全部あるときは何もしない。
    let mut full: Vec<f64> = dist.iter().map(|v| v.unwrap_or(f64::INFINITY)).collect();
    if filled_pairs > 0 {
        for k in 0..m {
            for i in 0..m {
                let dik = full[i * m + k];
                if !dik.is_finite() {
                    continue;
                }
                for j in 0..m {
                    let via = dik + full[k * m + j];
                    if via < full[i * m + j] {
                        full[i * m + j] = via;
                    }
                }
            }
        }
    }
    let mds = classical_mds(&full, m);
    let stress = stress1(&dist, m, &mds.coords);
    let mut points = vec![None; n];
    for (x, &a) in kept.iter().enumerate() {
        points[eligible[a]] = Some(mds.coords[x]);
    }
    MapSet {
        margins,
        points,
        stress,
        eigenvalues: mds.eigenvalues,
        excluded: n - m,
        direct_pairs,
        filled_pairs,
    }
}

fn round(v: f64, decimals: i32) -> f64 {
    let k = 10f64.powi(decimals);
    (v * k).round() / k
}

/// 役ごとの地図(尺度ごと)をページに埋め込む形にする。`dist` は、`reference` の行から全行への距離
/// (尺度ごと。`golden` に使い、ページの JS が自分の計算と突き合わせる)。
pub fn role_json(
    axes: &[Axis],
    rows: &[Vec<Option<f64>>],
    sets: &[Margins],
    reference: usize,
) -> (Value, Vec<Value>) {
    let mut set_json = Vec::new();
    for &margins in sets {
        let s = build_set(axes, rows, margins, MIN_COMMON);
        let pts: Vec<Value> = s
            .points
            .iter()
            .flat_map(|p| match p {
                Some([x, y]) => [json!(round(*x, 3)), json!(round(*y, 3))],
                None => [Value::Null, Value::Null],
            })
            .collect();
        set_json.push(json!({
            "margins": {"capability_points": margins.capability_points, "speed_ratio": margins.speed_ratio},
            "stress": round(s.stress, 4),
            "pts": pts,
            "n_points": rows.len() - s.excluded,
            "excluded": s.excluded,
            "direct_pairs": s.direct_pairs,
            "filled_pairs": s.filled_pairs,
        }));
    }
    // 基準から全行への距離。行ごとに尺度の数だけ並べる(基準自身と、距離を出せない組は null)。
    let mut dist = Vec::with_capacity(rows.len() * sets.len());
    for (i, row) in rows.iter().enumerate() {
        for &margins in sets {
            let d = if i == reference {
                None
            } else {
                distance(axes, &rows[reference], row, margins, MIN_COMMON).value
            };
            dist.push(d.map_or(Value::Null, |v| json!(round(v, 4))));
        }
    }
    (
        json!({
            "axes": axes.iter().map(|a| json!({"id": a.id, "label": a.label})).collect::<Vec<_>>(),
            "sets": set_json,
        }),
        dist,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Margins = Margins {
        capability_points: 5.0,
        speed_ratio: 0.2,
    };

    /// 課題数 100 のベンチ 2 本と、課題数の無い指数 1 本、速さ 1 本。
    fn axes4() -> Vec<Axis> {
        let cap = |id: &str, n: Option<f64>, fraction: bool| Axis {
            id: id.into(),
            label: id.into(),
            kind: AxisKind::Capability { n, fraction },
        };
        vec![
            cap("a", Some(100.0), true),
            cap("b", Some(2000.0), true),
            cap("idx", None, false),
            Axis {
                id: "tps".into(),
                label: "tps".into(),
                kind: AxisKind::Speed(SpeedMetric::OutputTokensPerSecond),
            },
        ]
    }

    fn row(a: Option<f64>, b: Option<f64>, idx: Option<f64>, tps: Option<f64>) -> Vec<Option<f64>> {
        vec![a, b, idx, tps.map(f64::ln)]
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b} (tol {tol})");
    }

    #[test]
    fn distance_is_linf_of_differences_over_max_of_margin_and_resolution() {
        let ax = axes4();
        // a(n=100): 50 → 56 点。分解能は中点 53 点で ±12pt 台なので、尺度は分解能。
        // b(n=2000): 60 → 62 点。分解能は ±2pt 台なので、尺度は許容幅 5。
        // idx(課題数なし): 40 → 50。尺度は許容幅 5 → 2.0。
        // tps: 100 → 150。|ln 1.5| / ln 1.2 ≈ 2.224。
        let x = row(Some(50.0), Some(60.0), Some(40.0), Some(100.0));
        let y = row(Some(56.0), Some(62.0), Some(50.0), Some(150.0));
        let d = distance(&ax, &x, &y, M, MIN_COMMON);
        assert_eq!(d.common, 4);
        let res_a = resolution_points(0.53, 100.0);
        assert!(res_a > 5.0, "a の分解能が許容幅より広いこと: {res_a}");
        let want = [
            6.0 / res_a,
            2.0 / 5.0_f64.max(resolution_points(0.61, 2000.0)),
            10.0 / 5.0,
            1.5_f64.ln() / 1.2_f64.ln(),
        ];
        close(
            d.value.unwrap(),
            want.iter().cloned().fold(0.0, f64::max),
            1e-12,
        );
        close(d.value.unwrap(), want[3], 1e-12);
        assert!((want[3] - 2.2239).abs() < 1e-3);
    }

    #[test]
    fn distance_is_symmetric_zero_on_itself_and_speed_direction_free() {
        let ax = axes4();
        let x = row(Some(50.0), Some(60.0), Some(40.0), Some(100.0));
        let y = row(Some(58.0), Some(55.0), Some(47.0), Some(70.0));
        let dxy = distance(&ax, &x, &y, M, MIN_COMMON).value.unwrap();
        let dyx = distance(&ax, &y, &x, M, MIN_COMMON).value.unwrap();
        assert_eq!(dxy, dyx, "対称");
        assert_eq!(distance(&ax, &x, &x, M, MIN_COMMON).value, Some(0.0));
        // 速さが 2 倍になる向きと 1/2 になる向きで、成分の値は同じ。
        let fast = row(None, None, None, Some(200.0));
        let slow = row(None, None, None, Some(50.0));
        let base = row(Some(1.0), Some(1.0), Some(1.0), Some(100.0));
        let df = distance(&ax, &base, &fast, M, 1).value.unwrap();
        let ds = distance(&ax, &base, &slow, M, 1).value.unwrap();
        close(df, ds, 1e-12);
    }

    #[test]
    fn scale_follows_the_margin_when_it_exceeds_the_resolution() {
        let ax = axes4();
        let x = row(Some(50.0), Some(60.0), Some(40.0), None);
        let y = row(Some(50.0), Some(60.0), Some(50.0), None);
        // 差は idx の 10 点だけ。許容幅を 5 → 20 に広げると、距離は 2 → 0.5。
        let wide = Margins {
            capability_points: 20.0,
            speed_ratio: 0.2,
        };
        close(distance(&ax, &x, &y, M, 3).value.unwrap(), 2.0, 1e-12);
        close(distance(&ax, &x, &y, wide, 3).value.unwrap(), 0.5, 1e-12);
    }

    #[test]
    fn fewer_than_k_common_components_give_no_distance_but_keep_the_count() {
        let ax = axes4();
        let x = row(Some(50.0), None, Some(40.0), Some(100.0));
        let y = row(None, Some(60.0), Some(50.0), Some(100.0));
        let d = distance(&ax, &x, &y, M, 3);
        assert_eq!(d.common, 2);
        assert_eq!(d.value, None);
        // k を下げれば出る。出る値は、共通の 2 成分の最大。
        assert_eq!(distance(&ax, &x, &y, M, 2).value, Some(2.0));
    }

    fn euclid(points: &[[f64; 2]]) -> Vec<f64> {
        let n = points.len();
        let mut d = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                d[i * n + j] = (points[i][0] - points[j][0]).hypot(points[i][1] - points[j][1]);
            }
        }
        d
    }

    fn some(d: &[f64]) -> Vec<Option<f64>> {
        d.iter().map(|x| Some(*x)).collect()
    }

    fn pairwise(coords: &[[f64; 2]]) -> Vec<f64> {
        euclid(coords)
    }

    #[test]
    fn equilateral_triangle_is_recovered_with_zero_stress() {
        let h = 3f64.sqrt() / 2.0;
        let d = euclid(&[[0.0, 0.0], [1.0, 0.0], [0.5, h]]);
        let mds = classical_mds(&d, 3);
        let s = stress1(&some(&d), 3, &mds.coords);
        assert!(s < 1e-6, "stress {s}");
        for (a, b) in pairwise(&mds.coords).iter().zip(&d) {
            close(*a, *b, 1e-6);
        }
        // 正三角形は 2 つの固有値が等しい(縮退)。
        close(mds.eigenvalues[0], mds.eigenvalues[1], 1e-6);
    }

    #[test]
    fn collinear_points_land_on_one_axis_with_zero_stress() {
        let pts = [[0.0, 0.0], [1.0, 0.0], [3.0, 0.0], [6.0, 0.0], [10.0, 0.0]];
        let d = euclid(&pts);
        let mds = classical_mds(&d, 5);
        assert!(stress1(&some(&d), 5, &mds.coords) < 1e-6);
        // 直線上なので、2 つ目の固有値は 0(2 軸目の座標は 0)。
        assert!(mds.eigenvalues[1] < 1e-6, "{:?}", mds.eigenvalues);
        assert!(mds.coords.iter().all(|c| c[1].abs() < 1e-6));
        // 並びの順序と間隔が保たれる。
        let xs: Vec<f64> = mds.coords.iter().map(|c| c[0]).collect();
        let gaps: Vec<f64> = xs.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        for (g, want) in gaps.iter().zip([1.0, 2.0, 3.0, 4.0]) {
            close(*g, want, 1e-6);
        }
        assert!(xs.windows(2).all(|w| w[0] < w[1]) || xs.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn a_square_in_the_plane_is_recovered() {
        let d = euclid(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
        let mds = classical_mds(&d, 4);
        assert!(stress1(&some(&d), 4, &mds.coords) < 1e-6);
    }

    #[test]
    fn three_dimensional_points_get_a_positive_stress() {
        // 正四面体は 2 次元に収まらない。stress は 0 にならず、1 未満。
        let d = vec![
            0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0,
        ];
        let mds = classical_mds(&d, 4);
        let s = stress1(&some(&d), 4, &mds.coords);
        assert!(s > 0.1 && s < 0.5, "stress {s}");
    }

    #[test]
    fn mds_is_deterministic_and_fixes_the_eigenvector_signs() {
        let pts = [
            [0.0, 0.0],
            [4.0, 1.0],
            [1.0, 5.0],
            [-3.0, 2.0],
            [2.0, -4.0],
            [6.0, 6.0],
        ];
        let d = euclid(&pts);
        let a = classical_mds(&d, 6);
        let b = classical_mds(&d, 6);
        assert_eq!(a, b, "同じ入力は同じ座標");
        // 符号の規則: 各軸で、絶対値が最大の座標は正。
        for axis in 0..2 {
            let m = a
                .coords
                .iter()
                .map(|c| c[axis])
                .max_by(|x, y| x.abs().total_cmp(&y.abs()))
                .unwrap();
            assert!(m > 0.0, "軸 {axis}: {m}");
        }
        // 行の並べ替えに依らず、各点の座標は(符号の規則のもとで)一致する、ということは言わない。
        // 言うのは、元の配置を回転・反転した距離行列から同じ形(距離)が得られること。
        for (x, y) in pairwise(&a.coords).iter().zip(&d) {
            close(*x, *y, 1e-6);
        }
    }

    #[test]
    fn non_euclidean_distances_still_give_finite_deterministic_coordinates() {
        // 三角不等式を破る距離(1, 1, 3)。B に負の固有値が出る。
        let d = vec![0.0, 1.0, 3.0, 1.0, 0.0, 1.0, 3.0, 1.0, 0.0];
        let a = classical_mds(&d, 3);
        assert_eq!(a, classical_mds(&d, 3));
        assert!(a.coords.iter().flatten().all(|x| x.is_finite()));
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(classical_mds(&[], 0).coords.is_empty());
        assert_eq!(classical_mds(&[0.0], 1).coords, vec![[0.0, 0.0]]);
        let same = classical_mds(&[0.0; 9], 3);
        assert!(same.coords.iter().flatten().all(|x| *x == 0.0));
        let two = classical_mds(&[0.0, 2.0, 2.0, 0.0], 2);
        close(
            (two.coords[0][0] - two.coords[1][0]).hypot(two.coords[0][1] - two.coords[1][1]),
            2.0,
            1e-9,
        );
    }

    #[test]
    fn build_set_drops_rows_with_too_few_components() {
        let ax = axes4();
        let rows = vec![
            row(Some(50.0), Some(60.0), Some(40.0), Some(100.0)),
            row(Some(55.0), Some(61.0), Some(42.0), Some(120.0)),
            row(Some(60.0), Some(70.0), Some(47.0), Some(90.0)),
            // 成分が 2 本しか無い行は、どの組も距離を出せないので載せない。
            row(None, None, Some(44.0), Some(95.0)),
            row(Some(10.0), None, None, Some(10.0)),
        ];
        let s = build_set(&ax, &rows, M, MIN_COMMON);
        assert_eq!(s.excluded, 2);
        assert!(s.points[3].is_none() && s.points[4].is_none());
        assert_eq!((s.direct_pairs, s.filled_pairs), (3, 0));
        assert!(s.stress.is_finite() && s.stress >= 0.0);
    }

    #[test]
    fn a_pair_without_enough_common_components_is_filled_by_the_shortest_path() {
        let ax = axes4();
        let rows = vec![
            row(Some(50.0), Some(60.0), Some(40.0), Some(100.0)),
            // 行 1 と 2 の共通は a・idx の 2 本: 直接の距離は出せない。行 0 と 3 を経由して補う。
            row(Some(55.0), Some(61.0), Some(42.0), None),
            row(Some(52.0), None, Some(41.0), Some(110.0)),
            row(Some(58.0), Some(64.0), Some(45.0), Some(95.0)),
        ];
        let s = build_set(&ax, &rows, M, MIN_COMMON);
        assert_eq!((s.excluded, s.direct_pairs, s.filled_pairs), (0, 5, 1));
        assert!(s.stress.is_finite());
    }

    #[test]
    fn only_the_largest_connected_group_is_kept() {
        let ax = axes4();
        // 群 A は {a, b, idx}、群 B は {a, idx, tps} だけを持つ。A と B は共通が 2 本で、つながらない。
        let rows = vec![
            row(Some(50.0), Some(60.0), Some(40.0), None),
            row(Some(52.0), Some(61.0), Some(41.0), None),
            row(Some(54.0), Some(62.0), Some(42.0), None),
            row(Some(50.0), None, Some(40.0), Some(100.0)),
            row(Some(51.0), None, Some(41.0), Some(105.0)),
        ];
        let s = build_set(&ax, &rows, M, MIN_COMMON);
        assert_eq!(s.excluded, 2);
        assert!(s.points[3].is_none() && s.points[4].is_none());
        assert!(s.points[..3].iter().all(Option::is_some));
        assert_eq!(s.filled_pairs, 0);
    }
}
