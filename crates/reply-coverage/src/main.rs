//! reply-coverage — 相手のいる会話への返信が、相手の未決の依頼すべてに応答しているかを検査する
//! (docs/agents/reply-coverage.md)。
//!
//! 使い方:
//!   reply-coverage import --id <媒体:id> --file <ファイル> [--format raw|normalized] [--message-id <id>]…
//!                                     会話の全文を保存する(ブラウザの snapshot など)
//!   reply-coverage import-gmail --file <get_thread の結果の JSON>
//!                                     get_thread の結果がファイルに保存されたとき、正規の形に直して保存する
//!   reply-coverage check --thread <媒体:id> --draft <ファイル> [--model <m>] [--me <手がかり>]
//!                                     下書きが、会話の相手の依頼すべてに応答しているかを検査し、記録する
//!                                     (終了コード 0=合格、1=不合格、2=検査できない)
//!   reply-coverage verify --thread <媒体:id> --draft <ファイル>
//!                                     検査に合格した最終稿かを記録で照会する(0=合格、1=不合格・未検査)
//!   reply-coverage ready --thread <媒体:id>
//!                                     返信案を作る・本人に見せる前の確認: 会話の全文が保存済みで 24 時間以内か
//!                                     (媒体の通知メールの会話は、ブラウザで読んだ会話の取り込みもあるか)。0=読み込み済み、1=足りない
//!   reply-coverage bench --cases <dir>… [--variants m1,m2] [--repeat N] [--me <手がかり>]
//!                                     見本でモデルごとの質と時間を測る(書き換えず、記録もしない)
//!   reply-coverage sweep              期限(60 日)を過ぎた会話の全文と記録を消す

use reply_coverage::backend::ClaudeCli;
use reply_coverage::store::{self, Store, StoredThread};
use reply_coverage::{bench, check, gate, verify};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "使い方:
  reply-coverage import --id <媒体:id> --file <ファイル> [--format raw|normalized] [--message-id <id>]…
  reply-coverage import-gmail --file <get_thread の結果の JSON>
  reply-coverage check --thread <媒体:id> --draft <ファイル> [--model <m>] [--me <手がかり>]
  reply-coverage verify --thread <媒体:id> --draft <ファイル>
  reply-coverage ready --thread <媒体:id>
  reply-coverage bench --cases <dir>… [--variants m1,m2] [--repeat N] [--me <手がかり>]
  reply-coverage sweep";

/// 引数の解析結果: 1 回だけの `--flag value` の表と、何回でも指定できる `--flag value` の並び。
struct Args {
    single: std::collections::HashMap<String, String>,
    multi: Vec<(String, String)>,
}

/// `--flag value` の取り出し。知らない引数は理由つきで失敗にする。
fn parse(args: &[String], single: &[&str], multi: &[&str]) -> Result<Args, String> {
    let mut out = Args {
        single: std::collections::HashMap::new(),
        multi: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let name = a
            .strip_prefix("--")
            .ok_or(format!("知らない引数です: {a}"))?;
        let val = it
            .next()
            .ok_or(format!("--{name} に値がありません"))?
            .clone();
        if single.contains(&name) {
            if out.single.insert(name.to_string(), val).is_some() {
                return Err(format!("--{name} は 1 回だけ指定できます"));
            }
        } else if multi.contains(&name) {
            out.multi.push((name.to_string(), val));
        } else {
            return Err(format!("知らない引数です: {a}"));
        }
    }
    Ok(out)
}

fn need<'a>(
    m: &'a std::collections::HashMap<String, String>,
    k: &str,
) -> Result<&'a String, String> {
    m.get(k).ok_or(format!("--{k} が要ります"))
}

fn open_store() -> Result<Store, String> {
    Store::from_env().ok_or("状態の置き場を決められません(HOME が無い)".to_string())
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path} を読めません: {e}"))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let (cmd, rest) = args.split_first().ok_or("")?;
    match cmd.as_str() {
        "import" => {
            let Args {
                single: s,
                multi: m,
            } = parse(rest, &["id", "file", "format"], &["message-id"])?;
            let format = s.get("format").map(String::as_str).unwrap_or("raw");
            if format != "raw" && format != "normalized" {
                return Err("--format は raw か normalized です".to_string());
            }
            let text = read(need(&s, "file")?)?;
            if text.trim().is_empty() {
                return Err("会話の全文が空です".to_string());
            }
            let t = StoredThread {
                id: need(&s, "id")?.clone(),
                fetched_ts: store::now(),
                format: format.to_string(),
                message_ids: m.into_iter().map(|(_, v)| v).collect(),
                text,
            };
            open_store()?
                .save_thread(&t)
                .map_err(|e| format!("保存できません: {e}"))?;
            println!("保存しました: {}({} 字)", t.id, t.text.chars().count());
            Ok(ExitCode::SUCCESS)
        }
        "import-gmail" => {
            // `get_thread` の結果が大きいと、ツールの結果がファイルに保存され、hook は応答を受け取れない。
            // そのファイル(JSON)を、hook と同じ関数で正規の形に直して保存する(エージェントが書き写さない)。
            let Args { single: s, .. } = parse(rest, &["file"], &[])?;
            let text = read(need(&s, "file")?)?;
            let response: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("JSON として読めません: {e}"))?;
            let id = response["id"]
                .as_str()
                .ok_or("会話の id(トップレベルの id)がありません")?
                .to_string();
            let store = open_store()?;
            match gate::snapshot_from_get_thread(
                &store,
                &serde_json::json!({ "threadId": id }),
                &response,
                store::now(),
            ) {
                Some(saved) => {
                    println!("保存しました: {saved}");
                    Ok(ExitCode::SUCCESS)
                }
                None => Err("get_thread の応答として読めません(messages が無い)".to_string()),
            }
        }
        "check" => {
            let Args { single: s, .. } = parse(rest, &["thread", "draft", "model", "me"], &[])?;
            let store = open_store()?;
            let id = need(&s, "thread")?;
            let thread = store.load_thread(id).ok_or(format!(
                "会話 {id} の全文が保存されていません(先に会話を取得・import してください)"
            ))?;
            let draft = read(need(&s, "draft")?)?;
            let model = s
                .get("model")
                .cloned()
                .unwrap_or_else(check::model_from_env);
            match check::run_check(
                &store,
                &ClaudeCli,
                &thread,
                &draft,
                &model,
                s.get("me").map(String::as_str),
                store::now(),
            ) {
                Ok((v, _)) => {
                    print!("{}", check::report(id, &v));
                    Ok(if v.pass {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    })
                }
                Err(e) => {
                    eprintln!("reply-coverage: 検査できませんでした: {e}");
                    Ok(ExitCode::from(2))
                }
            }
        }
        "verify" => {
            let Args { single: s, .. } = parse(rest, &["thread", "draft"], &[])?;
            let store = open_store()?;
            let id = need(&s, "thread")?;
            let draft = read(need(&s, "draft")?)?;
            let thread = store.load_thread(id);
            match verify::verify(&store, thread.as_ref(), &draft, store::now()) {
                Ok(()) => Ok(ExitCode::SUCCESS),
                Err(f) => {
                    eprintln!("{}", f.message(id));
                    Ok(ExitCode::from(1))
                }
            }
        }
        "ready" => {
            let Args { single: s, .. } = parse(rest, &["thread"], &[])?;
            let store = open_store()?;
            let id = need(&s, "thread")?;
            let thread = store.load_thread(id);
            match verify::thread_ready(&store, thread.as_ref(), store::now()) {
                Ok(()) => {
                    println!("ready: 会話 {id} の全文は取り込み済みです(24 時間以内)");
                    Ok(ExitCode::SUCCESS)
                }
                Err(f) => {
                    eprintln!("{}", f.message(id));
                    Ok(ExitCode::from(1))
                }
            }
        }
        "bench" => {
            let Args {
                single: s,
                multi: m,
            } = parse(rest, &["variants", "me", "repeat"], &["cases"])?;
            let mut dirs: Vec<PathBuf> = m.into_iter().map(|(_, v)| PathBuf::from(v)).collect();
            if let Some(d) = process_env::var_nonempty("REPLY_COVERAGE_PRIVATE_CASES") {
                dirs.push(PathBuf::from(d));
            }
            if dirs.is_empty() {
                return Err("--cases(か REPLY_COVERAGE_PRIVATE_CASES)が要ります".to_string());
            }
            let mut cases = Vec::new();
            for d in &dirs {
                cases.extend(bench::load_cases(d)?);
            }
            let models: Vec<String> = match s.get("variants") {
                Some(v) => v
                    .split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect(),
                None => vec![check::model_from_env()],
            };
            let cwd = open_store()?
                .cwd()
                .map_err(|e| format!("作業ディレクトリを作れません: {e}"))?;
            eprintln!(
                "{} 件の見本 × {} モデルを測ります(1 件ごとに数秒〜数十秒)",
                cases.len(),
                models.len()
            );
            let repeat: usize = match s.get("repeat") {
                Some(v) => v
                    .parse()
                    .ok()
                    .filter(|n| (1..=10).contains(n))
                    .ok_or("--repeat は 1〜10 の整数です")?,
                None => 1,
            };
            let rows = bench::run(
                &cases,
                &models,
                &ClaudeCli,
                &cwd,
                s.get("me").map(String::as_str),
                repeat,
            );
            print!("{}", bench::render(&rows));
            Ok(ExitCode::SUCCESS)
        }
        "sweep" => {
            let removed = open_store()?.sweep(store::now(), store::TTL_SECS);
            println!("期限切れの会話を {removed} 件消しました");
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" => Err(String::new()),
        other => Err(format!("知らないサブコマンドです: {other}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("reply-coverage: {msg}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
