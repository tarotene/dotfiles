//! ベンダー中立のモデルカタログ。正本は `catalog.toml` の 1 本で、価格・文脈長・出典と、ベンチの
//! 定義・測定値をここから読む。codex-plan の費用比較も、model-equivalence の同等性判定も同じ型を使う。
//!
//! 不正な行(未知の提供元、段階に無いエフォート、重複した測定、未定義のベンチ、非有限の値)は
//! `Catalog::parse` が拒否する。埋め込みの `catalog.toml` は `cargo test` が検査するので、
//! 実行時に検査で落ちる状態は、テストを通ったビルドには入らない。

pub mod aa;
pub mod date;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub const CATALOG_TOML: &str = include_str!("../catalog.toml");
pub const SUPPORTED_SCHEMA: u32 = 1;

// ------------------------------------------------------------------ 価格の見え方(codex-plan から移設)

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Source {
    pub author: String,
    pub title: String,
    pub url: String,
    /// None means the page gives no publication date.
    pub published: Option<String>,
    pub retrieved: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ApiRates {
    /// USD per million tokens; all five categories are disjoint.
    pub input: f64,
    pub cache_read: f64,
    pub cache_write_short: f64,
    pub cache_write_long: f64,
    pub output: f64,
    pub short_write_ttl: String,
    pub long_write_ttl: String,
    /// Strictly greater than this many request input tokens triggers the premium.
    pub long_context_threshold: Option<u64>,
    pub long_input_multiplier: f64,
    pub long_output_multiplier: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreditRates {
    /// Credits per million tokens, from the credit rate card, not derived from USD.
    pub input: f64,
    pub cache_read: f64,
    pub output: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CodexContext {
    pub version: String,
    pub default_window: u64,
    pub maximum_override: u64,
    pub effective_percent: u64,
    pub default_effective: u64,
    pub conditions: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub purpose: String,
    pub comparison_only: bool,
    pub api_context: u64,
    pub api_max_output: u64,
    pub codex_context: Option<CodexContext>,
    pub api: ApiRates,
    pub credits: Option<CreditRates>,
    pub conditions: Vec<String>,
    pub sources: Vec<Source>,
}

// ------------------------------------------------------------------ 測定の見え方

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Retired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// 0..1 の割合。
    Fraction,
    /// 指数の点(0..100)。
    IndexPoints,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub aa_creator: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub status: Status,
    /// 公式の段階名のまま、測る対象のエフォート。並びが、表示と出力の順になる。
    pub efforts: Vec<String>,
    /// エフォート → AA の行 ID(uuid)。
    pub aa_ids: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Benchmark {
    pub name: String,
    pub unit: Unit,
    pub harness: Option<String>,
    /// 課題数。`n_unit` が数える単位(tasks / subproblems / questions)。無ければ区間を作れない。
    pub n: Option<u32>,
    pub n_unit: Option<String>,
    pub trials: Option<u32>,
    pub grader: Option<String>,
    pub note: Option<String>,
    pub source: Option<BenchSource>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BenchSource {
    pub author: String,
    pub title: String,
    pub url: String,
    pub retrieved: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AaMeta {
    pub retrieved: String,
    pub source: String,
    pub attribution: String,
    pub prompt_length_tokens: Option<u64>,
    pub parallel_queries: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub model: String,
    pub effort: String,
    /// AA が取得時にそう呼んでいた名前・slug・公開日。名前は変わりうるので、突き合わせには使わない。
    pub aa_name: String,
    pub aa_slug: String,
    pub release_date: Option<String>,
    pub median_output_tokens_per_second: Option<f64>,
    pub median_time_to_first_token_seconds: Option<f64>,
    /// ベンチ ID → 値(単位はベンチの `unit`)。値の無いベンチは、0 を入れず、載せない。
    #[serde(default)]
    pub scores: BTreeMap<String, f64>,
}

/// 同等性の判定の既定値。ページのスライダーが、この値から動かす。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Equivalence {
    /// 能力の許容幅 ±δ(0..100 の点)。割合のベンチには 1/100 にして当てる。
    pub capability_margin_points: f64,
    /// 速さの許容幅 ±δ(比)。基準に対し 1/(1+δ)〜(1+δ) の範囲を同等とする。
    pub speed_margin_ratio: f64,
    /// 履歴の内訳で、この割合以上を占める種類だけを、判定に使うベンチの選択に効かせる。
    pub history_share_min: f64,
}

/// 判定の単位。Plan 役・実行役のように、同じモデルでも役ごとに効く力が違うので役ごとに判定する。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub note: String,
    /// true の役は、履歴の内訳(種類ごとの割合)に応じて判定するベンチを絞る。
    pub follows_history: bool,
    /// 判定に使うベンチ。
    pub benchmarks: Vec<String>,
    /// 表示だけして判定に使わないベンチ(区間を作れない総合指数など)。
    #[serde(default)]
    pub reference_benchmarks: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// そのベンチが、その種類の仕事そのものを測っている。
    Direct,
    /// 近い力を測っているだけの代理。根拠欄に「代理」と出す。
    Proxy,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CategoryBench {
    pub id: String,
    pub fit: Fit,
}

/// 履歴の課題の種類。`benchmarks` が空の種類は、現行モデルに値のある公開ベンチで測れていない。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub name: String,
    pub note: Option<String>,
    #[serde(default)]
    pub benchmarks: Vec<CategoryBench>,
}

#[derive(Clone)]
pub struct Catalog {
    pub retrieved: String,
    pub providers: Vec<Provider>,
    pub models: Vec<Model>,
    pub benchmarks: BTreeMap<String, Benchmark>,
    pub equivalence: Equivalence,
    pub roles: BTreeMap<String, Role>,
    pub categories: BTreeMap<String, Category>,
    pub aa: Option<AaMeta>,
    pub measurements: Vec<Measurement>,
    pricing: Vec<CatalogModel>,
}

/// 要約だけを出す(全体を出すと、テストの失敗表示が数十 KB になる)。
impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Catalog {{ providers: {}, models: {}, benchmarks: {}, measurements: {} }}",
            self.providers.len(),
            self.models.len(),
            self.benchmarks.len(),
            self.measurements.len()
        )
    }
}

// ------------------------------------------------------------------ ファイルの形(読むためだけ)

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    schema_version: u32,
    pricing: RawPricing,
    #[serde(rename = "provider")]
    providers: Vec<RawProvider>,
    #[serde(rename = "model")]
    models: Vec<RawModel>,
    #[serde(default)]
    benchmark: BTreeMap<String, Benchmark>,
    equivalence: Equivalence,
    #[serde(default)]
    role: BTreeMap<String, Role>,
    #[serde(default)]
    category: BTreeMap<String, Category>,
    aa: Option<AaMeta>,
    #[serde(default, rename = "measurement")]
    measurements: Vec<Measurement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPricing {
    retrieved: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    id: String,
    name: String,
    aa_creator: Option<String>,
    comparison_only: bool,
    api_context: u64,
    api_max_output: u64,
    conditions: Vec<String>,
    cache: RawCache,
    long_context: Option<RawLongContext>,
    codex_context: Option<RawCodexContext>,
    #[serde(rename = "source")]
    sources: Vec<RawSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCache {
    write_short_multiplier: f64,
    write_long_multiplier: f64,
    short_ttl: String,
    long_ttl: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLongContext {
    threshold: u64,
    input_multiplier: f64,
    output_multiplier: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCodexContext {
    version: String,
    default_window: u64,
    maximum_override: u64,
    effective_percent: u64,
    conditions: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    author: String,
    title: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    id: String,
    provider: String,
    name: String,
    purpose: String,
    input: f64,
    cache_read: f64,
    output: f64,
    credits: Option<CreditRates>,
    codex_models_json_line: Option<u64>,
    docs_slug: Option<String>,
    status: Option<Status>,
    #[serde(default)]
    efforts: Vec<String>,
    #[serde(default)]
    aa_ids: BTreeMap<String, String>,
}

// ------------------------------------------------------------------ 読み込みと検査

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// `{id} {name} {docs_slug} {line}` をモデルの値に置き換える。未知の名前・値の無い名前は誤り。
fn expand(template: &str, model: &RawModel) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("{template:?}: 閉じていない {{"))?;
        let value = match &after[..close] {
            "id" => Some(model.id.clone()),
            "name" => Some(model.name.clone()),
            "docs_slug" => model.docs_slug.clone(),
            "line" => model.codex_models_json_line.map(|n| n.to_string()),
            other => return Err(format!("{template:?}: 未知の置換 {{{other}}}")),
        };
        out.push_str(&value.ok_or_else(|| {
            format!(
                "{template:?}: モデル {} に {{{}}} の値が無い",
                model.id,
                &after[..close]
            )
        })?);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn finite_positive(label: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v >= 0.0 {
        Ok(())
    } else {
        Err(format!("{label}: 0 以上の有限の値でない: {v}"))
    }
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, String> {
        let raw: RawFile = toml::from_str(text).map_err(|e| format!("catalog.toml: {e}"))?;
        if raw.schema_version != SUPPORTED_SCHEMA {
            return Err(format!(
                "schema_version {} は未対応(対応: {SUPPORTED_SCHEMA})",
                raw.schema_version
            ));
        }
        if !is_date(&raw.pricing.retrieved) {
            return Err(format!(
                "pricing.retrieved が日付でない: {}",
                raw.pricing.retrieved
            ));
        }
        let mut seen = BTreeSet::new();
        for p in &raw.providers {
            if !seen.insert(p.id.as_str()) {
                return Err(format!("provider {} が重複", p.id));
            }
        }
        let providers: BTreeMap<&str, &RawProvider> =
            raw.providers.iter().map(|p| (p.id.as_str(), p)).collect();

        let mut model_ids = BTreeSet::new();
        let mut aa_seen = BTreeMap::<&str, String>::new();
        let mut models = Vec::new();
        let mut pricing = Vec::new();
        for m in &raw.models {
            if !model_ids.insert(m.id.as_str()) {
                return Err(format!("model {} が重複", m.id));
            }
            let p = providers
                .get(m.provider.as_str())
                .ok_or_else(|| format!("model {}: 未知の provider {}", m.id, m.provider))?;
            let mut effort_seen = BTreeSet::new();
            for e in &m.efforts {
                if !effort_seen.insert(e.as_str()) {
                    return Err(format!("model {}: effort {e} が重複", m.id));
                }
            }
            for (e, id) in &m.aa_ids {
                if !m.efforts.contains(e) {
                    return Err(format!("model {}: aa_ids の {e} が efforts に無い", m.id));
                }
                if let Some(prev) = aa_seen.insert(id.as_str(), m.id.clone()) {
                    return Err(format!("AA の行 ID {id} が {prev} と {} で重複", m.id));
                }
            }
            for (label, v) in [
                ("input", m.input),
                ("cache_read", m.cache_read),
                ("output", m.output),
            ] {
                finite_positive(&format!("model {} の {label}", m.id), v)?;
            }
            models.push(Model {
                id: m.id.clone(),
                provider: m.provider.clone(),
                name: m.name.clone(),
                status: m.status.unwrap_or(Status::Active),
                efforts: m.efforts.clone(),
                aa_ids: m.aa_ids.clone(),
            });

            let mut sources = Vec::new();
            for s in &p.sources {
                sources.push(Source {
                    author: s.author.clone(),
                    title: expand(&s.title, m)?,
                    url: expand(&s.url, m)?,
                    published: None,
                    retrieved: raw.pricing.retrieved.clone(),
                });
            }
            let (threshold, im, om) = match &p.long_context {
                Some(l) => (Some(l.threshold), l.input_multiplier, l.output_multiplier),
                None => (None, 1.0, 1.0),
            };
            pricing.push(CatalogModel {
                id: m.id.clone(),
                name: m.name.clone(),
                provider: p.name.clone(),
                purpose: m.purpose.clone(),
                comparison_only: p.comparison_only,
                api_context: p.api_context,
                api_max_output: p.api_max_output,
                codex_context: match (&p.codex_context, m.codex_models_json_line) {
                    (Some(c), Some(_)) => Some(CodexContext {
                        version: c.version.clone(),
                        default_window: c.default_window,
                        maximum_override: c.maximum_override,
                        effective_percent: c.effective_percent,
                        default_effective: c.default_window * c.effective_percent / 100,
                        conditions: c.conditions.clone(),
                    }),
                    (Some(_), None) => {
                        return Err(format!(
                            "model {}: provider に codex_context があるのに codex_models_json_line が無い",
                            m.id
                        ))
                    }
                    (None, _) => None,
                },
                api: ApiRates {
                    input: m.input,
                    cache_read: m.cache_read,
                    cache_write_short: m.input * p.cache.write_short_multiplier,
                    cache_write_long: m.input * p.cache.write_long_multiplier,
                    output: m.output,
                    short_write_ttl: p.cache.short_ttl.clone(),
                    long_write_ttl: p.cache.long_ttl.clone(),
                    long_context_threshold: threshold,
                    long_input_multiplier: im,
                    long_output_multiplier: om,
                },
                credits: m.credits.clone(),
                conditions: p.conditions.clone(),
                sources,
            });
        }

        for (id, b) in &raw.benchmark {
            if matches!(b.n, Some(0)) {
                return Err(format!("benchmark {id}: n が 0"));
            }
            if b.n.is_some() != b.n_unit.is_some() {
                return Err(format!(
                    "benchmark {id}: n と n_unit は両方書くか、両方書かない"
                ));
            }
            if let Some(s) = &b.source {
                if !s.url.starts_with("https://") || !is_date(&s.retrieved) {
                    return Err(format!("benchmark {id}: source の url か取得日が不正"));
                }
            }
        }
        if let Some(aa) = &raw.aa {
            if !is_date(&aa.retrieved) {
                return Err(format!("aa.retrieved が日付でない: {}", aa.retrieved));
            }
        }
        let eq = &raw.equivalence;
        for (label, v) in [
            ("capability_margin_points", eq.capability_margin_points),
            ("speed_margin_ratio", eq.speed_margin_ratio),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("equivalence.{label} は 0 より大きい有限の値: {v}"));
            }
        }
        if !(eq.history_share_min > 0.0 && eq.history_share_min < 1.0) {
            return Err(format!(
                "equivalence.history_share_min は 0 と 1 の間: {}",
                eq.history_share_min
            ));
        }
        for (id, role) in &raw.role {
            let mut seen = BTreeSet::new();
            for b in role.benchmarks.iter().chain(&role.reference_benchmarks) {
                if !raw.benchmark.contains_key(b) {
                    return Err(format!("role {id}: 未定義のベンチ {b}"));
                }
                if !seen.insert(b) {
                    return Err(format!("role {id}: ベンチ {b} が重複"));
                }
            }
        }
        for (id, cat) in &raw.category {
            let mut seen = BTreeSet::new();
            for b in &cat.benchmarks {
                if !raw.benchmark.contains_key(&b.id) {
                    return Err(format!("category {id}: 未定義のベンチ {}", b.id));
                }
                if !seen.insert(&b.id) {
                    return Err(format!("category {id}: ベンチ {} が重複", b.id));
                }
            }
        }
        if !raw.measurements.is_empty() && raw.aa.is_none() {
            return Err("[[measurement]] があるのに [aa] が無い(取得日と出典の表示が要る)".into());
        }
        let by_id: BTreeMap<&str, &Model> = models.iter().map(|m| (m.id.as_str(), m)).collect();
        let mut keys = BTreeSet::new();
        for r in &raw.measurements {
            let m = by_id
                .get(r.model.as_str())
                .ok_or_else(|| format!("measurement: 未知の model {}", r.model))?;
            if !m.efforts.contains(&r.effort) {
                return Err(format!(
                    "measurement: model {} の efforts に {} が無い",
                    r.model, r.effort
                ));
            }
            if !keys.insert((r.model.as_str(), r.effort.as_str())) {
                return Err(format!("measurement: ({}, {}) が重複", r.model, r.effort));
            }
            for (bench, v) in &r.scores {
                if !raw.benchmark.contains_key(bench) {
                    return Err(format!(
                        "measurement {}/{}: 未定義のベンチ {bench}",
                        r.model, r.effort
                    ));
                }
                finite_positive(
                    &format!("measurement {}/{} の {bench}", r.model, r.effort),
                    *v,
                )?;
            }
            for (label, v) in [
                ("tokens_per_second", r.median_output_tokens_per_second),
                ("ttft", r.median_time_to_first_token_seconds),
            ] {
                if let Some(v) = v {
                    finite_positive(
                        &format!("measurement {}/{} の {label}", r.model, r.effort),
                        v,
                    )?;
                }
            }
        }

        Ok(Catalog {
            retrieved: raw.pricing.retrieved,
            providers: raw
                .providers
                .iter()
                .map(|p| Provider {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    aa_creator: p.aa_creator.clone(),
                })
                .collect(),
            models,
            benchmarks: raw.benchmark,
            equivalence: raw.equivalence,
            roles: raw.role,
            categories: raw.category,
            aa: raw.aa,
            measurements: raw.measurements,
            pricing,
        })
    }

    /// 価格の見え方(codex-plan の費用比較が読む)。並びはファイルの `[[model]]` の順。
    pub fn pricing(&self) -> &[CatalogModel] {
        &self.pricing
    }

    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn measurement(&self, model: &str, effort: &str) -> Option<&Measurement> {
        self.measurements
            .iter()
            .find(|m| m.model == model && m.effort == effort)
    }
}

/// 埋め込みのカタログ。検査は `cargo test` が行うので、ここでの失敗はビルドの不良を意味する。
pub fn data() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        Catalog::parse(CATALOG_TOML)
            .unwrap_or_else(|e| panic!("埋め込みの catalog.toml が不正: {e}"))
    })
}

pub fn catalog() -> Vec<CatalogModel> {
    data().pricing().to_vec()
}

/// Exact model IDs only. Unknown versions never inherit another model's rates.
pub fn lookup(id: &str) -> Option<CatalogModel> {
    data().pricing().iter().find(|m| m.id == id).cloned()
}

/// 価格・文脈長の出典を最後に読んだ日。
pub fn retrieved() -> &'static str {
    &data().retrieved
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成ブロックを空にした土台(測定値を足すテストが、`[aa]` を二重にしないため)。
    fn base() -> String {
        aa::splice(CATALOG_TOML, &format!("{}\n{}\n", aa::BEGIN, aa::END)).unwrap()
    }

    fn with(edit: impl Fn(&str) -> String) -> Result<Catalog, String> {
        Catalog::parse(&edit(&base()))
    }

    /// 追跡するモデルの (モデル × エフォート) は、1 つも欠けずに測定値を持つ。エフォートの段階を
    /// 読み替えたり、段階を落としたりすると、ここで落ちる(R5: 関係を崩さない)。
    #[test]
    fn every_tracked_model_effort_pair_has_a_measurement() {
        let c = data();
        let mut tracked = 0;
        for m in c.models.iter().filter(|m| !m.aa_ids.is_empty()) {
            assert_eq!(
                m.aa_ids.keys().collect::<BTreeSet<_>>(),
                m.efforts.iter().collect::<BTreeSet<_>>(),
                "{}: aa_ids と efforts が食い違う",
                m.id
            );
            for e in &m.efforts {
                let row = c
                    .measurement(&m.id, e)
                    .unwrap_or_else(|| panic!("{} / {e} の測定が無い", m.id));
                assert!(
                    row.scores.contains_key("terminalbench_v4_0"),
                    "{} / {e}",
                    m.id
                );
                tracked += 1;
            }
        }
        assert_eq!(tracked, 20);
        assert_eq!(c.measurements.len(), tracked);
        assert!(c
            .aa
            .as_ref()
            .unwrap()
            .attribution
            .contains("artificialanalysis.ai"));
    }

    #[test]
    fn embedded_catalog_is_valid_and_keeps_model_order() {
        let c = Catalog::parse(CATALOG_TOML).unwrap();
        let ids: Vec<_> = c.pricing().iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "gpt-6-astra",
                "gpt-6.1-sol",
                "gpt-6-sol",
                "gpt-6-luna",
                "claude-fable-5-1",
                "claude-opus-5-5",
                "claude-sonnet-5-5"
            ]
        );
    }

    #[test]
    fn derived_values_are_derived_not_stored() {
        let sol = lookup("gpt-6.1-sol").unwrap();
        let ctx = sol.codex_context.unwrap();
        assert_eq!(ctx.default_effective, 258_400);
        assert_eq!(sol.api.cache_write_short, 2.0 * 1.25);
        assert_eq!(sol.api.cache_write_long, 2.0 * 2.5);
        assert!(sol.sources[0].url.ends_with("/models/gpt-6.1-sol"));
        assert!(sol.sources[4].url.ends_with("models.json#L206"));
        let opus = lookup("claude-opus-5-5").unwrap();
        assert_eq!(
            opus.sources[0].url,
            "https://platform.claude.com/docs/en/models/opus-5-5/overview"
        );
        assert_eq!(opus.api.long_context_threshold, None);
        assert!(opus.codex_context.is_none() && opus.credits.is_none());
    }

    #[test]
    fn rejects_unknown_provider() {
        let e = with(|t| {
            t.replacen(
                "provider = \"openai\"\nname = \"GPT-6 Astra\"",
                "provider = \"nobody\"\nname = \"GPT-6 Astra\"",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("未知の provider"), "{e}");
    }

    #[test]
    fn rejects_effort_outside_the_model_steps() {
        let e = with(|t| {
            t.replacen(
                "low = \"e76e0fe5-93ca-4272-9359-2eb1740467d1\" # gpt-6-1-sol-low",
                "ultra = \"e76e0fe5-93ca-4272-9359-2eb1740467d1\"",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("efforts に無い"), "{e}");
    }

    #[test]
    fn rejects_duplicate_aa_row_id() {
        let e = with(|t| {
            t.replacen(
                "05776db7-f5c0-40f7-b824-079160f8cfa8",
                "e76e0fe5-93ca-4272-9359-2eb1740467d1",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("重複"), "{e}");
    }

    #[test]
    fn rejects_unknown_placeholder_and_missing_value() {
        let e = with(|t| t.replacen("/models/{docs_slug}/overview", "/models/{nope}/overview", 1))
            .unwrap_err();
        assert!(e.contains("未知の置換"), "{e}");
        let e = with(|t| t.replacen("docs_slug = \"fable-5-1\"\n", "", 1)).unwrap_err();
        assert!(e.contains("値が無い"), "{e}");
    }

    fn measurement_block(extra: &str) -> String {
        format!(
            "[aa]\nretrieved = \"2026-10-27\"\nsource = \"https://example.com\"\nattribution = \"x\"\n\n\
             [[measurement]]\nmodel = \"claude-opus-5-5\"\neffort = \"high\"\naa_name = \"n\"\naa_slug = \"s\"\n{extra}\n"
        )
    }

    fn with_block(block: &str) -> Result<Catalog, String> {
        with(|t| t.replace("# END GENERATED", &format!("{block}# END GENERATED")))
    }

    #[test]
    fn accepts_a_measurement_and_finds_it() {
        let c = with_block(&measurement_block(
            "[measurement.scores]\nterminalbench_v4_0 = 0.5\n",
        ))
        .unwrap();
        assert_eq!(
            c.measurement("claude-opus-5-5", "high").unwrap().scores["terminalbench_v4_0"],
            0.5
        );
        assert!(c.measurement("claude-opus-5-5", "low").is_none());
    }

    #[test]
    fn rejects_bad_measurements() {
        let e = with_block(&measurement_block(
            "[measurement.scores]\nswe_bench = 0.5\n",
        ))
        .unwrap_err();
        assert!(e.contains("未定義のベンチ"), "{e}");
        let e = with_block(&measurement_block(
            "[measurement.scores]\nterminalbench_v4_0 = -0.1\n",
        ))
        .unwrap_err();
        assert!(e.contains("有限の値でない"), "{e}");
        let dup = format!("{}{}", measurement_block(""), "[[measurement]]\nmodel = \"claude-opus-5-5\"\neffort = \"high\"\naa_name = \"n\"\naa_slug = \"s\"\n");
        let e = with_block(&dup).unwrap_err();
        assert!(e.contains("重複"), "{e}");
        let e =
            with_block(&measurement_block("").replace("effort = \"high\"", "effort = \"ultra\""))
                .unwrap_err();
        assert!(e.contains("efforts に"), "{e}");
        let e = with_block(&measurement_block("").replace("claude-opus-5-5", "nope")).unwrap_err();
        assert!(e.contains("未知の model"), "{e}");
    }

    #[test]
    fn measurement_without_attribution_block_is_rejected() {
        let e = with_block("[[measurement]]\nmodel = \"claude-opus-5-5\"\neffort = \"high\"\naa_name = \"n\"\naa_slug = \"s\"\n").unwrap_err();
        assert!(e.contains("[aa] が無い"), "{e}");
    }

    #[test]
    fn rejects_unknown_fields() {
        let e = with(|t| t.replacen("schema_version = 1", "schema_version = 1\nsurprise = 1", 1))
            .unwrap_err();
        assert!(e.contains("surprise"), "{e}");
    }

    #[test]
    fn embedded_roles_and_categories_point_at_defined_benchmarks() {
        let c = data();
        assert_eq!(
            c.roles.keys().map(String::as_str).collect::<Vec<_>>(),
            ["execute", "plan"]
        );
        assert_eq!(
            c.categories.keys().map(String::as_str).collect::<Vec<_>>(),
            ["bugfix", "config", "docs", "feature", "other", "refactor"]
        );
        assert!(c.roles["execute"].follows_history && !c.roles["plan"].follows_history);
        // 総合指数は区間を作れないので、判定ではなく表示だけに回している。
        assert!(!c.roles["plan"]
            .benchmarks
            .contains(&"artificial_analysis_intelligence_index".to_owned()));
        assert!(c.categories["other"].benchmarks.is_empty());
    }

    #[test]
    fn rejects_dangling_role_and_category_benchmarks() {
        let e = with(|t| {
            t.replacen(
                "benchmarks = [\"lcr\", \"hle\"]",
                "benchmarks = [\"lcr\", \"nope\"]",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("role plan: 未定義のベンチ nope"), "{e}");
        let e = with(|t| {
            t.replacen(
                "{ id = \"lcr\", fit = \"proxy\" }",
                "{ id = \"nope\", fit = \"proxy\" }",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("category docs: 未定義のベンチ nope"), "{e}");
        let e = with(|t| {
            t.replacen(
                "benchmarks = [\"lcr\", \"hle\"]",
                "benchmarks = [\"lcr\", \"lcr\"]",
                1,
            )
        })
        .unwrap_err();
        assert!(e.contains("重複"), "{e}");
    }

    #[test]
    fn rejects_non_positive_margins() {
        let e = with(|t| t.replacen("speed_margin_ratio = 0.2", "speed_margin_ratio = 0.0", 1))
            .unwrap_err();
        assert!(e.contains("speed_margin_ratio"), "{e}");
        let e = with(|t| t.replacen("history_share_min = 0.1", "history_share_min = 1.0", 1))
            .unwrap_err();
        assert!(e.contains("history_share_min"), "{e}");
    }

    #[test]
    fn rejects_unsupported_schema() {
        let e = with(|t| t.replacen("schema_version = 1", "schema_version = 2", 1)).unwrap_err();
        assert!(e.contains("未対応"), "{e}");
    }
}
