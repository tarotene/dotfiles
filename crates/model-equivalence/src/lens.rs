//! 過去の課題(マージ済み PR)の内訳。PR タイトルの conventional prefix だけで種類を決める
//! (決定論的で、LLM を使わない)。出力は種類ごとの件数と期間だけで、タイトルもリポジトリ名も残さない。

use model_catalog::Catalog;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `feat(scope)!: …` の prefix から種類を決める。prefix が無い・未知のものは `other`。
/// 返す ID は、`catalog.toml` の `[category.*]` に必ずある(テストで縛っている)。
pub fn classify(title: &str) -> &'static str {
    let head = title.split(':').next().unwrap_or("");
    if head.len() == title.len() {
        return "other"; // ':' が無い
    }
    let kind = head
        .split('(')
        .next()
        .unwrap_or("")
        .trim_end_matches('!')
        .trim();
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
        return "other";
    }
    match kind.to_ascii_lowercase().as_str() {
        "feat" | "feature" => "feature",
        "fix" | "bugfix" | "hotfix" => "bugfix",
        "refactor" | "perf" => "refactor",
        "docs" | "doc" => "docs",
        "ci" | "build" => "config",
        _ => "other",
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lens {
    /// 集計した日(UTC)。
    pub generated: String,
    /// 何日分のマージ済み PR か。
    pub days: u32,
    pub total: u64,
    /// 種類 ID → 件数。
    pub counts: BTreeMap<String, u64>,
}

impl Lens {
    pub fn build<'a>(
        titles: impl IntoIterator<Item = &'a str>,
        generated: &str,
        days: u32,
    ) -> Lens {
        let mut counts = BTreeMap::new();
        let mut total = 0;
        for t in titles {
            *counts.entry(classify(t).to_owned()).or_insert(0) += 1;
            total += 1;
        }
        Lens {
            generated: generated.to_owned(),
            days,
            total,
            counts,
        }
    }

    pub fn share(&self, category: &str) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.counts.get(category).copied().unwrap_or(0) as f64 / self.total as f64
    }

    /// 件数の合計が合い、未知の種類が無いこと。
    pub fn validate(&self, catalog: &Catalog) -> Result<(), String> {
        let sum: u64 = self.counts.values().sum();
        if sum != self.total {
            return Err(format!(
                "件数の合計 {sum} が total {} と合わない",
                self.total
            ));
        }
        for id in self.counts.keys() {
            if !catalog.categories.contains_key(id) {
                return Err(format!("未知の種類: {id}"));
            }
        }
        Ok(())
    }

    pub fn from_json(text: &str, catalog: &Catalog) -> Result<Lens, String> {
        let lens: Lens = serde_json::from_str(text).map_err(|e| format!("lens.json: {e}"))?;
        lens.validate(catalog)?;
        Ok(lens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_catalog::data;

    #[test]
    fn classifies_conventional_prefixes() {
        let cases = [
            ("feat(model-catalog): add a catalog", "feature"),
            ("feat: x", "feature"),
            ("fix(wrapup): defer unchanged rows", "bugfix"),
            ("fix!: breaking", "bugfix"),
            ("Fix(Scope): upper case", "bugfix"),
            ("refactor(core): split", "refactor"),
            ("perf: faster", "refactor"),
            ("docs(adr): note", "docs"),
            ("ci: pin action", "config"),
            ("build(deps): bump", "config"),
            ("chore: tidy", "other"),
            ("test: add", "other"),
            ("revert: x", "other"),
            ("no prefix at all", "other"),
            ("Add a thing: with a colon later", "other"),
            (
                "feat(a:b): a colon inside the scope still reads the prefix",
                "feature",
            ),
            ("", "other"),
            (": empty kind", "other"),
        ];
        for (title, want) in cases {
            assert_eq!(classify(title), want, "{title}");
        }
    }

    #[test]
    fn every_class_id_exists_in_the_catalog() {
        let c = data();
        for title in ["feat: a", "fix: a", "refactor: a", "docs: a", "ci: a", "x"] {
            assert!(c.categories.contains_key(classify(title)), "{title}");
        }
    }

    #[test]
    fn build_counts_shares_and_keeps_no_titles() {
        let lens = Lens::build(["feat: a", "feat: b", "fix: c", "wip"], "2026-10-27", 365);
        assert_eq!(lens.total, 4);
        assert_eq!(lens.counts["feature"], 2);
        assert!((lens.share("feature") - 0.5).abs() < 1e-12);
        assert_eq!(lens.share("docs"), 0.0);
        let json = serde_json::to_string(&lens).unwrap();
        assert!(!json.contains("feat: a") && !json.contains("wip"), "{json}");
        assert_eq!(Lens::from_json(&json, data()).unwrap(), lens);
    }

    #[test]
    fn empty_lens_has_zero_shares() {
        let lens = Lens::build([], "2026-10-27", 30);
        assert_eq!(lens.total, 0);
        assert_eq!(lens.share("feature"), 0.0);
    }

    #[test]
    fn validate_rejects_bad_sums_and_unknown_categories() {
        let mut lens = Lens::build(["feat: a"], "2026-10-27", 30);
        lens.total = 2;
        assert!(lens.validate(data()).unwrap_err().contains("合わない"));
        let mut lens = Lens::build(["feat: a"], "2026-10-27", 30);
        lens.counts.insert("surprise".into(), 0);
        assert!(lens.validate(data()).unwrap_err().contains("未知の種類"));
        assert!(Lens::from_json("{\"generated\":\"x\"}", data()).is_err());
    }
}
