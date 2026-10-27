//! pr-checks-wait — PR の CI を「赤 1 件で即 NG、required 全緑で即 OK」で待つ CLI。
//!
//! 判定は Stop の G_CI と同じ [`ci::wait`] を使う(判定の正本は 1 か所)。
//! `gh pr checks --watch` は使わない: `--required --watch --fail-fast` は、集約ジョブ
//! (`CI passed`)が全ジョブの完了後にしか結果を出さないので、先に落ちたジョブの NG が
//! 遅れ、`--required` を外すと required でないチェックの完了まで待つ(docs/agents/pr-gate.md)。
//!
//! 終了コード: 0 = OK、1 = NG、8 = 時間切れで pending(gh の pending と同じ値)、
//! 2 = 判定できない(チェックが揃わない・API 失敗・引数の誤り)。結果は stdout に出す
//! (`run_in_background` の完了通知にそのまま載る)。

use crate::config::Config;
use crate::gates::ci::{self, Status};
use crate::{gh, repo};
use std::path::Path;
use std::time::{Duration, Instant};

pub const EXIT_OK: i32 = 0;
pub const EXIT_NG: i32 = 1;
pub const EXIT_UNKNOWN: i32 = 2;
pub const EXIT_PENDING: i32 = 8;

/// `--timeout` の既定(秒)。Bash の `run_in_background` の既定(30 分)より先に自分で終わる。
pub const DEFAULT_TIMEOUT_SECS: u64 = 1500;

const USAGE: &str = "usage: pr-checks-wait [<PR>] [-R OWNER/REPO] [--timeout SECS]
  CI を待つ。赤 1 件で即 NG(exit 1)、required が全緑になったら即 OK(exit 0)。
  時間切れで pending のままなら exit 8、判定できなければ exit 2。
  <PR> を省くと現在のブランチの PR。--timeout の既定は 1500 秒。";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    pr: Option<String>,
    repo: Option<String>,
    timeout: u64,
    help: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        pr: None,
        repo: None,
        timeout: DEFAULT_TIMEOUT_SECS,
        help: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{name} に値がありません"))
        };
        match key {
            "-h" | "--help" => a.help = true,
            "-R" | "--repo" => a.repo = Some(value("-R")?),
            "--timeout" => {
                a.timeout = value("--timeout")?
                    .parse()
                    .map_err(|_| "--timeout は秒数(整数)で指定してください".to_string())?;
            }
            k if k.starts_with('-') => return Err(format!("不明なオプション: {k}")),
            _ if a.pr.is_none() => a.pr = Some(arg.clone()),
            _ => return Err(format!("PR が複数指定されています: {arg}")),
        }
    }
    Ok(a)
}

fn join_names(names: &[String]) -> String {
    names.join(", ")
}

/// 結果を (終了コード, 出力) にする。`cwd` は `-R` を省いたときのリポジトリ検出に使う。
pub fn execute(args: &[String], cwd: &Path, cfg: &Config) -> (i32, String) {
    let unknown = |msg: String| (EXIT_UNKNOWN, format!("UNKNOWN: {msg}"));
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return unknown(format!("{e}\n{USAGE}")),
    };
    if a.help {
        return (EXIT_OK, USAGE.to_string());
    }
    let Some(nwo) = a.repo.clone().or_else(|| repo::owner_repo(cwd)) else {
        return unknown(
            "リポジトリを特定できません。-R OWNER/REPO を付けるか、GitHub の remote を持つ\
             リポジトリで実行してください"
                .to_string(),
        );
    };
    let Some(pr) = a.pr.clone().or_else(|| gh::current_pr_number(&nwo)) else {
        return unknown(format!(
            "PR を特定できません。{nwo} の PR 番号を渡すか、PR のあるブランチで実行してください"
        ));
    };
    let Some(base) = gh::pr_base(&pr, &nwo) else {
        return unknown(format!("PR {pr} の base ブランチを取得できません({nwo})"));
    };
    // -R で別のリポジトリを指すときは、手元の origin/HEAD は当てにならない。
    let default_base = if a.repo.is_some() {
        gh::default_branch(&nwo)
    } else {
        git_repository::default_branch(cwd).or_else(|| gh::default_branch(&nwo))
    };

    let started = Instant::now();
    let w = ci::wait(
        cfg,
        &nwo,
        &pr,
        &base,
        default_base.as_deref(),
        Duration::from_secs(a.timeout),
    );
    let secs = started.elapsed().as_secs();
    let n = ci::reported_count(&w.reported);
    let still_running = ci::pending_names(&w.reported, &[]);
    let o = &w.outcome;
    match o.status {
        Status::Pass => {
            let mut out = format!(
                "OK: PR #{pr} — 期待するチェックがすべて通りました({n} 件が報告済み、{secs} 秒)"
            );
            if !o.detail.is_empty() {
                out.push_str(&format!("\n{}", o.detail));
            }
            if !still_running.is_empty() {
                out.push_str(&format!(
                    "\nrequired でないチェックは待っていません(pending: {})",
                    join_names(&still_running)
                ));
            }
            (EXIT_OK, out)
        }
        Status::Failed => {
            let mut out = format!("NG: PR #{pr} — チェックが赤です({secs} 秒)\n\n");
            out.push_str(&ci::render_failed(&w.reported));
            if !still_running.is_empty() {
                out.push_str(&format!(
                    "\n\nまだ実行中(待たずに終えます): {}",
                    join_names(&still_running)
                ));
            }
            (EXIT_NG, out)
        }
        Status::Pending => {
            let waiting = ci::pending_names(&w.reported, &w.expected);
            (
                EXIT_PENDING,
                format!(
                    "PENDING: PR #{pr} — {} 秒待っても完了しません(pending: {})。\
                     もう一度待つか、`gh pr checks {pr} --required` で現在の状態を読んでください",
                    a.timeout,
                    join_names(&waiting)
                ),
            )
        }
        Status::Missing => unknown(format!(
            "PR #{pr} — 期待する required チェックが報告されていません: {}",
            o.detail
        )),
        Status::Empty => unknown(format!("PR #{pr} — チェックが 1 件も報告されていません")),
        Status::ApiFailure => unknown(format!(
            "required チェック集合の取得に失敗しました。\
             `gh api repos/{nwo}/rules/branches/{base}` で確認してください"
        )),
    }
}

/// bin の入口。
pub fn run(args: &[String]) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (code, out) = execute(args, &cwd, &Config::from_env());
    if code == EXIT_UNKNOWN && out.contains("usage:") {
        eprintln!("{out}");
    } else {
        println!("{out}");
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_forms() {
        let a = parse(&v(&["12", "-R", "o/r", "--timeout", "30"])).unwrap();
        assert_eq!(
            (a.pr.as_deref(), a.repo.as_deref(), a.timeout),
            (Some("12"), Some("o/r"), 30)
        );
        let a = parse(&v(&["--repo=o/r", "--timeout=5"])).unwrap();
        assert_eq!((a.pr, a.repo.as_deref(), a.timeout), (None, Some("o/r"), 5));
        assert_eq!(parse(&[]).unwrap().timeout, DEFAULT_TIMEOUT_SECS);
        assert!(parse(&v(&["-h"])).unwrap().help);
    }

    #[test]
    fn rejects_bad_args() {
        assert!(parse(&v(&["--timeout", "x"])).is_err());
        assert!(parse(&v(&["--timeout"])).is_err());
        assert!(parse(&v(&["--watch"])).is_err());
        assert!(parse(&v(&["1", "2"])).is_err());
    }
}
