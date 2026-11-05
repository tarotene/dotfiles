//! 日次のずれ検出の純粋関数。宣言した層と、実測の変化の速さのずれを見つける。
//!
//! 実測は、直近 [`WINDOW_DAYS`] 日に、そのファイルを書き換えたコミットがある日の数(`git log --name-only`)。
//! 層は「記録の種類」に当てるもので、コミット数ではない(ADR-0000 record-layers)。ここで見るのは、
//! 宣言と矛盾する強い兆候だけ: L1 と宣言したのに、窓の大半の日に書き換わっている、など。
//! 拒否(Write の hook)は Edit と Bash の書き込みを見ないので、それらはこの検出が拾う。

use crate::{Declaration, Layer, Lookup};
use std::collections::{BTreeMap, BTreeSet};

/// 実測の窓(日)。
pub const WINDOW_DAYS: usize = 7;

/// 層ごとの、窓の中で書き換わってよい日数の上限。この日数**以上**書き換わっていればずれ。
/// L3・L4 は Git に置かない層なので、速さを測らない(Git にあること自体は移行の対象で、別の扱い)。
pub fn drift_threshold(layer: Layer) -> Option<usize> {
    match layer {
        // 追記のみの記録が、窓の中の 3 日で書き換わるのは、追記ではない。
        Layer::L0 => Some(3),
        // 月〜年の記録が、窓の 7 日のうち 5 日で書き換わるのは、日〜都度の記録である兆候。
        Layer::L1 => Some(5),
        // 週の記録が、毎日書き換わるなら、日の層である。
        Layer::L2 => Some(WINDOW_DAYS),
        Layer::L3 | Layer::L4 => None,
    }
}

/// ずれ 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub path: String,
    pub layer: Layer,
    /// 窓の中で書き換わった日の数。
    pub days: usize,
}

/// `git log --name-only --format=@%cs` の出力から、パスごとに「書き換わった日」の数を数える。
/// `@<日付>` の行が日を切り替え、それ以外の空でない行がパス。
pub fn parse_touch_log(text: &str) -> BTreeMap<String, usize> {
    let mut days: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut day: Option<&str> = None;
    for line in text.lines() {
        if let Some(d) = line.strip_prefix('@') {
            day = Some(d.trim());
        } else if !line.trim().is_empty() {
            if let Some(d) = day {
                days.entry(line.trim().to_string())
                    .or_default()
                    .insert(d.to_string());
            }
        }
    }
    days.into_iter().map(|(p, d)| (p, d.len())).collect()
}

/// 宣言した層と実測のずれ(パスの昇順)。1 つの glob にだけ当たるパスだけを見る
/// (重なり・漏れは宣言の欠陥で、別に報告する)。
pub fn detect_layer_drift(decl: &Declaration, touch_days: &BTreeMap<String, usize>) -> Vec<Drift> {
    touch_days
        .iter()
        .filter_map(|(path, &days)| {
            let Lookup::One(layer) = decl.lookup(path) else {
                return None;
            };
            let limit = drift_threshold(layer)?;
            (days >= limit).then(|| Drift {
                path: path.clone(),
                layer,
                days,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl() -> Declaration {
        Declaration::parse(
            r#"
[[path]]
glob = "adr/**"
layer = "L0"
[[path]]
glob = "src/**"
layer = "L1"
[[path]]
glob = "plan/**"
layer = "L2"
[[path]]
glob = "gen/**"
layer = "L4"
"#,
        )
        .unwrap()
    }

    #[test]
    fn touch_days_count_distinct_days_per_path() {
        let log =
            "@2026-11-05\nsrc/a.rs\nsrc/b.rs\n\n@2026-11-05\nsrc/a.rs\n\n@2026-11-04\nsrc/a.rs\n";
        let m = parse_touch_log(log);
        assert_eq!(m["src/a.rs"], 2, "同じ日の 2 コミットは 1 日");
        assert_eq!(m["src/b.rs"], 1);
    }

    #[test]
    fn a_path_before_any_day_marker_is_ignored() {
        assert!(parse_touch_log("src/a.rs\n").is_empty());
    }

    fn touch(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries.iter().map(|(p, d)| (p.to_string(), *d)).collect()
    }

    #[test]
    fn an_l1_file_rewritten_almost_daily_is_drift() {
        let d = detect_layer_drift(&decl(), &touch(&[("src/a.rs", 5), ("src/b.rs", 4)]));
        assert_eq!(
            d,
            vec![Drift {
                path: "src/a.rs".into(),
                layer: Layer::L1,
                days: 5
            }]
        );
    }

    #[test]
    fn thresholds_follow_the_layer() {
        let d = detect_layer_drift(
            &decl(),
            &touch(&[("adr/1.md", 3), ("plan/w.md", 6), ("plan/x.md", 7)]),
        );
        let paths: Vec<&str> = d.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, vec!["adr/1.md", "plan/x.md"]);
    }

    #[test]
    fn layers_outside_git_and_unmatched_paths_are_not_measured() {
        let d = detect_layer_drift(&decl(), &touch(&[("gen/x.json", 7), ("other/y", 7)]));
        assert!(d.is_empty());
    }

    #[test]
    fn a_path_hit_by_two_globs_is_not_measured() {
        let d = Declaration::parse(
            "[[path]]\nglob = \"a/**\"\nlayer = \"L1\"\n[[path]]\nglob = \"a/b\"\nlayer = \"L0\"\n",
        )
        .unwrap();
        assert!(detect_layer_drift(&d, &touch(&[("a/b", 7)])).is_empty());
    }

    #[test]
    fn the_window_matches_the_l2_threshold() {
        assert_eq!(drift_threshold(Layer::L2), Some(WINDOW_DAYS));
        assert_eq!(drift_threshold(Layer::L3), None);
    }
}
