//! herdr-issue-counts [--dry-run]
//!
//! herdr の各 workspace について、そのリポジトリの open Issue 数(PR を除く)を
//! workspace metadata の `$issues` トークンとして報告する。systemd --user /
//! launchd のタイマー(home/modules/herdr.nix)から 5 分毎に呼ばれる。
//! パースと組み立ては lib.rs(`cargo test` で検証)にあり、このファイルは
//! herdr / git / gh の起動と exit code だけを担う。
use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

use herdr_issue_counts::{
    alias, build_query, exit_code, format_token, parse_counts, parse_workspace_list,
    repo_from_remotes, Outcome, Repo, SOURCE, TOKEN, TTL_MS,
};

const USAGE: &str = "usage: herdr-issue-counts [--dry-run]\n\n\
Reports each Herdr workspace's open GitHub issue count (pull requests\n\
excluded) as the `$issues` workspace-metadata token, shown in the sidebar's\n\
space rows (config/herdr/config.toml). One GraphQL request covers every\n\
repository. Values expire after 15 minutes if not refreshed.\n\n\
--dry-run: print `workspace_id<TAB>owner/repo<TAB>count` instead of reporting.\n\n\
Exit code reflects delivery: 0 = reported, or skipped because Herdr is not\n\
running / gh is not authenticated / no workspace has a GitHub remote;\n\
1 = the GraphQL request or a report failed.";

fn main() -> ExitCode {
    let mut dry_run = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown option: {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::from(exit_code(run(dry_run)))
}

fn run(dry_run: bool) -> Outcome {
    // herdr 未起動(サーバが無い)は恒久状態になり得るので skip 扱い。
    let list = match output("herdr", &["workspace", "list"]) {
        Ok(out) => out,
        Err(msg) => {
            eprintln!("herdr-issue-counts: herdr workspace list に失敗(skip): {msg}");
            return Outcome::Skipped;
        }
    };
    let workspaces = match parse_workspace_list(&list) {
        Ok(w) => w,
        Err(msg) => {
            eprintln!("herdr-issue-counts: {msg}");
            return Outcome::Failed;
        }
    };

    // workspace → リポジトリ。同じ repo_root の git は 1 回だけ呼ぶ。
    let mut root_repo: BTreeMap<&str, Option<Repo>> = BTreeMap::new();
    for w in &workspaces {
        root_repo.entry(w.repo_root.as_str()).or_insert_with(|| {
            output("git", &["-C", &w.repo_root, "remote", "-v"])
                .ok()
                .and_then(|text| repo_from_remotes(&text))
        });
    }
    let mut repos: Vec<Repo> = root_repo.values().flatten().cloned().collect();
    repos.sort();
    repos.dedup();
    if repos.is_empty() {
        eprintln!("herdr-issue-counts: GitHub リポジトリの workspace が無い(skip)");
        return Outcome::Skipped;
    }

    // 未認証は恒久状態になり得る(fresh machine)ので skip 扱い。
    if let Err(msg) = output("gh", &["auth", "status"]) {
        eprintln!("herdr-issue-counts: gh が未認証(skip): {msg}");
        return Outcome::Skipped;
    }

    let query = build_query(&repos);
    let counts = match graphql(&query) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("herdr-issue-counts: {msg}");
            return Outcome::Failed;
        }
    };
    let repo_count: BTreeMap<&Repo, u64> = repos
        .iter()
        .enumerate()
        .filter_map(|(i, r)| match counts.get(&alias(i)) {
            Some(Ok(n)) => Some((r, *n)),
            Some(Err(reason)) => {
                eprintln!("herdr-issue-counts: {r} を取得できない(skip): {reason}");
                None
            }
            None => {
                eprintln!("herdr-issue-counts: {r} が応答に無い(skip)");
                None
            }
        })
        .collect();

    let seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        .to_string();
    let ttl = TTL_MS.to_string();
    let mut outcome = Outcome::Reported;
    for w in &workspaces {
        let Some(repo) = root_repo.get(w.repo_root.as_str()).and_then(Option::as_ref) else {
            continue;
        };
        let Some(&n) = repo_count.get(repo) else {
            continue;
        };
        if dry_run {
            println!("{}\t{repo}\t{n}", w.workspace_id);
            continue;
        }
        let token = format!("{TOKEN}={}", format_token(n));
        // herdr 0.8.2 の CLI は workspace ID を先に置かないと `--source <ID>`
        // の値を未知のオプションとして弾く(2026-09-24 実測)。
        if let Err(msg) = output(
            "herdr",
            &[
                "workspace",
                "report-metadata",
                &w.workspace_id,
                "--source",
                SOURCE,
                "--token",
                &token,
                "--seq",
                &seq,
                "--ttl-ms",
                &ttl,
            ],
        ) {
            eprintln!(
                "herdr-issue-counts: {} への報告に失敗: {msg}",
                w.workspace_id
            );
            outcome = Outcome::Failed;
        }
    }
    outcome
}

/// `gh api graphql` を実行する。部分エラー(一部リポジトリが解決できない)では
/// gh は非 0 で終わるが stdout に `data` が入るので、終了コードではなく
/// 応答の中身で判断する(lib.rs `parse_counts`)。
fn graphql(query: &str) -> Result<BTreeMap<String, Result<u64, String>>, String> {
    let q = format!("query={query}");
    let out = Command::new("gh")
        .args(["api", "graphql", "-f", &q])
        .output()
        .map_err(|e| format!("gh の実行に失敗: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_counts(&stdout).map_err(|e| {
        let stderr = String::from_utf8_lossy(&out.stderr);
        match stderr
            .lines()
            .next()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            Some(line) => format!("gh api graphql に失敗: {line}"),
            None => e,
        }
    })
}

/// コマンドを実行し、成功なら stdout を返す。失敗時は stderr の先頭行
/// (無ければ stdout の先頭行 — herdr はエラーも JSON で stdout に出す)を返す。
fn output(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd} の実行に失敗: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        return Err(if line.is_empty() {
            format!("終了コード {}", out.status)
        } else {
            line
        });
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{cmd} の出力が UTF-8 でない: {e}"))
}
