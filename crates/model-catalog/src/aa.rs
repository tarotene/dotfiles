//! Artificial Analysis(AA)の `/data/llms/models` の応答から、カタログの生成ブロックを作る。
//!
//! 生成ブロックには、測定値のほか、手で登録していない提供元・モデルの自動登録も入る。
//!
//! - 突き合わせは AA の行 ID(uuid)で行う。AA 公式が、slug や名前は変わりうるので ID を主キーに
//!   するよう推奨している(<https://artificialanalysis.ai/api-reference>、取得 2026-10-27)。
//! - 取り込むのは、カタログの `[benchmark.*]` に定義済みのベンチだけで、値が null のものは載せない。
//! - 手登録のモデル(生成ブロックの外の `[[model]]`)は書き換えない。手登録と同名の別の行は取り込まず、
//!   報告する。
//! - 新しいモデルは、`[discovery] aa_release_since` 以降の公開で、判定に使うベンチのどれかに値が
//!   ある行だけを載せる。既に自動登録したモデルは、日付の外に出ても、値が null になっても残る。

use crate::{AaMeta, Catalog, GeneratedBlock, Measurement, Model, Provider, Status};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const ENDPOINT: &str = "https://artificialanalysis.ai/api/v2/data/llms/models";
pub const ATTRIBUTION: &str = "Artificial Analysis (https://artificialanalysis.ai/)";
pub const BEGIN: &str = "# BEGIN GENERATED (model-catalog fetch-aa) — do not edit by hand";
pub const END: &str = "# END GENERATED";

/// エフォートとして読む括弧の先頭語(閉じた語彙)。表示と出力の順もこの並び。
pub const EFFORT_VOCABULARY: [&str; 8] = [
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "reasoning",
    "non-reasoning",
];
/// 語彙に当たる括弧が無い行の、エフォートの名前。
pub const DEFAULT_EFFORT: &str = "default";

fn effort_rank(effort: &str) -> usize {
    EFFORT_VOCABULARY
        .iter()
        .position(|e| *e == effort)
        .unwrap_or(EFFORT_VOCABULARY.len())
}

/// 行の名前を、基本名とエフォートに分ける。末尾の括弧の先頭語(カンマの前)が語彙に当たるときだけ
/// エフォートとして括弧を外し、それ以外(`(Dec '24)`・`(Preview)` など)は名前の一部のまま `default`。
pub fn split_effort(name: &str) -> (String, &'static str) {
    let name = name.trim();
    if let Some(inner) = name.strip_suffix(')') {
        if let Some(open) = inner.rfind('(') {
            let first = inner[open + 1..]
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let base = inner[..open].trim_end();
            if let Some(effort) = EFFORT_VOCABULARY.iter().find(|e| **e == first) {
                if !base.is_empty() {
                    return (base.to_owned(), effort);
                }
            }
        }
    }
    (name.to_owned(), DEFAULT_EFFORT)
}

/// モデル ID・提供元 ID に使う形(ASCII の小文字・数字・`-`)。
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "model".to_owned()
    } else {
        out.to_owned()
    }
}

#[derive(Debug, PartialEq)]
pub struct Missing {
    pub model: String,
    pub effort: String,
    pub aa_id: String,
}

/// 同じ (提供元, 基本名, エフォート) に複数の行があり、公開が新しい方だけを取った。
#[derive(Debug, PartialEq)]
pub struct Conflict {
    pub creator: String,
    pub base_name: String,
    pub effort: String,
    /// 取った行 `名前 (ID)`。
    pub kept: String,
    /// 取らなかった行 `名前 (ID)`。
    pub dropped: Vec<String>,
}

/// 手登録のモデルと同名の別の行。取り込まない。
#[derive(Debug, PartialEq)]
pub struct Skipped {
    pub creator: String,
    pub name: String,
    pub id: String,
    pub manual_model: String,
}

#[derive(Debug)]
pub struct Report {
    pub meta: AaMeta,
    /// 自動登録の提供元(生成ブロックに書く)。
    pub providers: Vec<Provider>,
    /// 自動登録のモデル(生成ブロックに書く)。
    pub models: Vec<Model>,
    /// 手登録・自動登録の全測定値(応答に無い行は、前回の値を持ち越す)。
    pub measurements: Vec<Measurement>,
    /// 登録済みなのに応答に無い行(最後の値は持ち越す)。
    pub missing: Vec<Missing>,
    /// 手登録のモデルで、応答に 1 行も無いもの(`status` は手登録の部分なので、人が `retired` にする)。
    pub retire_candidates: Vec<String>,
    /// 今回、新しく自動登録したモデル ID。
    pub added: Vec<String>,
    /// 今回、応答に 1 行も無くなって `retired` にした自動登録のモデル ID。
    pub retired: Vec<String>,
    pub conflicts: Vec<Conflict>,
    pub skipped: Vec<Skipped>,
}

struct Row<'a> {
    id: &'a str,
    name: Option<&'a str>,
    slug: Option<&'a str>,
    creator: Option<&'a str>,
    release: Option<&'a str>,
    tps: Option<f64>,
    ttft: Option<f64>,
    scores: BTreeMap<String, f64>,
}

/// 有限で 0 以上の数だけ(カタログの検査が拒否する値を、書き出す前に落とす)。
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && *x >= 0.0)
}

fn parse_row<'a>(catalog: &Catalog, row: &'a Value) -> Option<Row<'a>> {
    let id = row.get("id")?.as_str()?;
    let evals = row.get("evaluations");
    let text = |k: &str| row.get(k).and_then(Value::as_str);
    Some(Row {
        id,
        name: text("name"),
        slug: text("slug"),
        creator: row
            .get("model_creator")
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str),
        release: text("release_date"),
        tps: row.get("median_output_tokens_per_second").and_then(number),
        ttft: row
            .get("median_time_to_first_token_seconds")
            .and_then(number),
        scores: catalog
            .benchmarks
            .keys()
            .filter_map(|bench| Some((bench.clone(), number(evals?.get(bench)?)?)))
            .collect(),
    })
}

fn measurement(
    model: &str,
    effort: &str,
    row: &Row,
    retrieved: &str,
) -> Result<Measurement, String> {
    Ok(Measurement {
        model: model.to_owned(),
        effort: effort.to_owned(),
        aa_name: row
            .name
            .ok_or_else(|| format!("AA の行 {} に name が無い", row.id))?
            .to_owned(),
        aa_slug: row
            .slug
            .ok_or_else(|| format!("AA の行 {} に slug が無い", row.id))?
            .to_owned(),
        retrieved: retrieved.to_owned(),
        release_date: row.release.map(str::to_owned),
        median_output_tokens_per_second: row.tps,
        median_time_to_first_token_seconds: row.ttft,
        scores: row.scores.clone(),
    })
}

/// 名前の比較用(大文字小文字と空白の違いを無視する)。
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 応答の 1 行を、自動登録のモデルへ割り振るための材料。
struct Cand<'a> {
    row: &'a Row<'a>,
    creator: &'a str,
    base: String,
    effort: &'static str,
}

/// 同じモデルにまとめる単位。既に自動登録したモデルは ID で、新しいものは (提供元, 基本名) で。
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Existing(String),
    New(String, String),
}

/// `base` は生成ブロックを空にした手登録だけのカタログ、`prev` は前回の生成ブロック。
pub fn collect(
    base: &Catalog,
    prev: &GeneratedBlock,
    json: &str,
    retrieved: &str,
) -> Result<Report, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("AA の応答が JSON でない: {e}"))?;
    let data = root
        .get("data")
        .and_then(Value::as_array)
        .ok_or("AA の応答に data の配列が無い")?;
    if root.get("status").is_some_and(|s| s.as_u64() != Some(200)) {
        return Err("AA の応答 status が 200 ではない".into());
    }
    if data.is_empty()
        || data.iter().any(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        })
    {
        return Err("AA の応答に行が無いか、行 ID が不正".into());
    }
    let rows: Vec<Row> = data.iter().filter_map(|r| parse_row(base, r)).collect();
    let by_id: BTreeMap<&str, &Row> = rows.iter().map(|r| (r.id, r)).collect();

    let mut conflicts = Vec::new();
    let mut skipped = Vec::new();
    let mut missing = Vec::new();
    let mut retire_candidates = Vec::new();

    // ---- 手登録のモデル: aa_ids の行だけを測る。
    let mut manual_row_ids: BTreeSet<&str> = BTreeSet::new();
    let mut measured: BTreeMap<(String, String), Measurement> = BTreeMap::new();
    let carry = |model: &str, effort: &str| {
        prev.measurements
            .iter()
            .find(|m| m.model == model && m.effort == effort)
            .cloned()
    };
    for model in &base.models {
        if model.aa_ids.is_empty() {
            continue;
        }
        let mut found = 0;
        for effort in &model.efforts {
            let Some(aa_id) = model.aa_ids.get(effort) else {
                continue;
            };
            manual_row_ids.insert(aa_id.as_str());
            match by_id.get(aa_id.as_str()) {
                Some(row) => {
                    found += 1;
                    measured.insert(
                        (model.id.clone(), effort.clone()),
                        measurement(&model.id, effort, row, retrieved)?,
                    );
                }
                None => {
                    missing.push(Missing {
                        model: model.id.clone(),
                        effort: effort.clone(),
                        aa_id: aa_id.clone(),
                    });
                    if let Some(m) = carry(&model.id, effort) {
                        measured.insert((model.id.clone(), effort.clone()), m);
                    }
                }
            }
        }
        if found == 0 {
            retire_candidates.push(model.id.clone());
        }
    }

    // ---- 自動登録の土台: 前回の生成ブロックのうち、手登録と ID が衝突しないもの。
    let manual_model_ids: BTreeSet<&str> = base.models.iter().map(|m| m.id.as_str()).collect();
    let manual_provider_ids: BTreeSet<&str> =
        base.providers.iter().map(|p| p.id.as_str()).collect();
    let prev_models: BTreeMap<&str, &Model> = prev
        .models
        .iter()
        .filter(|m| {
            !manual_model_ids.contains(m.id.as_str())
                && m.aa_ids
                    .values()
                    .all(|id| !manual_row_ids.contains(id.as_str()))
        })
        .map(|m| (m.id.as_str(), m))
        .collect();
    let prev_by_aa: BTreeMap<&str, &str> = prev_models
        .values()
        .flat_map(|m| m.aa_ids.values().map(|id| (id.as_str(), m.id.as_str())))
        .collect();

    // 提供元: 手登録の aa_creator → 前回の自動登録 → 新規。
    let mut creator_provider: BTreeMap<String, String> = BTreeMap::new();
    for p in &base.providers {
        if let Some(c) = &p.aa_creator {
            creator_provider.insert(c.clone(), p.id.clone());
        }
    }
    let mut auto_providers: BTreeMap<String, Provider> = BTreeMap::new();
    for p in &prev.providers {
        if manual_provider_ids.contains(p.id.as_str()) {
            continue;
        }
        let Some(c) = &p.aa_creator else { continue };
        if creator_provider.contains_key(c) {
            continue;
        }
        creator_provider.insert(c.clone(), p.id.clone());
        auto_providers.insert(p.id.clone(), p.clone());
    }

    // ---- 候補の行を選ぶ。
    let since = base.discovery.as_ref().map(|d| d.aa_release_since.as_str());
    let judged: BTreeSet<&str> = base
        .roles
        .values()
        .flat_map(|r| r.benchmarks.iter().map(String::as_str))
        .collect();
    let manual_names: Vec<(String, Option<&str>, &str)> = base
        .models
        .iter()
        .map(|m| {
            let creator = base
                .providers
                .iter()
                .find(|p| p.id == m.provider)
                .and_then(|p| p.aa_creator.as_deref());
            (norm(&m.name), creator, m.name.as_str())
        })
        .collect();
    let mut cands: Vec<Cand> = Vec::new();
    for row in by_id.values() {
        if manual_row_ids.contains(row.id) {
            continue;
        }
        let (Some(creator), Some(name), Some(_)) = (row.creator, row.name, row.slug) else {
            continue;
        };
        let known = prev_by_aa.contains_key(row.id);
        if !known {
            let in_range = match (since, row.release) {
                (Some(since), Some(date)) => date.get(..10).is_some_and(|d| d >= since),
                _ => false,
            };
            let measured_any = judged.iter().any(|b| row.scores.contains_key(*b));
            if !(in_range && measured_any) {
                continue;
            }
        }
        let (base_name, effort) = split_effort(name);
        if !known {
            if let Some((_, _, manual)) = manual_names
                .iter()
                .find(|(n, c, _)| *n == norm(&base_name) && c.is_none_or(|c| c == creator))
            {
                skipped.push(Skipped {
                    creator: creator.to_owned(),
                    name: name.to_owned(),
                    id: row.id.to_owned(),
                    manual_model: (*manual).to_owned(),
                });
                continue;
            }
        }
        cands.push(Cand {
            row,
            creator,
            base: base_name,
            effort,
        });
    }

    // ---- 行をモデルへまとめる。
    // 既知の行は、前回のモデル ID へ(名前が変わっても別モデルにならない)。未知の行は、同じ
    // (提供元, 基本名) の既知の行があればそのモデルへ、無ければ新しいモデルへ。
    let mut by_name: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    let mut known_sorted: Vec<&Cand> = cands
        .iter()
        .filter(|c| prev_by_aa.contains_key(c.row.id))
        .collect();
    known_sorted.sort_by_key(|c| prev_by_aa[c.row.id]);
    for c in known_sorted {
        by_name
            .entry((c.creator, c.base.as_str()))
            .or_insert(prev_by_aa[c.row.id]);
    }
    let mut groups: BTreeMap<Group, Vec<&Cand>> = BTreeMap::new();
    for c in &cands {
        let g = match prev_by_aa.get(c.row.id) {
            Some(mid) => Group::Existing((*mid).to_owned()),
            None => match by_name.get(&(c.creator, c.base.as_str())) {
                Some(mid) => Group::Existing((*mid).to_owned()),
                None => Group::New(c.creator.to_owned(), c.base.clone()),
            },
        };
        groups.entry(g).or_default().push(c);
    }

    let mut taken: BTreeSet<String> = manual_model_ids.iter().map(|s| (*s).to_owned()).collect();
    taken.extend(prev_models.keys().map(|s| (*s).to_owned()));
    let mut provider_taken: BTreeSet<String> = manual_provider_ids
        .iter()
        .map(|s| (*s).to_owned())
        .chain(auto_providers.keys().cloned())
        .collect();
    let mut auto_models: BTreeMap<String, Model> = BTreeMap::new();
    let mut added = Vec::new();

    for (group, members) in &groups {
        // 同じエフォートに複数の行があれば、公開が新しい方を取る。
        let mut per_effort: BTreeMap<&str, Vec<&Cand>> = BTreeMap::new();
        for c in members {
            per_effort.entry(c.effort).or_default().push(c);
        }
        let mut chosen: BTreeMap<&str, &Cand> = BTreeMap::new();
        for (effort, mut list) in per_effort {
            list.sort_by(|a, b| {
                (b.row.release.unwrap_or(""), b.row.id)
                    .cmp(&(a.row.release.unwrap_or(""), a.row.id))
            });
            let label = |c: &Cand| format!("{} ({})", c.row.name.unwrap_or(""), c.row.id);
            if list.len() > 1 {
                conflicts.push(Conflict {
                    creator: list[0].creator.to_owned(),
                    base_name: list[0].base.clone(),
                    effort: effort.to_owned(),
                    kept: label(list[0]),
                    dropped: list[1..].iter().map(|c| label(c)).collect(),
                });
            }
            chosen.insert(effort, list[0]);
        }
        let newest = chosen
            .values()
            .max_by_key(|c| (c.row.release.unwrap_or(""), c.row.id))
            .expect("グループは 1 行以上を持つ");
        let creator = newest.creator;

        let (id, provider, mut aa_ids) = match group {
            Group::Existing(mid) => {
                let p = prev_models[mid.as_str()];
                // 前回の対応のうち、応答に行が残っているものは今回の読み取りで置き換え、
                // 応答に無いものだけを持ち越す。
                let kept: BTreeMap<String, String> = p
                    .aa_ids
                    .iter()
                    .filter(|(_, id)| !by_id.contains_key(id.as_str()))
                    .map(|(e, id)| (e.clone(), id.clone()))
                    .collect();
                (p.id.clone(), p.provider.clone(), kept)
            }
            Group::New(..) => {
                let provider = match creator_provider.get(creator) {
                    Some(p) => p.clone(),
                    None => {
                        let mut pid = slugify(creator);
                        if provider_taken.contains(&pid) {
                            pid = format!("{pid}-aa");
                        }
                        let stem = pid.clone();
                        let mut n = 2;
                        while provider_taken.contains(&pid) {
                            pid = format!("{stem}-{n}");
                            n += 1;
                        }
                        provider_taken.insert(pid.clone());
                        creator_provider.insert(creator.to_owned(), pid.clone());
                        auto_providers.insert(
                            pid.clone(),
                            Provider {
                                id: pid.clone(),
                                name: creator.to_owned(),
                                aa_creator: Some(creator.to_owned()),
                            },
                        );
                        pid
                    }
                };
                let slug = slugify(&newest.base);
                let mut id = slug.clone();
                if taken.contains(&id) {
                    id = format!("{provider}-{slug}");
                }
                let stem = id.clone();
                let mut n = 2;
                while taken.contains(&id) {
                    id = format!("{stem}-{n}");
                    n += 1;
                }
                taken.insert(id.clone());
                added.push(id.clone());
                (id, provider, BTreeMap::new())
            }
        };
        for (effort, c) in &chosen {
            aa_ids.insert((*effort).to_owned(), c.row.id.to_owned());
            measured.insert(
                (id.clone(), (*effort).to_owned()),
                measurement(&id, effort, c.row, retrieved)?,
            );
        }
        let mut efforts: Vec<String> = aa_ids.keys().cloned().collect();
        efforts.sort_by_key(|e| (effort_rank(e), e.clone()));
        auto_models.insert(
            id.clone(),
            Model {
                id,
                provider,
                name: newest.base.clone(),
                status: Status::Active,
                efforts,
                aa_ids,
            },
        );
    }

    // ---- 応答に 1 行も無くなった自動登録のモデルは、`retired` で残す(最後の値を持ち越す)。
    let mut retired = Vec::new();
    for (id, m) in &prev_models {
        if auto_models.contains_key(*id) {
            continue;
        }
        if m.status != Status::Retired {
            retired.push((*id).to_owned());
        }
        let mut kept = (*m).clone();
        kept.status = Status::Retired;
        auto_models.insert(kept.id.clone(), kept);
    }
    // 応答に行が無かったエフォートは、最後の値を持ち越す。
    for m in auto_models.values() {
        for (effort, aa_id) in &m.aa_ids {
            if measured.contains_key(&(m.id.clone(), effort.clone())) {
                continue;
            }
            missing.push(Missing {
                model: m.id.clone(),
                effort: effort.clone(),
                aa_id: aa_id.clone(),
            });
            if let Some(carried) = carry(&m.id, effort) {
                measured.insert((m.id.clone(), effort.clone()), carried);
            }
        }
    }
    missing.sort_by(|a, b| (&a.model, &a.effort).cmp(&(&b.model, &b.effort)));

    // 使っていない自動登録の提供元は残さない。
    let used: BTreeSet<&str> = auto_models.values().map(|m| m.provider.as_str()).collect();
    auto_providers.retain(|id, _| used.contains(id.as_str()));

    // ---- 測定値の並び: 手登録のモデル(ファイルの順)→ 自動登録のモデル(ID 順)。エフォートは段階の順。
    let mut measurements = Vec::new();
    let order: Vec<(&str, &[String])> = base
        .models
        .iter()
        .map(|m| (m.id.as_str(), m.efforts.as_slice()))
        .chain(
            auto_models
                .values()
                .map(|m| (m.id.as_str(), m.efforts.as_slice())),
        )
        .collect();
    for (id, efforts) in order {
        for e in efforts {
            if let Some(m) = measured.remove(&(id.to_owned(), e.clone())) {
                measurements.push(m);
            }
        }
    }
    debug_assert!(measured.is_empty(), "並びに載らなかった測定値がある");

    let opts = root.get("prompt_options");
    Ok(Report {
        meta: AaMeta {
            retrieved: retrieved.to_owned(),
            source: ENDPOINT.to_owned(),
            attribution: ATTRIBUTION.to_owned(),
            prompt_length_tokens: opts
                .and_then(|o| o.get("prompt_length"))
                .and_then(Value::as_u64),
            parallel_queries: opts
                .and_then(|o| o.get("parallel_queries"))
                .and_then(Value::as_u64),
        },
        providers: auto_providers.into_values().collect(),
        models: auto_models.into_values().collect(),
        measurements,
        missing,
        retire_candidates,
        added,
        retired,
        conflicts,
        skipped,
    })
}

fn s(v: &str) -> String {
    toml::Value::String(v.to_owned()).to_string()
}

/// 浮動小数は Debug 形式で書く(整数値にも `.0` が付き、TOML の浮動小数として読める)。
fn f(v: f64) -> String {
    format!("{v:?}")
}

pub fn render_block(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(BEGIN);
    out.push('\n');
    out.push_str("[aa]\n");
    out.push_str(&format!("retrieved = {}\n", s(&report.meta.retrieved)));
    out.push_str(&format!("source = {}\n", s(&report.meta.source)));
    out.push_str(&format!("attribution = {}\n", s(&report.meta.attribution)));
    if let Some(n) = report.meta.prompt_length_tokens {
        out.push_str(&format!("prompt_length_tokens = {n}\n"));
    }
    if let Some(n) = report.meta.parallel_queries {
        out.push_str(&format!("parallel_queries = {n}\n"));
    }
    for p in &report.providers {
        out.push_str("\n[[provider]]\n");
        out.push_str(&format!("id = {}\n", s(&p.id)));
        out.push_str(&format!("name = {}\n", s(&p.name)));
        if let Some(c) = &p.aa_creator {
            out.push_str(&format!("aa_creator = {}\n", s(c)));
        }
    }
    for m in &report.models {
        out.push_str("\n[[model]]\n");
        out.push_str(&format!("id = {}\n", s(&m.id)));
        out.push_str(&format!("provider = {}\n", s(&m.provider)));
        out.push_str(&format!("name = {}\n", s(&m.name)));
        if m.status == Status::Retired {
            out.push_str("status = \"retired\"\n");
        }
        let efforts: Vec<String> = m.efforts.iter().map(|e| s(e)).collect();
        out.push_str(&format!("efforts = [{}]\n", efforts.join(", ")));
        out.push_str("\n[model.aa_ids]\n");
        for e in &m.efforts {
            if let Some(id) = m.aa_ids.get(e) {
                out.push_str(&format!("{} = {}\n", s(e), s(id)));
            }
        }
    }
    for m in &report.measurements {
        out.push_str("\n[[measurement]]\n");
        out.push_str(&format!("model = {}\n", s(&m.model)));
        out.push_str(&format!("effort = {}\n", s(&m.effort)));
        out.push_str(&format!("aa_name = {}\n", s(&m.aa_name)));
        out.push_str(&format!("aa_slug = {}\n", s(&m.aa_slug)));
        out.push_str(&format!("retrieved = {}\n", s(&m.retrieved)));
        if let Some(d) = &m.release_date {
            out.push_str(&format!("release_date = {}\n", s(d)));
        }
        if let Some(v) = m.median_output_tokens_per_second {
            out.push_str(&format!("median_output_tokens_per_second = {}\n", f(v)));
        }
        if let Some(v) = m.median_time_to_first_token_seconds {
            out.push_str(&format!("median_time_to_first_token_seconds = {}\n", f(v)));
        }
        out.push_str("\n[measurement.scores]\n");
        for (bench, v) in &m.scores {
            out.push_str(&format!("{bench} = {}\n", f(*v)));
        }
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// ファイルの生成ブロック(BEGIN〜END の行)を `block` に置き換える。
pub fn splice(file: &str, block: &str) -> Result<String, String> {
    let begin = file
        .find(BEGIN)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの開始行が無い: {BEGIN}"))?;
    let end_at = file[begin..]
        .find(END)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの終了行が無い: {END}"))?
        + begin;
    let mut tail = &file[end_at + END.len()..];
    tail = tail.strip_prefix('\n').unwrap_or(tail);
    Ok(format!("{}{}{}", &file[..begin], block, tail))
}

/// ファイルの生成ブロックの中身(BEGIN〜END の行の間)。
pub fn block_of(file: &str) -> Result<&str, String> {
    let begin = file
        .find(BEGIN)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの開始行が無い: {BEGIN}"))?
        + BEGIN.len();
    let end = file[begin..]
        .find(END)
        .ok_or_else(|| format!("catalog.toml に生成ブロックの終了行が無い: {END}"))?
        + begin;
    Ok(&file[begin..end])
}

/// 取り込みの全体: ファイルの本文と AA の応答から、報告と、書き換え後の本文を作る。
/// 書き換え後の本文は `Catalog::parse` で検査済み(不正なら Err で、書かない)。
pub fn run(text: &str, json: &str, retrieved: &str) -> Result<(Report, String), String> {
    let base = Catalog::parse(&splice(text, &format!("{BEGIN}\n{END}\n"))?)?;
    let prev = GeneratedBlock::parse(block_of(text)?)?;
    let report = collect(&base, &prev, json, retrieved)?;
    let next = splice(text, &render_block(&report))?;
    Catalog::parse(&next).map_err(|e| format!("書き換え後のカタログが不正(書かない): {e}"))?;
    // 取得日だけの更新は差分にしない。実値が変わったときだけ新しい取得日を保存する。
    fn semantic(block: &str) -> Result<toml::Value, String> {
        let mut value: toml::Value = toml::from_str(block).map_err(|e| e.to_string())?;
        if let Some(aa) = value.get_mut("aa").and_then(toml::Value::as_table_mut) {
            aa.remove("retrieved");
        }
        if let Some(rows) = value
            .get_mut("measurement")
            .and_then(toml::Value::as_array_mut)
        {
            for row in rows {
                if let Some(table) = row.as_table_mut() {
                    table.remove("retrieved");
                }
            }
        }
        Ok(value)
    }
    let unchanged = semantic(block_of(text)?)? == semantic(block_of(&next)?)?;
    Ok((report, if unchanged { text.to_owned() } else { next }))
}

#[cfg(test)]
mod tests;
