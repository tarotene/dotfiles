//! Numbered selection over native discovery. EOF/cancel never produces a pair.
use crate::{Model, Settings};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::{BufRead, Write};

fn ask(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> Result<Option<String>> {
    write!(out, "{prompt}: ")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 || matches!(line.trim(), "q" | "cancel") {
        return Ok(None);
    }
    Ok(Some(line.trim().to_owned()))
}

fn number(
    input: &mut impl BufRead,
    out: &mut impl Write,
    prompt: &str,
    count: usize,
    default: Option<usize>,
) -> Result<Option<usize>> {
    loop {
        let prompt = match default {
            Some(n) => format!("{prompt} [Enter: {} / q: cancel]", n + 1),
            None => format!("{prompt} [q: cancel]"),
        };
        let Some(line) = ask(input, out, &prompt)? else {
            return Ok(None);
        };
        if line.is_empty() && default.is_some() {
            return Ok(default);
        }
        if let Ok(n) = line.parse::<usize>() {
            if (1..=count).contains(&n) {
                return Ok(Some(n - 1));
            }
        }
        writeln!(out, "1..={count} の番号を入力してください。")?;
    }
}

fn choose(
    input: &mut impl BufRead,
    out: &mut impl Write,
    side: &str,
    current: &Model,
    models: &[Value],
) -> Result<Option<Model>> {
    writeln!(
        out,
        "\n{side} — native model/list（全ページ、hiddenを含む）"
    )?;
    for (n, model) in models.iter().enumerate() {
        writeln!(
            out,
            "  {}. {} — {}",
            n + 1,
            model["model"]
                .as_str()
                .context("model/list is missing model ID")?,
            model["description"].as_str().unwrap_or("用途説明なし")
        )?;
    }
    let default = models
        .iter()
        .position(|m| m["model"].as_str() == Some(&current.model));
    let Some(n) = number(input, out, "モデル", models.len(), default)? else {
        return Ok(None);
    };
    let model = &models[n];
    let efforts: Vec<&str> = model["supportedReasoningEfforts"]
        .as_array()
        .context("model/list is missing supportedReasoningEfforts")?
        .iter()
        .map(|e| {
            e["reasoningEffort"]
                .as_str()
                .context("invalid native reasoning effort")
        })
        .collect::<Result<_>>()?;
    if efforts.is_empty() {
        bail!("native model advertises no supported reasoning efforts")
    }
    for (n, effort) in efforts.iter().enumerate() {
        writeln!(out, "  {}. {effort}", n + 1)?;
    }
    let default = efforts
        .iter()
        .position(|e| *e == current.reasoning_effort)
        .or_else(|| {
            efforts
                .iter()
                .position(|e| Some(*e) == model["defaultReasoningEffort"].as_str())
        });
    let Some(e) = number(input, out, "推論量", efforts.len(), default)? else {
        return Ok(None);
    };
    Ok(Some(Model {
        model: model["model"].as_str().unwrap().to_owned(),
        reasoning_effort: efforts[e].to_owned(),
    }))
}

pub fn select(
    input: &mut impl BufRead,
    out: &mut impl Write,
    current: &Settings,
    models: &[Value],
) -> Result<Option<Settings>> {
    if models.is_empty() {
        bail!("native model/list is empty; saving is disabled")
    }
    let Some(plan) = choose(input, out, "Plan", &current.plan, models)? else {
        return Ok(None);
    };
    let Some(execute) = choose(
        input,
        out,
        "Auto（実行／native Default）",
        &current.execute,
        models,
    )?
    else {
        return Ok(None);
    };
    let selected = Settings { plan, execute };
    selected.validate_models(models)?;
    writeln!(out, "\n{}", toml::to_string_pretty(&selected)?)?;
    let confirm = ask(input, out, "このペアを保存しますか? [y/N]")?;
    Ok(confirm
        .filter(|s| matches!(s.as_str(), "y" | "Y" | "yes"))
        .map(|_| selected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn models() -> Vec<Value> {
        vec![
            json!({"model":"gpt-6.1-sol","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}),
        ]
    }
    #[test]
    fn confirmed_pair_or_cancel_eof() {
        let mut out = Vec::new();
        let selected = select(
            &mut &b"1\n2\n1\n1\ny\n"[..],
            &mut out,
            &Settings::default(),
            &models(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.plan.reasoning_effort, "high");
        assert_eq!(selected.execute.reasoning_effort, "medium");
        for text in ["", "q\n", "1\n", "1\n1\n1\n1\n", "1\n1\n1\n1\nn\n"] {
            assert!(select(
                &mut text.as_bytes(),
                &mut Vec::new(),
                &Settings::default(),
                &models()
            )
            .unwrap()
            .is_none());
        }
    }
    #[test]
    fn invalid_number_reprompts_and_enter_keeps_current() {
        assert_eq!(
            select(
                &mut &b"999\nno\n\n\n\n\ny\n"[..],
                &mut Vec::new(),
                &Settings::default(),
                &models()
            )
            .unwrap(),
            Some(Settings::default())
        );
    }
}
