//! model-catalog — カタログ(catalog.toml)の検査と、AA の測定値の取り込み。
//!
//! ```text
//! model-catalog check [--file PATH]
//! model-catalog fetch-aa [--file PATH] [--input JSON] [--retrieved YYYY-MM-DD] [--write]
//! ```
//!
//! `fetch-aa` は既定では書かず、差分の要約だけを出す。`--write` は `--file` の生成ブロックだけを
//! 置き換える。鍵は `ARTIFICIAL_ANALYSIS_API_KEY`、無ければ login keyring(`secret-tool`、
//! application=model-catalog credential=artificial-analysis-api-key)から読み、標準入力で curl に渡す。

use model_catalog::aa::{self, Report};
use model_catalog::Catalog;
use std::io::Write;
use std::process::{Command, ExitCode, Stdio};

const KEY_ENV: &str = "ARTIFICIAL_ANALYSIS_API_KEY";

fn usage() -> ExitCode {
    eprintln!(
        "usage: model-catalog check [--file PATH]\n       model-catalog fetch-aa [--file PATH] [--input JSON] [--retrieved YYYY-MM-DD] [--write]"
    );
    ExitCode::from(2)
}

struct Opts {
    file: Option<String>,
    input: Option<String>,
    retrieved: Option<String>,
    write: bool,
}

fn parse_opts(args: impl Iterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        file: None,
        input: None,
        retrieved: None,
        write: false,
    };
    let mut it = args;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => o.write = true,
            "--file" => o.file = Some(it.next().ok_or("--file に値が無い")?),
            "--input" => o.input = Some(it.next().ok_or("--input に値が無い")?),
            "--retrieved" => o.retrieved = Some(it.next().ok_or("--retrieved に値が無い")?),
            other => return Err(format!("未知の引数: {other}")),
        }
    }
    Ok(o)
}

fn load(file: &Option<String>) -> Result<(Catalog, String), String> {
    let text = match file {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        None => model_catalog::CATALOG_TOML.to_owned(),
    };
    Ok((Catalog::parse(&text)?, text))
}

fn api_key() -> Result<String, String> {
    if let Ok(k) = std::env::var(KEY_ENV) {
        if !k.trim().is_empty() {
            return Ok(k.trim().to_owned());
        }
    }
    let out = Command::new("secret-tool")
        .args([
            "lookup",
            "application",
            "model-catalog",
            "credential",
            "artificial-analysis-api-key",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("secret-tool を起動できない: {e}"))?;
    let key = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if key.is_empty() {
        return Err(format!(
            "AA の鍵が無い。環境変数 {KEY_ENV} か、login keyring(secret-tool store --label='model-catalog Artificial Analysis API key' application model-catalog credential artificial-analysis-api-key)に入れる"
        ));
    }
    Ok(key)
}

/// 鍵は argv に出さず、curl の設定を標準入力で渡す(`ps` に鍵が見えない)。
fn fetch_live() -> Result<String, String> {
    let key = api_key()?;
    let mut child = Command::new("curl")
        .args(["-sS", "-f", "--max-time", "60", "-K", "-", aa::ENDPOINT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("curl を起動できない: {e}"))?;
    let config = format!(
        "header = \"x-api-key: {}\"\n",
        key.replace('\\', "\\\\").replace('"', "\\\"")
    );
    child
        .stdin
        .take()
        .ok_or("curl の標準入力を取れない")?
        .write_all(config.as_bytes())
        .map_err(|e| format!("curl へ書けない: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("curl の待機に失敗: {e}"))?;
    if !out.status.success() {
        return Err(format!("AA の取得に失敗(curl {})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("応答が UTF-8 でない: {e}"))
}

fn print_report(report: &Report, before: &Catalog) {
    println!(
        "AA 取得 {}: 測定 {} 行(取得前 {} 行)、提供元 {} 社・モデル {} 件を自動登録、プロンプト長 {:?} token",
        report.meta.retrieved,
        report.measurements.len(),
        before.measurements.len(),
        report.providers.len(),
        report.models.len(),
        report.meta.prompt_length_tokens
    );
    let changed = report
        .measurements
        .iter()
        .filter(|m| before.measurement(&m.model, &m.effort) != Some(*m))
        .count();
    println!("  値が変わった・増えた行: {changed}");
    for id in &report.added {
        println!("  新しく自動登録したモデル: {id}");
    }
    for id in &report.retired {
        println!("  応答に 1 行も無くなり retired にしたモデル: {id}");
    }
    for m in &report.missing {
        println!(
            "  応答に無い行(最後の値を持ち越す): {} / {} (AA の行 ID {})",
            m.model, m.effort, m.aa_id
        );
    }
    for id in &report.retire_candidates {
        println!(
            "  手登録のモデルで、応答に 1 行も無い(status = \"retired\" にするか判断する): {id}"
        );
    }
    for c in &report.conflicts {
        println!(
            "  同じ (提供元, 基本名, エフォート) の行が複数: [{}] {} / {} は {} を取り、{} を取らなかった",
            c.creator,
            c.base_name,
            c.effort,
            c.kept,
            c.dropped.join("、")
        );
    }
    for s in &report.skipped {
        println!(
            "  手登録の「{}」と同名の別の行(取り込まない): [{}] {} ID {}",
            s.manual_model, s.creator, s.name, s.id
        );
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        return Ok(usage());
    };
    match cmd.as_str() {
        "check" => {
            let o = parse_opts(args)?;
            let (c, _) = load(&o.file)?;
            println!(
                "OK: 提供元 {}・モデル {}・ベンチ {}・測定 {} 行",
                c.providers.len(),
                c.models.len(),
                c.benchmarks.len(),
                c.measurements.len()
            );
            Ok(ExitCode::SUCCESS)
        }
        "fetch-aa" => {
            let o = parse_opts(args)?;
            if o.write && o.file.is_none() {
                return Err(
                    "--write には --file が要る(埋め込みのカタログは書き換えられない)".into(),
                );
            }
            let (catalog, text) = load(&o.file)?;
            let json = match &o.input {
                Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
                None => fetch_live()?,
            };
            let retrieved = o.retrieved.unwrap_or_else(model_catalog::date::today_utc);
            let (report, next) = aa::run(&text, &json, &retrieved)?;
            print_report(&report, &catalog);
            if o.write {
                let path = o.file.as_deref().unwrap_or_default();
                std::fs::write(path, next).map_err(|e| format!("{path}: {e}"))?;
                println!("書き込み: {path}");
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Ok(usage()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("model-catalog: {e}");
            ExitCode::from(1)
        }
    }
}
