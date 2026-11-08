//! ja-polish — 日本語の文書を `llm` 経由で Gemini に書き直させる(docs/agents/ja-polish.md)。
//!
//! 使い方:
//!   ja-polish --stdin [--dry-run]               標準入力の本文を書き直して標準出力へ(報告は標準エラー)
//!   ja-polish [--dry-run] [--no-baseline] FILE… ファイルをその場で書き直す
//!   ja-polish --check [--no-baseline] FILE…|-   未通過の日本語の段落を一覧する(あれば終了コード 1)
//!
//! 書き直しは、基点(git なら origin の既定ブランチとの merge-base)に無い段落、つまり追加・
//! 変更した段落だけが対象。基点が取れないときは文書全体が対象だが、一度通した段落は記録で
//! 分かるので書き換えない。通せなかった段落は元のまま残し(素通し)、理由を報告する。
//! 終了コードは、素通しがあっても 0(止めない設計。後から知らせる経路は gate イベント)。

use ja_polish::app::{self, Env};
use ja_polish::llm::LlmCli;
use ja_polish::{baseline, config, polish, records};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "使い方:
  ja-polish --stdin [--dry-run]
  ja-polish [--dry-run] [--no-baseline] FILE…
  ja-polish --check [--no-baseline] FILE…|-";

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

fn read_stdin() -> std::io::Result<String> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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

    if opts.stdin {
        let text = match read_stdin() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("ja-polish: 標準入力を読めません: {e}");
                return ExitCode::from(1);
            }
        };
        let result = app::run_polish(&env, &text, None, &LlmCli, opts.dry_run);
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
            baseline::git_baseline(&path)
        };
        let result = app::run_polish(&env, &text, base.as_deref(), &LlmCli, opts.dry_run);
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
                        baseline::git_baseline(&path)
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
    fn parse_rejects_bad_combinations() {
        assert!(parse(&v(&[])).is_err());
        assert!(parse(&v(&["--stdin", "a.md"])).is_err());
        assert!(parse(&v(&["--bogus", "a.md"])).is_err());
        assert!(parse(&v(&["--stdin", "--check"])).is_err());
    }
}
