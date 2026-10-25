//! kiriban — Issue / PR 番号のキリ番の判定と文面(docs/kiriban.md)。
//! 判定と文面は lib.rs。`.github/actions/kiriban` がこのバイナリを build して呼ぶ。
//!
//! 使い方:
//!   kiriban classify <N>      kiriban=… / classes=… / request=… を 1 行ずつ
//!   kiriban labels            name<TAB>color<TAB>description を 1 ラベル 1 行
//!   kiriban comment      --number N --kind issue|pr --getter LOGIN --getter-type TYPE
//!                        --owner OWNER --repo OWNER/REPO [--request-issue M --expires DATE]
//!   kiriban request-title --number N
//!   kiriban request-body  --number N --kind … --getter … --getter-type … --owner … --repo … --expires DATE
//!
//! 終了コード: 0 = 成功, 1 = 指定の番号がキリ番でない(comment / request-*), 2 = 使い方の誤り。

use kiriban::{
    classify, is_bot, is_date_shape, is_safe_name, opens_request, render_comment,
    render_request_body, request_title, Class, Ctx, Kind, Request, LABELS,
};
use std::collections::HashMap;
use std::process::ExitCode;

const USAGE: &str = "usage: kiriban <classify N | labels | comment … | request-title --number N | request-body …>\n";

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("kiriban: {msg}");
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn parse_number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || s.starts_with('0') {
        return None;
    }
    s.parse().ok()
}

/// `--key value` の並びを読む。未知のキー・値の欠落は `Err`。
fn parse_opts(args: &[String], allowed: &[&str]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let Some(key) = k.strip_prefix("--") else {
            return Err(format!("想定外の引数: {k}"));
        };
        if !allowed.contains(&key) {
            return Err(format!("未知のオプション: {k}"));
        }
        let Some(v) = it.next() else {
            return Err(format!("{k} の値がありません"));
        };
        m.insert(key.to_string(), v.clone());
    }
    Ok(m)
}

struct Subject {
    number: u64,
    kind: Kind,
    getter: String,
    bot: bool,
    owner: String,
    nwo: String,
}

fn subject(o: &HashMap<String, String>) -> Result<Subject, String> {
    let get = |k: &str| o.get(k).ok_or_else(|| format!("--{k} が必要です"));
    let number = parse_number(get("number")?).ok_or("--number は正の整数")?;
    let kind = Kind::parse(get("kind")?).ok_or("--kind は issue か pr")?;
    let getter = get("getter")?.clone();
    let owner = get("owner")?.clone();
    let nwo = get("repo")?.clone();
    for (k, v) in [("getter", &getter), ("owner", &owner), ("repo", &nwo)] {
        if !is_safe_name(v) {
            return Err(format!("--{k} に使えない文字があります"));
        }
    }
    let bot = is_bot(&getter, get("getter-type")?);
    Ok(Subject {
        number,
        kind,
        getter,
        bot,
        owner,
        nwo,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((sub, rest)) = args.split_first() else {
        return usage_error("サブコマンドがありません");
    };

    match sub.as_str() {
        "--help" | "-h" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        "classify" => {
            let [n] = rest else {
                return usage_error("classify は番号を 1 つ取ります");
            };
            let Some(n) = parse_number(n) else {
                return usage_error("番号は正の整数");
            };
            let c = classify(n);
            let list: Vec<&str> = c.iter().map(|c: &Class| c.name()).collect();
            println!("kiriban={}", !c.is_empty());
            println!("classes={}", list.join(","));
            println!("request={}", opens_request(&c));
            ExitCode::SUCCESS
        }
        "labels" => {
            for (name, color, desc) in LABELS {
                println!("{name}\t{color}\t{desc}");
            }
            ExitCode::SUCCESS
        }
        "request-title" => {
            let o = match parse_opts(rest, &["number"]) {
                Ok(o) => o,
                Err(e) => return usage_error(&e),
            };
            let Some(n) = o.get("number").and_then(|s| parse_number(s)) else {
                return usage_error("--number は正の整数");
            };
            match request_title(n, &classify(n)) {
                Some(t) => {
                    println!("{t}");
                    ExitCode::SUCCESS
                }
                None => ExitCode::from(1),
            }
        }
        "comment" | "request-body" => {
            let allowed: &[&str] = if sub == "comment" {
                &[
                    "number",
                    "kind",
                    "getter",
                    "getter-type",
                    "owner",
                    "repo",
                    "request-issue",
                    "expires",
                ]
            } else {
                &[
                    "number",
                    "kind",
                    "getter",
                    "getter-type",
                    "owner",
                    "repo",
                    "expires",
                ]
            };
            let o = match parse_opts(rest, allowed) {
                Ok(o) => o,
                Err(e) => return usage_error(&e),
            };
            let s = match subject(&o) {
                Ok(s) => s,
                Err(e) => return usage_error(&e),
            };
            let expires = o.get("expires").map(String::as_str);
            if let Some(d) = expires {
                if !is_date_shape(d) {
                    return usage_error("--expires は YYYY-MM-DD");
                }
            }
            let ctx = Ctx {
                number: s.number,
                kind: s.kind,
                getter: &s.getter,
                bot: s.bot,
                owner: &s.owner,
                nwo: &s.nwo,
            };
            let classes = classify(s.number);
            let out = if sub == "comment" {
                let req = match (o.get("request-issue"), expires) {
                    (Some(m), Some(d)) => match parse_number(m) {
                        Some(number) => Some(Request { number, expires: d }),
                        None => return usage_error("--request-issue は正の整数"),
                    },
                    (None, None) => None,
                    _ => return usage_error("--request-issue と --expires は組で渡します"),
                };
                render_comment(&ctx, &classes, req.as_ref())
            } else {
                let Some(d) = expires else {
                    return usage_error("--expires が必要です");
                };
                render_request_body(&ctx, &classes, d)
            };
            match out {
                Some(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                None => ExitCode::from(1),
            }
        }
        other => usage_error(&format!("未知のサブコマンド: {other}")),
    }
}
