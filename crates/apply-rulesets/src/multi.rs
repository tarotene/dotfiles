//! `github-rulesets-apply` — 1 つ以上のリポジトリの宣言済み ruleset
//! (`.github/rulesets/{security,quality,workflow}[,review].json`)を、リポジトリ
//! ごとに `apply-rulesets` を呼んで適用する薄い複数リポジトリ・ループ。
//! `scripts/github-rulesets-apply` の Rust 移植(ADR-0024、#414)。
//!
//! ADR-503 以前は、対象リポジトリの「型」(rust/typst/astro/core/dotfiles)を引数に
//! 取り、型ごとに別の apply-rulesets.sh(`*-repo-governance` skill 側、
//! プレースホルダ置換入り)を呼び分ける 5 型構成だった。正本を対象リポジトリ自身の
//! `.github/rulesets/*.json` に一本化した結果、型を問わず同じ `apply-rulesets` を
//! 呼べば足りる(D4「還元」)。
//!
//! 差分:
//! - `--selftest` は持たない(テストは `tests/*.rs`)。
//! - 呼び先は、既定では自分の隣の `apply-rulesets`(同じ `dotfiles-tools` の
//!   `bin/`)。bash 版は `$SCRIPT_DIR/apply-rulesets.sh` を `bash` 経由で呼んで
//!   いた。`GITHUB_RULESETS_APPLY_SELF_SCRIPT` による差し替え(テスト用)は
//!   そのまま残し、こちらは実行権のあるファイルを直接 exec する。

use std::path::PathBuf;
use std::process::Command;

const USAGE: &str = "\
usage: github-rulesets-apply [--ref REF] [--reconcile] [--dry-run]
                              [--unverified-contexts] <owner/repo>...
                              [-- <passthrough args>]
       github-rulesets-apply --selftest

対象リポジトリごとに apply-rulesets.sh <owner/repo> を呼び、その
リポジトリ自身の .github/rulesets/*.json 宣言を live な branch ruleset に
適用する。

--ref/--reconcile/--dry-run/--unverified-contexts は全 repo に共通で渡す。
`--` 以降は apply-rulesets.sh へそのまま渡す(例: `-- --verify-sha <sha>`)。

失敗した repo があっても他の repo の適用は続け、最後にまとめて報告する
(1 つの失敗が全体を止めない)。呼び出し自体が不正な場合、または失敗した
repo が 1 つでもあれば exit 1。
";

fn self_apply_script() -> PathBuf {
    if let Some(p) = std::env::var_os("GITHUB_RULESETS_APPLY_SELF_SCRIPT").filter(|p| !p.is_empty())
    {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.canonicalize().ok())
        .and_then(|e| e.parent().map(|d| d.join("apply-rulesets")))
        .unwrap_or_else(|| PathBuf::from("apply-rulesets"))
}

fn apply_one(script: &PathBuf, args: &[String]) -> bool {
    if !hook_io::proc::command_exists(&script.to_string_lossy()) {
        eprintln!(
            "ERROR: {} not found or not executable (home-manager 未配備の可能性)",
            script.display()
        );
        return false;
    }
    Command::new(script)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `github-rulesets-apply` の入口。戻り値は終了コード。
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--selftest") => {
            eprintln!(
                "github-rulesets-apply: --selftest は `cargo test -p apply-rulesets` に移った(#414)"
            );
            return 0;
        }
        Some("--help") | Some("-h") => {
            print!("{USAGE}");
            return 0;
        }
        _ => {}
    }

    let mut reference = String::new();
    let (mut reconcile, mut dry_run, mut unverified) = (false, false, false);
    let mut repos: Vec<String> = Vec::new();
    let mut passthrough: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ref" => match it.next() {
                Some(v) => reference = v.clone(),
                None => {
                    eprintln!("ERROR: --ref requires a value");
                    return 1;
                }
            },
            "--reconcile" => reconcile = true,
            "--dry-run" => dry_run = true,
            "--unverified-contexts" => unverified = true,
            "--" => {
                passthrough = it.by_ref().cloned().collect();
                break;
            }
            other if other.starts_with('-') => {
                eprintln!("Unknown option: {other}");
                eprint!("{USAGE}");
                return 2;
            }
            other => {
                if !other.contains('/') {
                    eprintln!("ERROR: '{other}' is not owner/repo");
                    return 2;
                }
                repos.push(other.to_string());
            }
        }
    }
    if repos.is_empty() {
        eprintln!("ERROR: at least one owner/repo is required");
        eprint!("{USAGE}");
        return 2;
    }

    let mut common: Vec<String> = Vec::new();
    if !reference.is_empty() {
        common.extend(["--ref".to_string(), reference]);
    }
    if reconcile {
        common.push("--reconcile".into());
    }
    if dry_run {
        common.push("--dry-run".into());
    }
    if unverified {
        common.push("--unverified-contexts".into());
    }

    let script = self_apply_script();
    let mut failed: Vec<&String> = Vec::new();
    for r in &repos {
        let mut call = vec![r.clone()];
        call.extend(common.iter().cloned());
        call.extend(passthrough.iter().cloned());
        if !apply_one(&script, &call) {
            failed.push(r);
        }
    }

    if !failed.is_empty() {
        eprintln!();
        eprintln!("FAILED ({}):", failed.len());
        for r in failed {
            eprintln!("  {r}");
        }
        return 1;
    }
    0
}
