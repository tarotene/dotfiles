//! pr-hygiene-check — 詳細は lib.rs と docs/agents/pr-hygiene-check.md。
//!
//! 使い方: `pr-hygiene-check [--json] [--owner <持ち主>]...`
//!
//! 終了コード: 0 = すべて clean、1 = 汚れあり、2 = 判定できない(確かな汚れが無く、判定できないものだけが残った。
//! 持ち主の表・`gh` の失敗・引数の誤りも 2)。

use pr_hygiene_check::{render_json, render_table, run, RealGh, Retry};
use repo_scope::{load_policy, machine_side, Verdict};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "使い方: pr-hygiene-check [--json] [--owner <持ち主>]...
  自分の open PR の汚れ(衝突・済んだコミットの混入・base の消失・merge commit・遅れ)を見つける。見つけるだけで、直さない。
  母集団は、この機体の契約の側(dotfiles-identity)と同じ側の持ち主だけ(持ち主の表 ~/.config/dotfiles/github-owners.toml)。
  別の側の持ち主には問い合わせない。--owner は、その中で絞るときだけ使える(別の側の持ち主は拒否する)。
  終了コード: 0 = clean、1 = 汚れあり、2 = 判定できない(または失敗)";

fn fail(msg: &str) -> ExitCode {
    eprintln!("pr-hygiene-check: {msg}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut json = false;
    let mut requested: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--json" => json = true,
            "--owner" => match args.next() {
                Some(v) => requested.push(v),
                None => return fail("--owner に値が無い"),
            },
            other => return fail(&format!("知らない引数: {other}\n{USAGE}")),
        }
    }

    let side = machine_side();
    let owners: Vec<String> = match load_policy(side) {
        Ok(Some(policy)) => {
            for o in &requested {
                if let Verdict::Deny(_) = policy.check_owner(o) {
                    return fail(&format!(
                        "`{o}` は、この機体の契約の側({})の持ち主ではないので、問い合わせない(docs/agents/repo-scope-guard.md)",
                        policy.side.as_str()
                    ));
                }
            }
            if requested.is_empty() {
                let same: Vec<String> = policy
                    .same_side_owners()
                    .into_iter()
                    .map(String::from)
                    .collect();
                if same.is_empty() {
                    return fail("持ち主の表に、この機体の側の持ち主が無い");
                }
                same
            } else {
                requested
            }
        }
        // 持ち主の表を配っていない機体: 規則が無いので、指定された持ち主、無ければログイン中のアカウント自身。
        Ok(None) => {
            if requested.is_empty() {
                match github_command::exec::gh_ok(&["api", "user", "--jq", ".login"]) {
                    Some(login) if !login.is_empty() => vec![login],
                    _ => {
                        return fail(
                            "持ち主の表が無く、ログイン名も引けなかった(--owner で指定する)",
                        )
                    }
                }
            } else {
                requested
            }
        }
        Err(e) => return fail(&e),
    };

    let retry = Retry {
        times: 3,
        delay: Duration::from_secs(2),
    };
    match run(&RealGh, &owners, retry, &std::thread::sleep) {
        Ok(report) => {
            if json {
                println!("{}", render_json(&report));
            } else {
                print!("{}", render_table(&report));
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => fail(&e),
    }
}
