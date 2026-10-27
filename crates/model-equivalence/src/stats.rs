//! 二項割合の差の区間。
//!
//! 同等性検定(TOST、Lakens 2017)は、差の `1-2α` 区間が許容幅 ±δ の内側に収まることを見る。
//! 区間は Newcombe(1998)の、2 つの Wilson 区間を組み合わせる方法(論文の method 10)で作る。
//! 公開ベンチは課題数が少なく(例: Terminal-Bench 4.0 は 66 課題)、割合が 0 や 1 に寄ることもあるので、
//! 近似の粗い Wald 区間は使わない。
//!
//! 2 つのモデルは同じ課題で測られている(対応のあるデータ)が、ここでは独立な 2 標本として扱う。
//! 対応を無視すると区間は広めに出るので、「同等」を言い過ぎない側(保守的)に倒れる。
//!
//! - Newcombe, R. G., "Interval estimation for the difference between independent proportions:
//!   comparison of eleven methods", Statistics in Medicine 17(8):873-890, 1998.
//! - Lakens, D., "Equivalence Tests: A Practical Primer for t Tests, Correlations, and
//!   Meta-Analyses", Social Psychological and Personality Science, 2017.

/// 標準正規分布の上側 5% 点。片側 α=0.05 の TOST に使う 90% 区間の臨界値。
/// α は固定で、設定にしない(許容幅 δ だけをページで動かす)。
pub const Z90: f64 = 1.644_853_626_951_472_2;

/// Wilson のスコア区間。`p` は割合、`n` は標本数。
pub fn wilson(p: f64, n: f64, z: f64) -> (f64, f64) {
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    ((center - half).max(0.0), (center + half).min(1.0))
}

/// `p1 - p2` の区間(Newcombe の method 10)。`(下限, 上限)`。
pub fn newcombe_diff(p1: f64, n1: f64, p2: f64, n2: f64, z: f64) -> (f64, f64) {
    let (l1, u1) = wilson(p1, n1, z);
    let (l2, u2) = wilson(p2, n2, z);
    let d = p1 - p2;
    let lower = d - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt();
    let upper = d + ((u1 - p1).powi(2) + (p2 - l2).powi(2)).sqrt();
    (lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b} (tol {tol})");
    }

    #[test]
    fn wilson_matches_the_widely_quoted_50_of_100_interval() {
        // 95% の Wilson 区間: 50/100 は (0.4038, 0.5962) と広く引用される。
        let (lo, hi) = wilson(0.5, 100.0, 1.959_963_984_540_054);
        close(lo, 0.4038, 5e-5);
        close(hi, 0.5962, 5e-5);
    }

    #[test]
    fn wilson_stays_inside_the_unit_interval_at_the_edges() {
        for (p, n) in [(0.0, 10.0), (1.0, 10.0), (0.0, 66.0), (1.0, 66.0)] {
            let (lo, hi) = wilson(p, n, Z90);
            assert!(
                (0.0..=1.0).contains(&lo) && (0.0..=1.0).contains(&hi),
                "{p} {n}"
            );
            assert!(lo <= p && p <= hi);
        }
        // 0/10 でも上限は 0 にならない(Wald 区間のような幅 0 に潰れない)。
        assert!(wilson(0.0, 10.0, Z90).1 > 0.1);
    }

    #[test]
    fn identical_samples_give_a_symmetric_interval_around_zero() {
        // 同じ割合・同じ n なら、区間は 0 を中心に対称。幅は Wilson の半幅 h の √2 倍。
        let (lo, hi) = newcombe_diff(0.5, 100.0, 0.5, 100.0, 1.959_963_984_540_054);
        close(lo, -hi, 1e-12);
        close(hi, (0.5962f64 - 0.5).hypot(0.5 - 0.4038), 2e-4);
    }

    #[test]
    fn newcombe_matches_the_papers_first_example() {
        // 56/70 − 48/80 の 95% 区間(method 10)は (0.0524, 0.3339)。Newcombe 1998 の例として知られる値だが、
        // 論文の表そのものは未読で、実装の前に控えていた値との一致を確かめたもの(出典の表での照合は未実施)。
        let (lo, hi) = newcombe_diff(56.0 / 70.0, 70.0, 48.0 / 80.0, 80.0, 1.959_963_984_540_054);
        close(lo, 0.0524, 5e-4);
        close(hi, 0.3339, 5e-4);
    }

    #[test]
    fn interval_contains_the_point_difference_and_shrinks_with_n() {
        let (lo1, hi1) = newcombe_diff(0.6, 66.0, 0.5, 66.0, Z90);
        assert!(lo1 < 0.1 && 0.1 < hi1);
        let (lo2, hi2) = newcombe_diff(0.6, 660.0, 0.5, 660.0, Z90);
        assert!(hi2 - lo2 < hi1 - lo1);
    }
}
