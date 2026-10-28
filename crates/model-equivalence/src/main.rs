//! model-equivalence — 乗り換え先の同等性を、役ごとに根拠付きで出す。
//!
//! ```text
//! model-equivalence lens [--days N] [--owners a,b] [--out PATH] [--list-orgs]
//! model-equivalence compare --reference MODEL:EFFORT [--role ID] [--lens PATH] [--verbose]
//!                           [--margin-points N] [--speed-margin R]
//! model-equivalence render [--lens PATH] [--out PATH] [--plan-ref MODEL:EFFORT] [--execute-ref MODEL:EFFORT] [--fragment]
//! ```
//!
//! `lens` は gh CLI で、自分の merge 済み PR のタイトルだけを読み、種類ごとの件数と期間だけを書く
//! (タイトルもリポジトリ名も残さない)。既定の対象は自分のアカウントだけで、組織は `--owners` で
//! 明示したものだけを足す(会社の業務リポジトリを巻き込まないため)。

use model_catalog::{data, date};
use model_equivalence::judge::{compare, Margins, Verdict};
use model_equivalence::lens::Lens;
use model_equivalence::render::{build_data, render_document, render_fragment, RenderOptions};
use std::process::{Command, ExitCode};

fn usage() -> ExitCode {
    eprintln!(
        "usage: model-equivalence lens [--days N] [--owners a,b] [--out PATH] [--list-orgs]\n       \
         model-equivalence compare --reference MODEL:EFFORT [--role ID] [--lens PATH] [--verbose] \
         [--margin-points N] [--speed-margin R]\n       model-equivalence render [--lens PATH] [--out PATH] [--plan-ref MODEL:EFFORT] [--execute-ref MODEL:EFFORT] [--fragment]"
    );
    ExitCode::from(2)
}

fn gh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh を起動できない: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh {} が失敗: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("gh の出力が UTF-8 でない: {e}"))
}

struct Flags {
    pairs: Vec<(String, Option<String>)>,
}

impl Flags {
    fn parse(
        args: impl Iterator<Item = String>,
        valued: &[&str],
        bare: &[&str],
    ) -> Result<Flags, String> {
        let mut pairs = Vec::new();
        let mut it = args;
        while let Some(a) = it.next() {
            if bare.contains(&a.as_str()) {
                pairs.push((a, None));
            } else if valued.contains(&a.as_str()) {
                let v = it.next().ok_or_else(|| format!("{a} に値が無い"))?;
                pairs.push((a, Some(v)));
            } else {
                return Err(format!("未知の引数: {a}"));
            }
        }
        Ok(Flags { pairs })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == name)
    }
}

fn default_lens_path() -> String {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/state"))
        })
        .unwrap_or_else(|| ".".into());
    format!("{base}/model-equivalence/lens.json")
}

fn cmd_lens(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let f = Flags::parse(args, &["--days", "--owners", "--out"], &["--list-orgs"])?;
    if f.has("--list-orgs") {
        let orgs = gh(&["api", "user/orgs", "--jq", ".[].login"])?;
        println!("{}", orgs.trim());
        return Ok(ExitCode::SUCCESS);
    }
    let days: u32 = f
        .get("--days")
        .map(|d| d.parse().map_err(|_| format!("--days が数でない: {d}")))
        .transpose()?
        .unwrap_or(365);
    let owners: Vec<String> = match f.get("--owners") {
        Some(list) => list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        None => vec![gh(&["api", "user", "--jq", ".login"])?.trim().to_owned()],
    };
    if owners.is_empty() {
        return Err("--owners が空".into());
    }
    // 履歴が多いと、検索の上限(1 クエリ 1000 件)で切り詰められて、新しい側に偏る。
    // 7 日ごとの範囲に区切って数え、どれかが上限に達したらエラーにする(切り詰めた集計は使わない)。
    let mut titles = Vec::new();
    let window = 7u32;
    let mut calls = 0;
    for owner in &owners {
        let mut ago = 0;
        while ago < days {
            let from = date::days_ago_utc((ago + window - 1).min(days - 1));
            let to = date::days_ago_utc(ago);
            if calls > 0 {
                // 検索 API は 1 分 30 回まで。
                std::thread::sleep(std::time::Duration::from_millis(2200));
            }
            calls += 1;
            let json = gh(&[
                "search",
                "prs",
                "--author",
                "@me",
                "--merged",
                "--owner",
                owner,
                "--merged-at",
                &format!("{from}..{to}"),
                "--limit",
                "1000",
                "--json",
                "title",
            ])?;
            let rows: Vec<serde_json::Value> =
                serde_json::from_str(&json).map_err(|e| format!("gh の出力が JSON でない: {e}"))?;
            if rows.len() >= 1000 {
                return Err(format!(
                    "{from}..{to} の PR が検索の上限 1000 件に達した。区切りを短くしないと全件を数えられない"
                ));
            }
            titles.extend(
                rows.iter()
                    .filter_map(|r| r.get("title")?.as_str().map(str::to_owned)),
            );
            ago += window;
        }
    }
    let lens = Lens::build(titles.iter().map(String::as_str), &date::today_utc(), days);
    lens.validate(data())?;
    let out = f.get("--out").map_or_else(default_lens_path, str::to_owned);
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&lens).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&out, text).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "対象 {} 件のアカウント・組織、直近 {days} 日のマージ済み PR {} 件",
        owners.len(),
        lens.total
    );
    for (id, cat) in &data().categories {
        let n = lens.counts.get(id).copied().unwrap_or(0);
        println!(
            "  {:<14} {:>4} 件 ({:>4.1}%)",
            cat.name,
            n,
            lens.share(id) * 100.0
        );
    }
    println!("書き込み: {out}(件数と期間だけ。タイトル・リポジトリ名は残さない)");
    Ok(ExitCode::SUCCESS)
}

fn cmd_compare(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let f = Flags::parse(
        args,
        &[
            "--reference",
            "--role",
            "--lens",
            "--margin-points",
            "--speed-margin",
        ],
        &["--verbose"],
    )?;
    let c = data();
    let reference = f
        .get("--reference")
        .ok_or("--reference MODEL:EFFORT が要る")?;
    let (model, effort) = reference
        .split_once(':')
        .ok_or_else(|| format!("--reference は MODEL:EFFORT の形: {reference}"))?;
    let mut margins = Margins::from_catalog(c);
    if let Some(v) = f.get("--margin-points") {
        margins.capability_points = v
            .parse()
            .map_err(|_| format!("--margin-points が数でない: {v}"))?;
    }
    if let Some(v) = f.get("--speed-margin") {
        margins.speed_ratio = v
            .parse()
            .map_err(|_| format!("--speed-margin が数でない: {v}"))?;
    }
    if !(margins.capability_points > 0.0 && margins.speed_ratio > 0.0) {
        return Err("許容幅は 0 より大きい値".into());
    }
    let lens = match f.get("--lens") {
        Some(p) => Some(Lens::from_json(
            &std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
            c,
        )?),
        None => None,
    };
    let roles: Vec<&str> = match f.get("--role") {
        Some(r) => vec![r],
        None => c.roles.keys().map(String::as_str).collect(),
    };
    let verbose = f.has("--verbose");
    let aa = c.aa.as_ref().map(|a| a.retrieved.as_str()).unwrap_or("?");
    println!(
        "基準: {model} / {effort}   許容幅: 能力 ±{}pt・速さ ±{:.0}%   履歴: {}   データ: Artificial Analysis {aa}",
        margins.capability_points,
        margins.speed_ratio * 100.0,
        lens.as_ref().map_or("なし(役の全ベンチ)".to_owned(), |l| format!(
            "{} 件({}、直近 {} 日)",
            l.total, l.generated, l.days
        )),
    );
    for role in roles {
        let mut rows = compare(c, role, (model, effort), lens.as_ref(), margins)?;
        let rank = |v: Verdict| match v {
            Verdict::Equivalent => 0,
            Verdict::Stronger => 1,
            Verdict::Insufficient => 2,
            Verdict::Weaker => 3,
        };
        rows.sort_by_key(|r| (rank(r.judgement.verdict), r.model.clone(), r.effort.clone()));
        let r = &c.roles[role];
        println!("\n■ {}({})", r.name, role);
        for row in &rows {
            let j = &row.judgement;
            let detail: Vec<String> = j
                .judgements
                .iter()
                .map(|x| format!("{}:{}", x.label, x.verdict.label()))
                .collect();
            println!(
                "  {:<8} {:<20} {:<7} {}",
                j.verdict.label(),
                row.model_name,
                row.effort,
                detail.join(" / ")
            );
            if verbose {
                for x in &j.judgements {
                    println!("      - {}: {}", x.label, x.basis);
                }
            }
        }
        if let Some(first) = rows.first() {
            for u in &first.judgement.uncovered {
                println!(
                    "  ※ 履歴の {:.0}% を占める「{}」は、現行モデルに値のある公開ベンチで測れていない",
                    u.share * 100.0,
                    u.name
                );
            }
            if let Some(n) = &first.judgement.note {
                println!("  ※ {n}");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_render(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let f = Flags::parse(
        args,
        &["--lens", "--out", "--plan-ref", "--execute-ref"],
        &["--fragment"],
    )?;
    let c = data();
    let lens = match f.get("--lens") {
        Some(p) => Some(Lens::from_json(
            &std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
            c,
        )?),
        None => None,
    };
    let mut opts = RenderOptions::default();
    if let Some(v) = f.get("--plan-ref") {
        opts.plan_reference = v.to_owned();
    }
    if let Some(v) = f.get("--execute-ref") {
        opts.execute_reference = v.to_owned();
    }
    let data_json = build_data(c, lens.as_ref(), &opts)?;
    let html = if f.has("--fragment") {
        render_fragment(&data_json)
    } else {
        render_document(&data_json)
    };
    let out = f.get("--out").map_or_else(
        || default_lens_path().replace("lens.json", "page.html"),
        str::to_owned,
    );
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&out, &html).map_err(|e| format!("{out}: {e}"))?;
    // 近さの地図の警告: 共通の成分が少なく距離を出せない組・地図に載せなかった行・歪みの大きい地図。
    if let Some(roles) = data_json["map"]["roles"].as_object() {
        for (role, m) in roles {
            for set in m["sets"].as_array().into_iter().flatten() {
                let (excluded, filled) = (set["excluded"].as_u64(), set["filled_pairs"].as_u64());
                let stress = set["stress"].as_f64().unwrap_or(0.0);
                if excluded > Some(0)
                    || filled > Some(0)
                    || stress > model_equivalence::map::STRESS_WARN
                {
                    eprintln!(
                        "警告: 地図({role}、許容幅 {})は、{} 行を載せず、{} 組の距離を最短経路で補った(共通の成分が {} 本未満)。stress {stress}",
                        set["margins"],
                        excluded.unwrap_or(0),
                        filled.unwrap_or(0),
                        model_equivalence::map::MIN_COMMON,
                    );
                }
            }
        }
    }
    println!(
        "書き込み: {out}({} KB。履歴: {})",
        html.len() / 1024,
        if lens.is_some() { "あり" } else { "なし" }
    );
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("lens") => cmd_lens(args),
        Some("compare") => cmd_compare(args),
        Some("render") => cmd_render(args),
        _ => return usage(),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("model-equivalence: {e}");
            ExitCode::from(1)
        }
    }
}
