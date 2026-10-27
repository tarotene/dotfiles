//! Comparison output is a scenario, never an invoice or a subscription budget.
use crate::{
    catalog::{self, CatalogModel, ContextBand, TokenUsage},
    history::{self, RateObservation},
    runtime::Discovery,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn codex_home() -> Result<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".codex")))
        .context("CODEX_HOME or HOME is required")
}

pub fn observations_path() -> Result<PathBuf> {
    Ok(std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/state")))
        .context("XDG_STATE_HOME or HOME is required")?
        .join("codex-plan/rate-observations.jsonl"))
}

/// Account-bound samples cannot be derived from frozen session creator IDs.
/// Store only usage metadata locally, with private directory/file permissions.
fn save_observations(path: &Path, observations: &[RateObservation]) -> Result<()> {
    if observations.is_empty() {
        return Ok(());
    }
    let parent = path
        .parent()
        .context("rate observation path has no parent")?;
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    let mut bytes = Vec::new();
    for observation in observations {
        serde_json::to_writer(&mut bytes, observation)?;
        bytes.push(b'\n');
    }
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn usage(tokens: &history::Tokens, long_write: bool) -> TokenUsage {
    TokenUsage {
        input: tokens
            .input_tokens
            .saturating_sub(tokens.cached_input_tokens)
            .saturating_sub(tokens.cache_write_input_tokens),
        cache_read: tokens.cached_input_tokens,
        cache_write_short: if long_write {
            0
        } else {
            tokens.cache_write_input_tokens
        },
        cache_write_long: if long_write {
            tokens.cache_write_input_tokens
        } else {
            0
        },
        output: tokens.output_tokens,
    }
}

fn projected(model: &CatalogModel, samples: &[&history::UsageSample]) -> Value {
    let mut low = catalog::ApiCost::default();
    let mut high = catalog::ApiCost::default();
    let mut credits = model.credits.as_ref().map(|_| 0.0);
    for sample in samples {
        let short = usage(&sample.tokens, false);
        let mut long = usage(&sample.tokens, true);
        if !sample.cache_write_tokens_known {
            // Unknown write counts: the lower scenario assumes ordinary input,
            // the upper scenario treats all non-read input as long-TTL writes.
            long.cache_write_long = long.input;
            long.input = 0;
        }
        let lo = if sample.request_boundary_known {
            catalog::price_request(model, short)
        } else {
            catalog::price_tokens(model, short, ContextBand::Short)
        };
        let hi = if sample.request_boundary_known {
            catalog::price_request(model, long)
        } else {
            catalog::price_tokens(model, long, ContextBand::Long)
        };
        for (sum, cost) in [(&mut low, &lo.api_usd), (&mut high, &hi.api_usd)] {
            sum.input += cost.input;
            sum.cache_read += cost.cache_read;
            sum.cache_write_short += cost.cache_write_short;
            sum.cache_write_long += cost.cache_write_long;
            sum.output += cost.output;
            sum.total += cost.total;
        }
        if let (Some(sum), Some(c)) = (&mut credits, lo.codex_credits) {
            *sum += c;
        }
    }
    json!({"model":model.id,"standard_api_usd_lower":low,"standard_api_usd_upper":high,
        "standard_codex_credits":credits,"requests":samples.len(),
        "cache_write_counts_unknown":samples.iter().filter(|s| !s.cache_write_tokens_known).count(),
        "basis":"同じトークン量なら。書込数不明は非読取入力がすべて通常入力〜長期書込の範囲。Standard/global相当。TTL不明の書込は短期〜長期、リクエスト境界不明は短文〜長文の範囲。実請求・仕事の完了費用・定額枠の持続比ではない。"})
}

fn token_summary(samples: &[&history::UsageSample]) -> Value {
    let mut input = 0u64;
    let mut cached = 0u64;
    let mut writes = 0u64;
    let mut output = 0u64;
    let mut reasoning = 0u64;
    let mut models = std::collections::BTreeSet::new();
    for sample in samples {
        input = input.saturating_add(sample.tokens.input_tokens);
        cached = cached.saturating_add(sample.tokens.cached_input_tokens);
        writes = writes.saturating_add(sample.tokens.cache_write_input_tokens);
        output = output.saturating_add(sample.tokens.output_tokens);
        reasoning = reasoning.saturating_add(sample.tokens.reasoning_output_tokens);
        if let Some(model) = &sample.model {
            models.insert(model.clone());
        }
    }
    json!({"requests":samples.len(), "input_including_cache":input,"cache_read":cached,
        "cache_write_reported":writes,"cache_write_count_unknown_requests":samples.iter().filter(|s|!s.cache_write_tokens_known).count(),
        "non_read_input_including_writes":input.saturating_sub(cached),"output_including_reasoning":output,
        "reasoning_subset":reasoning,"observed_models":models})
}

pub fn build(days: u32, discovery: std::result::Result<&Discovery, &str>) -> Result<Value> {
    let timestamp = now();
    let history = history::collect(&codex_home()?, days, timestamp)?;
    let path = observations_path()?;
    let mut observations = history::read_rate_observations(&path)?;
    let catalog = catalog::catalog();
    let mut groups: BTreeMap<String, Vec<&history::UsageSample>> = BTreeMap::new();
    for sample in &history.requests {
        groups
            .entry(sample.mode.clone().unwrap_or_else(|| "unknown".into()))
            .or_default()
            .push(sample);
    }
    let scenarios: BTreeMap<_, _> = groups
        .iter()
        .map(|(mode, samples)| {
            (
                mode,
                catalog
                    .iter()
                    .map(|m| projected(m, samples))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let token_totals: BTreeMap<_, _> = groups
        .iter()
        .map(|(mode, samples)| (mode, token_summary(samples)))
        .collect();
    let observed_costs: Vec<_> = history.requests.iter().map(|sample| {
        let cost = sample.model.as_deref().and_then(catalog::lookup).map(|m| projected(&m, &[sample]));
        json!({"timestamp":sample.timestamp,"model":sample.model,"mode":sample.mode,"effort":sample.effort,"standard_equivalent":cost})
    }).collect();
    let (models, current, rate_error, native_error, rate_metadata) = match discovery {
        Ok(d) => {
            let current = d
                .rate_limits
                .as_ref()
                .map(|v| history::decode_rate_observations(v, timestamp))
                .unwrap_or_default();
            let models: Vec<Value> = d.models.iter().map(|m| json!({"model":m["model"],"description":m["description"],"supportedReasoningEfforts":m["supportedReasoningEfforts"],"hidden":m["hidden"]})).collect();
            // Backend banner text and account identity are deliberately omitted.
            let metadata = d.rate_limits.as_ref().map(|v| json!({"ordinaryUsageAllowed":v["ordinaryUsageAllowed"],
                "rateLimits":v["rateLimits"],"rateLimitsByLimitId":v["rateLimitsByLimitId"],"rateLimitResetCredits":v["rateLimitResetCredits"]}));
            (models, current, d.rate_limits_error.clone(), None, metadata)
        }
        Err(e) => (vec![], vec![], None, Some(e.to_owned()), None),
    };
    save_observations(&path, &current)?;
    observations.extend(current.iter().cloned());
    let estimates: Vec<_> = current
        .iter()
        .map(|o| history::estimate_days(&observations, o, timestamp))
        .collect();
    let current_redacted: Vec<_> = current.iter().map(|o| json!({"timestamp":o.timestamp,"bucket":o.bucket,"window":o.window,
        "reset_at":o.reset_at,"window_minutes":o.window_minutes,"used_percent":o.used_percent,
        "ordinary_usage_allowed":o.ordinary_usage_allowed,"account_attributed":o.account_id.is_some()})).collect();
    Ok(
        json!({"schema_version":2,"timestamp":timestamp,"days":days,"reference_catalog":catalog,
        "native_models":models,"native_error":native_error,"rate_limits_error":rate_error,"rate_metadata":rate_metadata,
        "current_rate_windows":current_redacted,"remaining_days":estimates,"history":history,
        "observed_standard_equivalents":observed_costs,"observed_token_totals_by_mode":token_totals,"same_token_scenarios_by_mode":scenarios,
        "fixed_example":{"tokens":catalog::example_usage(),"requests":10,"each_request_input":100_000,
            "conditions":"通常入力100,000 + キャッシュ読取900,000 + 出力20,000、書込0。10個の短文リクエスト、Standard/global。",
            "costs":catalog.iter().map(|m|json!({"model":m.id,"cost":catalog::example_cost(m)})).collect::<Vec<_>>()},
        "limitations":["同じ仕事の品質・完了費用・一日持続は未実測。API単価から定額枠のドル上限を逆算しない。",
            "モデル間の定額枠持続比は推定不可: モデル別の同一作業・アカウント・bucketの消費観測が不足。",
            "残り作業日数は同一account/bucket/resetの完了観測日のみ。未完了日・欠測・ゼロ消費・reset跨ぎは別集計。リセットが先に来る場合がある。",
            "使用率は他端末やChatGPTとの共有分を含み得る。ordinaryUsageAllowedが不明なら百分率から利用可能と断定しない。",
            "費用はStandard/globalテキストのシナリオ。実際のservice tier・地域・ツール費・税・割引は実請求と異なり得る。",
            "Codex参考contextは0.164.0の同梱値。実行設定やremote catalogは異なり得る。"]}),
    )
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
fn display(value: &Value) -> String {
    match value {
        Value::Null => "不明".into(),
        Value::String(s) => clean(s),
        _ => value.to_string(),
    }
}

pub fn render(report: &Value) -> String {
    let mut text = format!("モデル比較（公開情報取得 {}、直近{}暦日）\nAuto（実行／native Default）。承認方式とは別です。\n", catalog::retrieved(), report["days"]);
    text.push_str("\nモデル | API context / 最大出力 | Codex既定 / 実効参考 | API $/MTok 入力 / 読取 / 書込短期 / 書込長期 / 出力 | credits/MTok 入力 / 読取 / 出力\n");
    if let Some(models) = report["reference_catalog"].as_array() {
        for m in models {
            let api = &m["api"];
            let c = &m["credits"];
            let ctx = &m["codex_context"];
            let codex_value = |v: &Value| {
                if m["comparison_only"] == true {
                    "対象外".into()
                } else {
                    display(v)
                }
            };
            text.push_str(&format!(
                "{}{} | {} / {} | {} / {} | {} / {} / {} / {} / {} | {} / {} / {}\n  {}\n",
                display(&m["id"]),
                if m["comparison_only"] == true {
                    "（比較専用）"
                } else {
                    ""
                },
                m["api_context"],
                m["api_max_output"],
                codex_value(&ctx["default_window"]),
                codex_value(&ctx["default_effective"]),
                api["input"],
                api["cache_read"],
                api["cache_write_short"],
                api["cache_write_long"],
                api["output"],
                codex_value(&c["input"]),
                codex_value(&c["cache_read"]),
                codex_value(&c["output"]),
                display(&m["purpose"])
            ));
            if let Some(conditions) = m["conditions"].as_array() {
                for condition in conditions {
                    text.push_str(&format!("  {}\n", display(condition)));
                }
            }
            if let Some(sources) = m["sources"].as_array() {
                for s in sources {
                    text.push_str(&format!(
                        "  出典: {} — {}（公開 {} / 取得 {}）{}\n",
                        display(&s["author"]),
                        display(&s["title"]),
                        display(&s["published"]),
                        display(&s["retrieved"]),
                        display(&s["url"])
                    ));
                }
            }
        }
    }
    text.push_str("\nnative 選択可能モデル（掲載は推論成功の保証ではありません）\n");
    if let Some(models) = report["native_models"].as_array() {
        for model in models {
            let id = model["model"].as_str().unwrap_or("unknown");
            text.push_str(&format!(
                "  {}{}\n",
                clean(id),
                if catalog::lookup(id).is_none() {
                    " — 価格・仕様は不明（他世代から補完しない）"
                } else {
                    ""
                }
            ));
        }
    }
    let mut contexts = BTreeMap::<String, Vec<i64>>::new();
    if let Some(samples) = report["history"]["requests"].as_array() {
        for sample in samples {
            if let (Some(model), Some(window)) = (
                sample["model"].as_str(),
                sample["model_context_window"].as_i64(),
            ) {
                let windows = contexts.entry(model.to_owned()).or_default();
                if !windows.contains(&window) {
                    windows.push(window);
                }
            }
        }
    }
    text.push_str("\n履歴で観測したCodex実効context（設定・remote catalogで変わります）\n");
    if contexts.is_empty() {
        text.push_str("  観測なし。現在の実効値は不明。\n");
    }
    for (model, windows) in contexts {
        text.push_str(&format!("  {}: {:?} tokens\n", clean(&model), windows));
    }
    text.push_str("\n固定例: 通常入力10万 + キャッシュ読取90万 + 出力2万、書込0。各request入力10万×10、Standard/global\nモデル | API相当額 USD | Codex credits\n");
    if let Some(costs) = report
        .pointer("/fixed_example/costs")
        .and_then(Value::as_array)
    {
        for c in costs {
            text.push_str(&format!(
                "{} | ${:.6} | {}\n",
                display(&c["model"]),
                c.pointer("/cost/api_usd/total")
                    .and_then(Value::as_f64)
                    .unwrap(),
                if c["cost"]["codex_credits"].is_null() {
                    "対象外".into()
                } else {
                    display(&c["cost"]["codex_credits"])
                }
            ));
        }
    }
    for (key, label) in [
        ("native_error", "native一覧取得失敗・新規保存不可"),
        ("rate_limits_error", "利用枠の読取失敗"),
    ] {
        if !report[key].is_null() {
            text.push_str(&format!("\n{label}: {}\n", display(&report[key])));
        }
    }
    text.push_str(&format!(
        "\nローカル履歴: {} files / {} 帰属済み記録 / {} 帰属不明記録\n",
        report["history"]["files_read"],
        report["history"]["requests"].as_array().map_or(0, Vec::len),
        report["history"]["unattributed"]
            .as_array()
            .map_or(0, Vec::len)
    ));
    if let Some(diagnostics) = report["history"]["diagnostics"].as_array() {
        for d in diagnostics {
            text.push_str(&format!("  {}\n", display(d)));
        }
    }
    if let Some(groups) = report["same_token_scenarios_by_mode"].as_object() {
        for (mode, rows) in groups {
            text.push_str(&format!(
                "\n{}: 同じトークン量なら（API相当額 範囲 / credits）\n",
                match mode.as_str() {
                    "default" => "Auto（実行／native Default）",
                    "plan" => "Plan",
                    _ => "帰属不明",
                }
            ));
            let totals = &report["observed_token_totals_by_mode"][mode];
            text.push_str(&format!("  観測{}request / 非読取入力（書込含む）{} / キャッシュ読取{} / 報告済み書込{} / 書込数不明{}request / 出力（推論含む）{}\n  観測モデル: {}\n",totals["requests"],totals["non_read_input_including_writes"],totals["cache_read"],totals["cache_write_reported"],totals["cache_write_count_unknown_requests"],totals["output_including_reasoning"],display(&totals["observed_models"])));
            for row in rows.as_array().unwrap() {
                text.push_str(&format!(
                    "  {}: ${:.6}〜${:.6} / {}\n",
                    display(&row["model"]),
                    row["standard_api_usd_lower"]["total"].as_f64().unwrap(),
                    row["standard_api_usd_upper"]["total"].as_f64().unwrap(),
                    if row["standard_codex_credits"].is_null() {
                        "対象外".into()
                    } else {
                        display(&row["standard_codex_credits"])
                    }
                ));
            }
        }
    }
    text.push_str("\n現在の利用枠（resetはUnix秒）\n");
    text.push_str(&format!(
        "ordinaryUsageAllowed: {}\n",
        display(&report["rate_metadata"]["ordinaryUsageAllowed"])
    ));
    if let Some(windows) = report["current_rate_windows"].as_array() {
        if windows.is_empty() {
            text.push_str("利用枠の観測なし。残り日数は推定不可。\n");
        }
        for w in windows {
            text.push_str(&format!(
                "  {} / {}: {}% 使用 / reset {} / {}分 / account帰属 {}\n",
                display(&w["bucket"]),
                display(&w["window"]),
                display(&w["used_percent"]),
                display(&w["reset_at"]),
                display(&w["window_minutes"]),
                display(&w["account_attributed"])
            ));
        }
    }
    if let Some(estimates) = report["remaining_days"].as_array() {
        for e in estimates {
            text.push_str(&format!("  {} / {}: 残り作業日数 {}（完了{}日、未完了{}、欠測{}、ゼロ{}、reset跨ぎ{}）\n    {}\n",display(&e["bucket"]),display(&e["window"]),display(&e["remaining_workdays_range"]),e["complete_observed_days"],e["incomplete_days"],e["missing_days"],e["zero_consumption_days"],e["reset_crossing_days"],display(&e["reason"])));
        }
    }
    if report.get("experiment_arms").is_some() {
        text.push_str(&crate::threads::render(&report["experiment_arms"]));
    }
    if let Some(limits) = report["limitations"].as_array() {
        for l in limits {
            text.push_str(&format!("\n{}", display(l)));
        }
    }
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_ttl_bounds_and_reasoning_are_not_added_twice() {
        let sample = history::UsageSample {
            timestamp: 0,
            day: "synthetic".into(),
            model: Some("gpt-6.1-sol".into()),
            mode: Some("plan".into()),
            effort: Some("medium".into()),
            tokens: history::Tokens {
                input_tokens: 100_000,
                cached_input_tokens: 50_000,
                cache_write_input_tokens: 10_000,
                output_tokens: 2000,
                reasoning_output_tokens: 1500,
            },
            request_boundary_known: true,
            cache_write_ttl: None,
            cache_write_tokens_known: true,
            model_context_window: None,
            service_tier: None,
        };
        let result = projected(&catalog::lookup("gpt-6.1-sol").unwrap(), &[&sample]);
        assert!((result["standard_api_usd_lower"]["total"].as_f64().unwrap() - 0.13).abs() < 1e-9);
        assert!((result["standard_api_usd_upper"]["total"].as_f64().unwrap() - 0.155).abs() < 1e-9);
    }
}
