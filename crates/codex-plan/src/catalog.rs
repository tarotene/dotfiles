//! Versioned public reference data, independent API/credit units, and request pricing.
//! Availability and actual runtime context always belong to native discovery/history.
use serde::{Deserialize, Serialize};

pub const RETRIEVED: &str = "2026-10-26";

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

/// Token categories are disjoint. `output` already includes reasoning tokens.
/// This deliberately has no separately billable reasoning field.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write_short: u64,
    pub cache_write_long: u64,
    pub output: u64,
}

impl TokenUsage {
    pub fn prompt_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write_short)
            .saturating_add(self.cache_write_long)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextBand {
    Short,
    Long,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ApiCost {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write_short: f64,
    pub cache_write_long: f64,
    pub output: f64,
    pub total: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CostEstimate {
    pub api_usd: ApiCost,
    pub codex_credits: Option<f64>,
    pub api_long_context: bool,
}

/// Price one request. Never pass a multi-request aggregate here: the long-context
/// threshold applies to each request, including cached inputs and cache writes.
pub fn price_request(model: &CatalogModel, usage: TokenUsage) -> CostEstimate {
    let long = model
        .api
        .long_context_threshold
        .is_some_and(|threshold| usage.prompt_tokens() > threshold);
    price_tokens(
        model,
        usage,
        if long {
            ContextBand::Long
        } else {
            ContextBand::Short
        },
    )
}

/// Explicit scenario for aggregates whose request boundaries are unavailable.
/// Call with both bands to report bounds, rather than silently assuming short.
/// These are Standard, global, text-token equivalents, not actual invoices.
pub fn price_tokens(model: &CatalogModel, usage: TokenUsage, band: ContextBand) -> CostEstimate {
    let rates = &model.api;
    let long = band == ContextBand::Long && rates.long_context_threshold.is_some();
    let im = if long {
        rates.long_input_multiplier
    } else {
        1.0
    } / 1_000_000.0;
    let om = if long {
        rates.long_output_multiplier
    } else {
        1.0
    } / 1_000_000.0;
    let mut api_usd = ApiCost {
        input: usage.input as f64 * rates.input * im,
        cache_read: usage.cache_read as f64 * rates.cache_read * im,
        cache_write_short: usage.cache_write_short as f64 * rates.cache_write_short * im,
        cache_write_long: usage.cache_write_long as f64 * rates.cache_write_long * im,
        output: usage.output as f64 * rates.output * om,
        total: 0.0,
    };
    api_usd.total = api_usd.input
        + api_usd.cache_read
        + api_usd.cache_write_short
        + api_usd.cache_write_long
        + api_usd.output;
    // Codex's own rate card has no separate cache-write surcharge. Writes are
    // ordinary non-cached input here; API context multipliers do not cross units.
    let codex_credits = model.credits.as_ref().map(|r| {
        ((usage.input as f64 + usage.cache_write_short as f64 + usage.cache_write_long as f64)
            * r.input
            + usage.cache_read as f64 * r.cache_read
            + usage.output as f64 * r.output)
            / 1_000_000.0
    });
    CostEstimate {
        api_usd,
        codex_credits,
        api_long_context: long,
    }
}

/// Ten synthetic requests, each with 10k ordinary + 90k cached + 2k output.
/// The aggregate million-input example is not one long-context request.
pub fn example_usage() -> TokenUsage {
    TokenUsage {
        input: 100_000,
        cache_read: 900_000,
        output: 20_000,
        ..TokenUsage::default()
    }
}

pub fn example_cost(model: &CatalogModel) -> CostEstimate {
    price_tokens(model, example_usage(), ContextBand::Short)
}

fn source(author: &str, title: &str, url: &str, published: Option<&str>) -> Source {
    Source {
        author: author.into(),
        title: title.into(),
        url: url.into(),
        published: published.map(str::to_owned),
        retrieved: RETRIEVED.into(),
    }
}

pub fn catalog() -> Vec<CatalogModel> {
    let openai = [
        (
            "gpt-6-astra",
            "GPT-6 Astra",
            "複雑な推論・コーディング・研究など、最も難しい仕事向け",
            10.0,
            1.0,
            50.0,
            250.0,
            25.0,
            1250.0,
            34,
        ),
        (
            "gpt-6.1-sol",
            "GPT-6.1 Sol",
            "複雑なコーディングや専門業務で、性能と費用の均衡を取る",
            2.0,
            0.1,
            10.0,
            50.0,
            2.5,
            250.0,
            206,
        ),
        (
            "gpt-6-sol",
            "GPT-6 Sol",
            "複雑なコーディングとエージェントの作業向け",
            2.0,
            0.2,
            10.0,
            50.0,
            5.0,
            250.0,
            382,
        ),
        (
            "gpt-6-luna",
            "GPT-6 Luna",
            "対象を絞った大量処理向けの効率重視モデル",
            0.1,
            0.01,
            0.5,
            2.5,
            0.25,
            12.5,
            553,
        ),
    ];
    let mut models = Vec::new();
    for (id, name, purpose, input, read, output, ci, cr, co, line) in openai {
        models.push(CatalogModel {
            id: id.into(), name: name.into(), provider: "OpenAI".into(),
            purpose: purpose.into(), comparison_only: false,
            api_context: 1_050_000, api_max_output: 128_000,
            codex_context: Some(CodexContext {
                version: "rust-v0.164.0 (6aaac694803004a63c6f393d153c96d22521a6b9)".into(),
                default_window: 272_000, maximum_override: 872_000,
                effective_percent: 95, default_effective: 258_400,
                conditions: "同梱upstreamカタログの既定参考値。実行中の値ではない。リモートカタログ・設定の上書きで変わる。最大値は設定上書きの上限で、自動適用されない。".into(),
            }),
            api: ApiRates { input, cache_read: read, cache_write_short: input * 1.25,
                cache_write_long: input * 2.5, output,
                short_write_ttl: "30m".into(), long_write_ttl: "12h".into(),
                long_context_threshold: Some(272_000), long_input_multiplier: 2.0,
                long_output_multiplier: 1.5 },
            credits: Some(CreditRates { input: ci, cache_read: cr, output: co }),
            conditions: vec![
                "API: Standard・global・テキスト、USD/100万tokens。ツール料金・税・地域加算を含まない。".into(),
                "API: 各リクエストの入力（キャッシュを含む）が272,000を超えると入力とキャッシュ2倍、出力1.5倍。".into(),
                "credits: Standardの別料金表。キャッシュ書込の追加課金なし。定額枠の消費率・ドル上限には換算しない。".into(),
                "Fast/Ultrafast・Batch/Flex・地域指定はこの費用例の対象外。同じ仕事の完了費用・品質・持続時間を示さない。".into(),
            ],
            sources: vec![
                source("OpenAI", name, &format!("https://developers.openai.com/api/docs/models/{id}"), None),
                source("OpenAI", "API Pricing — Standard text rates and 30m/12h cache writes", "https://developers.openai.com/api/docs/pricing", None),
                source("OpenAI", "Prompt caching — GPT-5.6 and later, cache lifetimes and disjoint usage categories", "https://developers.openai.com/api/docs/guides/prompt-caching", None),
                source("OpenAI", "Pricing — Codex token credit rates", "https://learn.chatgpt.com/docs/pricing", None),
                source("OpenAI", "Codex rust-v0.164.0 bundled model context", &format!("https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/models-manager/models.json#L{line}"), None),
                source("OpenAI", "Codex ModelInfo — default effective percent and usable_context_window", "https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/openai_models.rs#L393", None),
                source("OpenAI", "Codex ModelInfo — usable context calculation", "https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/openai_models.rs#L523", None),
                source("OpenAI", "Codex with_config_overrides — context clamped to model maximum", "https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/models-manager/src/model_info.rs#L19", None),
            ],
        });
    }
    for (id, name, purpose, input, read, output) in [
        (
            "claude-fable-5-1",
            "Claude Fable 5.1",
            "難しい推論と長期のエージェント作業向け",
            10.0,
            0.25,
            50.0,
        ),
        (
            "claude-opus-5-5",
            "Claude Opus 5.5",
            "長時間のエージェントによるコーディングと知識業務向け",
            4.0,
            0.2,
            20.0,
        ),
        (
            "claude-sonnet-5-5",
            "Claude Sonnet 5.5",
            "速度と知能の両立を重視",
            2.0,
            0.1,
            10.0,
        ),
    ] {
        let slug = id.strip_prefix("claude-").unwrap();
        models.push(CatalogModel {
            id: id.into(), name: name.into(), provider: "Anthropic".into(),
            purpose: purpose.into(), comparison_only: true,
            api_context: 1_000_000, api_max_output: 128_000, codex_context: None,
            api: ApiRates { input, cache_read: read, cache_write_short: input * 1.25,
                cache_write_long: input * 2.0, output,
                short_write_ttl: "5m".into(), long_write_ttl: "1h".into(),
                long_context_threshold: None, long_input_multiplier: 1.0,
                long_output_multiplier: 1.0 },
            credits: None,
            conditions: vec![
                "比較専用。Codexでの可用性を示さない。Claude APIのStandard・global・テキスト、USD/100万tokens。".into(),
                "1M context全域で標準単価。キャッシュ書込は5m/1hで別単価。Batch・Fast・地域加算・ツール料金・税は対象外。".into(),
                "最大出力は通常API。Opus/SonnetのBatch beta 300K出力とは区別する。".into(),
            ],
            sources: vec![
                source("Anthropic", name, &format!("https://platform.claude.com/docs/en/models/{slug}/overview"), None),
                source("Anthropic", "Pricing — model pricing, caching and long context", "https://platform.claude.com/docs/en/about-claude/pricing", None),
            ],
        });
    }
    models
}

/// Exact model IDs only. Unknown versions never inherit another model's rates.
pub fn lookup(id: &str) -> Option<CatalogModel> {
    catalog().into_iter().find(|model| model.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-10, "{a} != {b}");
    }

    #[test]
    fn example_has_independent_api_and_credit_oracles() {
        // Dollars calculated independently in cents: ordinary + cache + output.
        let usd = [
            (100.0 + 90.0 + 100.0) / 100.0,
            (20.0 + 9.0 + 20.0) / 100.0,
            (20.0 + 18.0 + 20.0) / 100.0,
            (1.0 + 0.9 + 1.0) / 100.0,
            (100.0 + 22.5 + 100.0) / 100.0,
            (40.0 + 18.0 + 40.0) / 100.0,
            (20.0 + 9.0 + 20.0) / 100.0,
        ];
        let credits = [
            Some(25.0 + 22.5 + 25.0),
            Some(5.0 + 2.25 + 5.0),
            Some(5.0 + 4.5 + 5.0),
            Some(0.25 + 0.225 + 0.25),
            None,
            None,
            None,
        ];
        for ((model, expected), credit) in catalog().iter().zip(usd).zip(credits) {
            let cost = example_cost(model);
            close(cost.api_usd.total, expected);
            match (cost.codex_credits, credit) {
                (Some(a), Some(b)) => close(a, b),
                (None, None) => (),
                _ => panic!("credit unit mismatch"),
            }
            assert!(!cost.api_long_context);
        }
    }

    #[test]
    fn request_threshold_is_strict_and_counts_cache() {
        let model = lookup("gpt-6.1-sol").unwrap();
        let mut usage = TokenUsage {
            input: 1,
            cache_read: 271_999,
            output: 2000,
            ..TokenUsage::default()
        };
        let short = price_request(&model, usage);
        assert!(!short.api_long_context);
        usage.cache_read += 1;
        let long = price_request(&model, usage);
        assert!(long.api_long_context);
        close(long.api_usd.input, short.api_usd.input * 2.0);
        close(long.api_usd.output, short.api_usd.output * 1.5);
        let no_premium = price_tokens(&model, usage, ContextBand::Short);
        close(
            long.codex_credits.unwrap(),
            no_premium.codex_credits.unwrap(),
        );
    }

    #[test]
    fn cache_writes_are_disjoint_and_keep_ttl_rates() {
        let usage = TokenUsage {
            cache_write_short: 100_000,
            cache_write_long: 100_000,
            output: 10_000,
            ..TokenUsage::default()
        };
        let model = lookup("gpt-6.1-sol").unwrap();
        let cost = price_request(&model, usage);
        close(cost.api_usd.cache_write_short, 0.25);
        close(cost.api_usd.cache_write_long, 0.5);
        close(cost.api_usd.input, 0.0);
        close(cost.api_usd.output, 0.1); // reasoning is already included here
        close(cost.codex_credits.unwrap(), 12.5);
        let claude = price_request(&lookup("claude-sonnet-5-5").unwrap(), usage);
        close(claude.api_usd.cache_write_long, 0.4);
        assert_eq!(claude.codex_credits, None);
    }

    #[test]
    fn no_cross_generation_fallback_or_claude_long_premium() {
        assert!(lookup("gpt-6.2-sol").is_none());
        let model = lookup("claude-fable-5-1").unwrap();
        let cost = price_request(
            &model,
            TokenUsage {
                input: 900_000,
                ..TokenUsage::default()
            },
        );
        assert!(!cost.api_long_context);
        close(cost.api_usd.total, 9.0);
        assert!(model.codex_context.is_none());
    }

    #[test]
    fn reference_context_and_provenance_survive_serialization() {
        for model in catalog() {
            assert!(model
                .sources
                .iter()
                .all(|s| s.retrieved == RETRIEVED && s.url.starts_with("https://")));
            if let Some(context) = &model.codex_context {
                assert_eq!(
                    context.default_window * context.effective_percent / 100,
                    context.default_effective
                );
            }
            let json = serde_json::to_string(&model).unwrap();
            assert_eq!(serde_json::from_str::<CatalogModel>(&json).unwrap(), model);
        }
    }
}
