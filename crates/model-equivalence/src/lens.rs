//! 過去の課題(マージ済み PR)の内訳。種類は決定論的に決め(LLM を使わない)、規則は `catalog.toml` の
//! `[category.*]` の宣言が持つ。1 番目の手がかりは PR タイトルの conventional prefix、prefix で決まらなかった
//! PR だけ、2 番目の手がかりとして変更ファイルの名前を取りに行く(PR 1 件ごとに API を 1 回呼ぶので、呼び出しを
//! 最小にする)。出力は種類ごとの件数と期間だけで、タイトルもリポジトリ名もファイル名も残さない。

use model_catalog::{Catalog, OTHER_CATEGORY};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `feat(scope)!: …` の prefix の型(小文字)。prefix が無い・英字だけでないものは None。
fn prefix_kind(title: &str) -> Option<String> {
    let head = title.split(':').next().unwrap_or("");
    if head.len() == title.len() {
        return None; // ':' が無い
    }
    let kind = head
        .split('(')
        .next()
        .unwrap_or("")
        .trim_end_matches('!')
        .trim();
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(kind.to_ascii_lowercase())
}

/// タイトルの prefix だけで決まる種類。prefix が無い、または `catalog.toml` の `[category.*].prefixes`
/// のどれにも無ければ None(変更ファイルなど、次の手がかりに回す)。
pub fn classify_prefix<'c>(catalog: &'c Catalog, title: &str) -> Option<&'c str> {
    catalog.category_for_prefix(&prefix_kind(title)?)
}

/// タイトルだけで種類を決める。決まらなければ `other`。返す ID は `catalog.categories` に必ずある。
pub fn classify<'c>(catalog: &'c Catalog, title: &str) -> &'c str {
    classify_prefix(catalog, title).unwrap_or(OTHER_CATEGORY)
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
    /// うち、prefix で決まらず、変更ファイルで種類が決まった件数(分類の手がかりの内訳)。
    #[serde(default)]
    pub by_files: u64,
    /// 変更ファイルを取りに行って取れなかった件数(`other` に倒した。取得が失敗した・100 件を超えて全体を
    /// 見られなかった)。
    #[serde(default)]
    pub file_failures: u64,
}

/// 検索の 1 行。タイトルとリポジトリ・番号は、変更ファイルを取りに行くために、メモリ上でだけ持つ(書き出さない)。
#[derive(Clone, Debug, PartialEq)]
pub struct PrRow {
    pub title: String,
    /// `owner/name`。検索の応答に無ければ空。
    pub repo: String,
    pub number: u64,
}

/// `gh search prs --json title,number,repository` の応答を読む。タイトルの無い行は読み飛ばす。
pub fn parse_search_rows(json: &str) -> Result<Vec<PrRow>, String> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("gh の出力が JSON でない: {e}"))?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(PrRow {
                title: r.get("title")?.as_str()?.to_owned(),
                repo: r
                    .pointer("/repository/nameWithOwner")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                number: r.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
            })
        })
        .collect())
}

/// 1 回の取得で読むファイル数。これに届いたら、全体を見られていないので種類を決めない。
pub const FILES_PAGE: usize = 100;

/// 変更ファイルを取る REST のパス(`gh api` に渡す)。リポジトリ名・番号が不正なら None。
pub fn files_endpoint(row: &PrRow) -> Option<String> {
    let (owner, name) = row.repo.split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(owner) && ok(name) && row.number > 0).then(|| {
        format!(
            "repos/{owner}/{name}/pulls/{}/files?per_page={FILES_PAGE}",
            row.number
        )
    })
}

/// `gh api … --jq '[.[].filename]'` の出力(ファイル名の JSON 配列)を読む。
pub fn parse_filenames(json: &str) -> Result<Vec<String>, String> {
    serde_json::from_str(json).map_err(|e| format!("ファイル一覧が JSON の配列でない: {e}"))
}

/// 変更ファイルの全部が同じ 1 つの種類に当たるときだけ、その種類。混在・どれにも当たらないファイルがある・
/// ファイルが無いときは None。
pub fn classify_files<'c>(catalog: &'c Catalog, files: &[String]) -> Option<&'c str> {
    let mut found: Option<&str> = None;
    for f in files {
        let id = catalog.category_for_file(f)?;
        match found {
            Some(prev) if prev != id => return None,
            _ => found = Some(id),
        }
    }
    found
}

impl Lens {
    pub fn build<'a>(
        catalog: &Catalog,
        titles: impl IntoIterator<Item = &'a str>,
        generated: &str,
        days: u32,
    ) -> Lens {
        let mut counts = BTreeMap::new();
        let mut total = 0;
        for t in titles {
            *counts.entry(classify(catalog, t).to_owned()).or_insert(0) += 1;
            total += 1;
        }
        Lens {
            generated: generated.to_owned(),
            days,
            total,
            counts,
            by_files: 0,
            file_failures: 0,
        }
    }

    /// タイトルの prefix で決まらなかった PR にだけ、`fetch` で変更ファイルを取って種類を推す。
    /// `fetch` が失敗したとき、または 100 件に届いたときは、その PR を `other` にして続ける(全体は失敗させない)。
    /// 実際の GitHub への呼び出しは `fetch` の側にあり、ここでは注入された関数だけを呼ぶ。
    pub fn build_with_files(
        catalog: &Catalog,
        prs: &[PrRow],
        generated: &str,
        days: u32,
        fetch: &mut dyn FnMut(&PrRow) -> Result<Vec<String>, String>,
    ) -> Lens {
        let mut lens = Lens::build(catalog, [], generated, days);
        for pr in prs {
            let id = match classify_prefix(catalog, &pr.title) {
                Some(id) => id,
                None => match fetch(pr) {
                    Ok(files) if files.len() >= FILES_PAGE => {
                        lens.file_failures += 1;
                        OTHER_CATEGORY
                    }
                    Ok(files) => match classify_files(catalog, &files) {
                        Some(id) => {
                            lens.by_files += 1;
                            id
                        }
                        None => OTHER_CATEGORY,
                    },
                    Err(_) => {
                        lens.file_failures += 1;
                        OTHER_CATEGORY
                    }
                },
            };
            *lens.counts.entry(id.to_owned()).or_insert(0) += 1;
            lens.total += 1;
        }
        lens
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
        if self.by_files > self.total || self.file_failures > self.total {
            return Err("by_files・file_failures が total を超えている".into());
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
        let c = data();
        let cases = [
            ("feat(model-catalog): add a catalog", "feature"),
            ("feat: x", "feature"),
            ("feat!: breaking", "feature"),
            ("feat(x)!: scope and bang", "feature"),
            ("fix(wrapup): defer unchanged rows", "bugfix"),
            ("fix!: breaking", "bugfix"),
            ("Fix(Scope): upper case", "bugfix"),
            ("revert: x", "bugfix"),
            ("refactor(core): split", "refactor"),
            ("perf: faster", "refactor"),
            ("style: fmt", "refactor"),
            ("docs(adr): note", "docs"),
            ("test: add", "test"),
            ("test(cli): add", "test"),
            ("ci: pin action", "config"),
            ("build(deps): bump", "config"),
            ("chore: tidy", "config"),
            ("chore(deps)!: bump", "config"),
            ("wip: unknown kind", "other"),
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
            assert_eq!(classify(c, title), want, "{title}");
        }
    }

    #[test]
    fn prefix_only_decides_when_the_catalog_declares_the_prefix() {
        let c = data();
        assert_eq!(classify_prefix(c, "chore: x"), Some("config"));
        assert_eq!(classify_prefix(c, "wip: x"), None);
        assert_eq!(classify_prefix(c, "no prefix"), None);
    }

    #[test]
    fn every_class_id_exists_in_the_catalog() {
        let c = data();
        for title in [
            "feat: a",
            "fix: a",
            "refactor: a",
            "docs: a",
            "ci: a",
            "test: a",
            "x",
        ] {
            assert!(c.categories.contains_key(classify(c, title)), "{title}");
        }
    }

    #[test]
    fn build_counts_shares_and_keeps_no_titles() {
        let c = data();
        let lens = Lens::build(
            c,
            ["feat: a", "feat: b", "fix: c", "wip"],
            "2026-10-27",
            365,
        );
        assert_eq!(lens.total, 4);
        assert_eq!(lens.counts["feature"], 2);
        assert!((lens.share("feature") - 0.5).abs() < 1e-12);
        assert_eq!(lens.share("docs"), 0.0);
        let json = serde_json::to_string(&lens).unwrap();
        assert!(!json.contains("feat: a") && !json.contains("wip"), "{json}");
        assert_eq!(Lens::from_json(&json, c).unwrap(), lens);
    }

    #[test]
    fn empty_lens_has_zero_shares() {
        let lens = Lens::build(data(), [], "2026-10-27", 30);
        assert_eq!(lens.total, 0);
        assert_eq!(lens.share("feature"), 0.0);
    }

    #[test]
    fn validate_rejects_bad_sums_and_unknown_categories() {
        let mut lens = Lens::build(data(), ["feat: a"], "2026-10-27", 30);
        lens.total = 2;
        assert!(lens.validate(data()).unwrap_err().contains("合わない"));
        let mut lens = Lens::build(data(), ["feat: a"], "2026-10-27", 30);
        lens.counts.insert("surprise".into(), 0);
        assert!(lens.validate(data()).unwrap_err().contains("未知の種類"));
        assert!(Lens::from_json("{\"generated\":\"x\"}", data()).is_err());
    }

    // ------------------------------------------------ 変更ファイルによる分類(GitHub は呼ばない)

    fn pr(title: &str, number: u64) -> PrRow {
        PrRow {
            title: title.to_owned(),
            repo: "someone/secret-repo".to_owned(),
            number,
        }
    }

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn files_pick_a_category_only_when_all_files_agree() {
        let c = data();
        let cases: [(&[&str], Option<&str>); 9] = [
            (&["docs/a.md", "docs/adr/0001.md"], Some("docs")),
            (&["README.md", "CONTRIBUTING.md"], Some("docs")),
            (
                &[".github/workflows/ci.yml", ".github/workflows/nix.yml"],
                Some("config"),
            ),
            (&["tests/a.rs", "crates/x/tests/b.rs"], Some("test")),
            (
                &["flake.nix", "home/modules/x.nix", "scripts/a.sh"],
                Some("config"),
            ),
            (&["docs/a.md", ".github/workflows/ci.yml"], None), // 混在
            (&["docs/a.md", "src/main.rs"], None),              // 当たらないファイルが混じる
            (&["src/main.rs"], None),
            (&[], None),
        ];
        for (paths, want) in cases {
            assert_eq!(classify_files(c, &files(paths)), want, "{paths:?}");
        }
    }

    #[test]
    fn files_are_fetched_only_for_prs_the_prefix_cannot_decide() {
        let c = data();
        let prs = [
            pr("feat: a", 1),
            pr("chore: b", 2),
            pr("test(x): c", 3),
            pr("Update the readme", 4), // prefix 無し → 取りに行く
            pr("wip: d", 5),            // 未知の prefix → 取りに行く
            pr("revert: e", 6),
        ];
        let mut asked = Vec::new();
        let lens = Lens::build_with_files(c, &prs, "2026-10-28", 365, &mut |p| {
            asked.push(p.number);
            Ok(files(&["docs/readme.md"]))
        });
        assert_eq!(asked, [4, 5], "prefix で決まった PR には取りに行かない");
        assert_eq!(lens.total, 6);
        assert_eq!(lens.counts["docs"], 2);
        assert_eq!(lens.by_files, 2);
        assert_eq!(lens.file_failures, 0);
        assert_eq!(lens.counts["config"], 1); // chore
        assert_eq!(lens.counts["test"], 1);
        assert_eq!(lens.counts["bugfix"], 1); // revert
        assert_eq!(lens.counts["feature"], 1);
    }

    #[test]
    fn a_failed_or_truncated_fetch_falls_back_to_other_without_failing_the_run() {
        let c = data();
        let prs = [
            pr("no prefix 1", 1),
            pr("no prefix 2", 2),
            pr("no prefix 3", 3),
            pr("no prefix 4", 4),
        ];
        let lens = Lens::build_with_files(c, &prs, "2026-10-28", 365, &mut |p| match p.number {
            1 => Err("rate limited".into()),
            2 => Ok(vec!["docs/a.md".to_owned(); FILES_PAGE]), // 全体を見られない
            3 => Ok(files(&["docs/a.md"])),
            _ => Ok(files(&["docs/a.md", "src/lib.rs"])), // 混在
        });
        assert_eq!(lens.total, 4);
        assert_eq!(lens.counts["other"], 3);
        assert_eq!(lens.counts["docs"], 1);
        assert_eq!(lens.by_files, 1);
        assert_eq!(lens.file_failures, 2);
        lens.validate(c).unwrap();
    }

    #[test]
    fn lens_output_keeps_no_title_repository_or_file_name() {
        let c = data();
        let prs = [
            pr("Secret title about acme", 7),
            pr("Another secret title", 8),
            pr("feat: visible-looking but still a title", 9),
        ];
        let lens = Lens::build_with_files(c, &prs, "2026-10-28", 365, &mut |_| {
            Ok(files(&["docs/secret-file-name.md"]))
        });
        let json = serde_json::to_string_pretty(&lens).unwrap();
        for leak in [
            "Secret",
            "acme",
            "secret-repo",
            "someone",
            "secret-file-name",
            "docs/",
            "visible-looking",
        ] {
            assert!(!json.contains(leak), "{leak} が lens.json に出た: {json}");
        }
        assert_eq!(Lens::from_json(&json, c).unwrap(), lens);
    }

    /// 従来の prefix の表(`chore`・`test`・`style`・`revert` が `other`)。比較のためにだけ残す。
    fn legacy_kind(title: &str) -> &'static str {
        match prefix_kind(title).as_deref() {
            Some("feat" | "feature") => "feature",
            Some("fix" | "bugfix" | "hotfix") => "bugfix",
            Some("refactor" | "perf") => "refactor",
            Some("docs" | "doc") => "docs",
            Some("ci" | "build") => "config",
            _ => "other",
        }
    }

    #[test]
    fn the_other_share_drops_on_a_fixture_resembling_a_real_history() {
        let c = data();
        // (タイトル, 変更ファイル)。実機の傾向(feat・fix が多く、chore・test・prefix 無しが残る)に似せた架空の 20 件。
        let fixture: [(&str, &[&str]); 20] = [
            ("feat(a): x", &["src/a.rs"]),
            ("feat(b): x", &["src/b.rs"]),
            ("feat(c): x", &["src/c.rs"]),
            ("fix(a): x", &["src/a.rs"]),
            ("fix(b): x", &["src/b.rs"]),
            ("refactor: x", &["src/a.rs"]),
            ("docs: x", &["docs/a.md"]),
            ("ci: x", &[".github/workflows/ci.yml"]),
            ("build(deps): x", &["Cargo.lock"]),
            ("chore: x", &["flake.lock"]),
            ("chore(deps): x", &["Cargo.lock"]),
            ("test: x", &["tests/a.rs"]),
            ("style: x", &["src/a.rs"]),
            ("revert: x", &["src/a.rs"]),
            ("Update the readme", &["README.md"]),
            ("Add a workflow", &[".github/workflows/new.yml"]),
            ("Add tests for the parser", &["tests/parser.rs"]),
            ("Rework the parser", &["src/parser.rs", "docs/parser.md"]),
            ("wip: x", &["src/a.rs", "tests/a.rs"]),
            ("misc", &[]),
        ];
        let before = fixture
            .iter()
            .filter(|(t, _)| legacy_kind(t) == "other")
            .count();
        let prs: Vec<PrRow> = fixture
            .iter()
            .zip(1u64..)
            .map(|((t, _), n)| pr(t, n))
            .collect();
        let lens = Lens::build_with_files(c, &prs, "2026-10-28", 365, &mut |p| {
            Ok(files(fixture[(p.number - 1) as usize].1))
        });
        let after = lens.counts["other"];
        assert_eq!((before, after), (11, 3), "従来 11/20(55%) → 3/20(15%)");
        assert_eq!(lens.by_files, 3); // readme・workflow・tests
    }

    #[test]
    fn search_rows_endpoint_and_filenames_are_parsed_strictly() {
        let rows = parse_search_rows(
            r#"[{"title":"feat: a","number":12,"repository":{"nameWithOwner":"o/r","name":"r"}},
                {"number":13},
                {"title":"no repo"}]"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 2, "タイトルの無い行は読み飛ばす");
        assert_eq!(rows[0].repo, "o/r");
        assert_eq!(
            files_endpoint(&rows[0]).unwrap(),
            "repos/o/r/pulls/12/files?per_page=100"
        );
        assert_eq!(
            files_endpoint(&rows[1]),
            None,
            "リポジトリ・番号が無ければ取りに行かない"
        );
        assert_eq!(files_endpoint(&pr("t", 0)), None);
        let mut bad = pr("t", 1);
        bad.repo = "o/r;rm -rf".into();
        assert_eq!(files_endpoint(&bad), None);
        bad.repo = "o/../r/x".into();
        assert_eq!(files_endpoint(&bad), None);
        assert_eq!(parse_filenames(r#"["a","b/c"]"#).unwrap(), ["a", "b/c"]);
        assert!(parse_filenames("{}").is_err());
        assert!(parse_search_rows("not json").is_err());
    }
}
