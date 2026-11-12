//! ja-polish — 日本語の文書を `llm` 経由で Gemini に書き直させる(docs/agents/ja-polish.md)。
//!
//! 使い方:
//!   ja-polish --stdin [--dry-run]               標準入力の本文を書き直して標準出力へ(報告は標準エラー)
//!   ja-polish [--dry-run] [--no-baseline] FILE… ファイルをその場で書き直す
//!   ja-polish --check [--no-baseline] FILE…|-   未通過の日本語の段落を一覧する(あれば終了コード 1)
//!   ja-polish stop-gate                         Stop hook: 標準入力の Stop の入力を読み、通していない
//!                                               文書があれば {"decision":"block"} を出す(常に終了コード 0)
//!   ja-polish bench LIST [--variants A,B,…]     固定の見本(LIST の `<commit>:<path>`)で、モデル × 思考量の
//!                                               組(`model` か `model@level`)ごとに時間と質を測り、表で出す。
//!                                               何も書き換えず、記録もしない(docs/agents/ja-polish.md)
//!
//! 書き直しは、基点(git なら origin の既定ブランチとの merge-base)に無い段落、つまり追加・
//! 変更した段落だけが対象。基点が取れないときは文書全体が対象だが、一度通した段落は記録で
//! 分かるので書き換えない。通せなかった段落は元のまま残し(素通し)、理由を報告する。
//! 終了コードは、素通しがあっても 0(止めない設計。後から知らせる経路は gate イベント)。

use ja_polish::app::{self, Env};
use ja_polish::llm::LlmCli;
use ja_polish::textlint::{Fixer, NoFixer, Textlint};
use ja_polish::{baseline, bench, config, polish, records};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "使い方:
  ja-polish --stdin [--dry-run]
  ja-polish [--dry-run] [--no-baseline] FILE…
  ja-polish --check [--no-baseline] FILE…|-
  ja-polish stop-gate   (Stop hook。標準入力の JSON を読む)
  ja-polish bench LIST [--variants model[@level],…]   (固定の見本で測る。書き換えない)";

#[derive(Default)]
struct Opts {
    stdin: bool,
    check: bool,
    dry_run: bool,
    no_baseline: bool,
    inputs: Vec<String>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    for a in args {
        match a.as_str() {
            "--stdin" => o.stdin = true,
            "--check" => o.check = true,
            "--dry-run" => o.dry_run = true,
            "--no-baseline" => o.no_baseline = true,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("知らない引数です: {s}")),
            s => o.inputs.push(s.to_string()),
        }
    }
    if o.stdin && (o.check || !o.inputs.is_empty()) {
        return Err("--stdin はほかの引数と一緒に使えません".to_string());
    }
    if !o.stdin && o.inputs.is_empty() {
        return Err("ファイルか --stdin を指定してください".to_string());
    }
    Ok(o)
}

const STOP_SPEC: agent_session::entry::StopSpec = agent_session::entry::StopSpec {
    name: "ja-polish",
    agents: &[agent_session::Agent::Claude, agent_session::Agent::Codex],
    reentry: ja_polish::stop::REENTRY,
};

fn read_stdin() -> std::io::Result<String> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("stop-gate") {
        // hook: 判定不能・入力が読めないときは何も出さず通す。
        return agent_session::entry::run_stop(&STOP_SPEC, ja_polish::stop::hook);
    }
    if args.first().map(String::as_str) == Some("bench") {
        return bench_command(&args[1..]);
    }
    let opts = match parse(&args) {
        Ok(o) => o,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("ja-polish: {msg}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let mut cfg = config::load().unwrap_or_default();
    if let Some(m) = std::env::var("JA_POLISH_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
    {
        cfg.model = m;
    }
    let env = Env::from_environment(cfg);

    if opts.check {
        return check(&env, &opts);
    }

    // textlint の自動修正。設定で切ってあれば何も直さない。`textlint` コマンドが無いときは、実行時に
    // 入力のまま返す(止めない)。
    let textlint = Textlint::from_config(&env.config);
    let fixer: &dyn Fixer = match &textlint {
        Some(t) => t,
        None => &NoFixer,
    };

    if opts.stdin {
        let text = match read_stdin() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("ja-polish: 標準入力を読めません: {e}");
                return ExitCode::from(1);
            }
        };
        let result = app::run_polish(&env, &text, None, &LlmCli, fixer, opts.dry_run);
        print!("{}", result.text);
        if !result.paras.is_empty() {
            eprint!("{}", app::report("標準入力", &result));
        }
        return ExitCode::SUCCESS;
    }

    let mut failed = false;
    for input in &opts.inputs {
        let path = PathBuf::from(input);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("ja-polish: {input} を読めません: {e}");
                failed = true;
                continue;
            }
        };
        let base = if opts.no_baseline {
            None
        } else {
            baseline::baseline_for(&path)
        };
        let result = app::run_polish(&env, &text, base.as_deref(), &LlmCli, fixer, opts.dry_run);
        if result.paras.is_empty() {
            println!("ja-polish: {input}: 書き直す日本語の段落はありません");
            continue;
        }
        if !opts.dry_run && result.text != text {
            if let Err(e) = std::fs::write(&path, &result.text) {
                eprintln!("ja-polish: {input} に書けません: {e}");
                failed = true;
                continue;
            }
        }
        print!("{}", app::report(input, &result));
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `bench` の引数: 一覧のファイルと、`--variants`(カンマ区切りの `model` か `model@level`)。
fn parse_bench(args: &[String]) -> Result<(String, Option<Vec<String>>), String> {
    let mut list = None;
    let mut variants = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--variants" => {
                let v = it.next().ok_or("--variants に値がありません")?;
                variants = Some(
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                );
            }
            s if s.starts_with("--") => return Err(format!("知らない引数です: {s}")),
            s if list.is_none() => list = Some(s.to_string()),
            s => return Err(format!("見本の一覧は 1 つだけ指定してください: {s}")),
        }
    }
    let list = list.ok_or("見本の一覧のファイルを指定してください")?;
    if variants.as_ref().is_some_and(Vec::is_empty) {
        return Err("--variants が空です".to_string());
    }
    Ok((list, variants))
}

/// 既定の組: 今の設定のモデルを、思考量の指定なし・minimal・low で。予備のモデルは、指定なしで 1 つ。
fn default_variants(cfg: &config::Config) -> Vec<bench::Variant> {
    let mut v = vec![
        bench::Variant {
            model: cfg.model.clone(),
            thinking: cfg.thinking_level.clone(),
        },
        bench::Variant {
            model: cfg.model.clone(),
            thinking: Some("minimal".to_string()),
        },
        bench::Variant {
            model: cfg.model.clone(),
            thinking: Some("low".to_string()),
        },
    ];
    if let Some(fb) = &cfg.fallback_model {
        v.push(bench::Variant {
            model: fb.clone(),
            thinking: None,
        });
    }
    let mut seen = Vec::new();
    v.retain(|x| {
        let dup = seen.contains(x);
        if !dup {
            seen.push(x.clone());
        }
        !dup
    });
    v
}

fn git_show(spec: &str) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["show", spec])
        .output()
        .map_err(|e| format!("git を起動できません: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn bench_command(args: &[String]) -> ExitCode {
    let (list_path, variants) = match parse_bench(args) {
        Ok(x) => x,
        Err(msg) => {
            eprintln!("ja-polish bench: {msg}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let cfg = config::load().unwrap_or_default();
    let list = match std::fs::read_to_string(&list_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ja-polish bench: {list_path} を読めません: {e}");
            return ExitCode::from(1);
        }
    };
    let samples = match bench::load_samples(&list, &git_show) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ja-polish bench: {e}");
            return ExitCode::from(1);
        }
    };
    let style = match ja_polish::style::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ja-polish bench: {e}");
            return ExitCode::from(1);
        }
    };
    let variants: Vec<bench::Variant> = match variants {
        Some(v) => v.iter().map(|s| bench::Variant::parse(s)).collect(),
        None => default_variants(&cfg),
    };
    // 指摘数を数えるためだけに、textlint を使う。無ければ「未計測」。
    let textlint = Textlint::from_config(&cfg);
    let counter: &dyn Fixer = match &textlint {
        Some(t) => t,
        None => &NoFixer,
    };

    eprintln!(
        "ja-polish bench: 見本 {} 本、組 {} 個を、順に測ります(1 組ごとに数十秒〜数分)",
        samples.len(),
        variants.len()
    );
    let mut rows = Vec::new();
    for (i, v) in variants.iter().enumerate() {
        eprintln!("[{}/{}] {} …", i + 1, variants.len(), v.label());
        let row = bench::measure(v, &cfg, &samples, &style, &LlmCli, counter);
        eprintln!(
            "[{}/{}] {}: {:.0} 秒、書き直し {} / 素通し {}",
            i + 1,
            variants.len(),
            row.label,
            row.secs,
            row.polished,
            row.skipped
        );
        rows.push(row);
    }
    print!("{}", bench::render_table(&rows, &samples));
    ExitCode::SUCCESS
}

fn check(env: &Env, opts: &Opts) -> ExitCode {
    let known = env
        .records_path
        .as_deref()
        .map(records::load)
        .unwrap_or_default();
    let mut any = false;
    for input in &opts.inputs {
        let (text, base) = if input == "-" {
            match read_stdin() {
                Ok(t) => (t, None),
                Err(e) => {
                    eprintln!("ja-polish: 標準入力を読めません: {e}");
                    return ExitCode::from(1);
                }
            }
        } else {
            let path = PathBuf::from(input);
            match std::fs::read_to_string(&path) {
                Ok(t) => {
                    let base = if opts.no_baseline {
                        None
                    } else {
                        baseline::baseline_for(&path)
                    };
                    (t, base)
                }
                Err(e) => {
                    eprintln!("ja-polish: {input} を読めません: {e}");
                    return ExitCode::from(1);
                }
            }
        };
        for p in polish::pending(&text, base.as_deref(), &known) {
            any = true;
            let head: String = p.text.chars().take(40).collect();
            println!("未通過: {input} 段落 {}: {head}", p.index + 1);
        }
    }
    if any {
        ExitCode::from(1)
    } else {
        println!("未通過の日本語の段落はありません");
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_accepts_the_three_shapes() {
        assert!(parse(&v(&["--stdin"])).unwrap().stdin);
        let o = parse(&v(&["--dry-run", "--no-baseline", "a.md", "b.md"])).unwrap();
        assert!(o.dry_run && o.no_baseline);
        assert_eq!(o.inputs, vec!["a.md", "b.md"]);
        let o = parse(&v(&["--check", "-"])).unwrap();
        assert!(o.check && o.inputs == vec!["-"]);
    }

    #[test]
    fn parse_bench_reads_the_list_and_variants() {
        let (l, v) = parse_bench(&v(&["samples.txt"])).unwrap();
        assert_eq!((l.as_str(), v), ("samples.txt", None));
        let (_, v) = parse_bench(&v_(&["s.txt", "--variants", "a@low, b ,"])).unwrap();
        assert_eq!(v, Some(vec!["a@low".to_string(), "b".to_string()]));
        assert!(parse_bench(&v_(&[])).is_err());
        assert!(parse_bench(&v_(&["a", "b"])).is_err());
        assert!(parse_bench(&v_(&["a", "--variants"])).is_err());
        assert!(parse_bench(&v_(&["a", "--variants", ","])).is_err());
        assert!(parse_bench(&v_(&["a", "--nope"])).is_err());
    }

    fn v_(a: &[&str]) -> Vec<String> {
        v(a)
    }

    #[test]
    fn default_variants_are_unique_and_include_the_fallback() {
        let mut c = config::Config::default();
        let d = default_variants(&c);
        assert_eq!(
            d.iter().map(bench::Variant::label).collect::<Vec<_>>(),
            vec![
                "gemini-3.6-flash@既定",
                "gemini-3.6-flash@minimal",
                "gemini-3.6-flash@low",
                "gemini-flash-lite-latest@既定"
            ]
        );
        // 設定で既に minimal なら、同じ組を重ねない。
        c.thinking_level = Some("minimal".into());
        c.fallback_model = None;
        let d = default_variants(&c);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn parse_rejects_bad_combinations() {
        assert!(parse(&v(&[])).is_err());
        assert!(parse(&v(&["--stdin", "a.md"])).is_err());
        assert!(parse(&v(&["--bogus", "a.md"])).is_err());
        assert!(parse(&v(&["--stdin", "--check"])).is_err());
    }
}
