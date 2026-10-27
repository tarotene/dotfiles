//! Request pricing over the shared model catalog.
//!
//! The reference data (rates, contexts, sources) lives in `model-catalog`'s `catalog.toml`, the single
//! source shared with the equivalence page. This module owns only the token categories and the cost
//! arithmetic. Availability and actual runtime context always belong to native discovery/history.
use serde::{Deserialize, Serialize};

pub use model_catalog::{
    catalog, lookup, retrieved, ApiRates, CatalogModel, CodexContext, CreditRates, Source,
};

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
                .all(|s| s.retrieved == retrieved() && s.url.starts_with("https://")));
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
