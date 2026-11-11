// 使い方: browser-settings <read|paste|verify|login|list> [<service>] [--file F] [--profile DIR] [--services FILE]
// 保存の確定は人が行う。paste は入力欄に文を入れて読み戻すだけで、保存ボタンは押さない。

use browser_settings::{
    chrome, compare, default_profile_dir, default_services_file, parse_services, Verdict,
};
use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage: browser-settings <command> [<service>] [--file F] [--profile DIR] [--services FILE]
  list                         services.toml のサービスの名前を出す
  login  <service>             窓を出して設定画面を開く。ログインして Enter で閉じる(専用プロファイルに残る)
  read   <service>             項目の現在の値を標準出力に出す
  paste  <service> --file F    項目に F の文を入れて、読み戻した値が F と一致するかを報告する(保存はしない)
  verify <service> --file F    項目の現在の値が F と一致するかを報告する(終了コード 0 一致、1 不一致、2 失敗)";

fn die(msg: &str, code: i32) -> ! {
    eprintln!("browser-settings: {msg}");
    exit(code)
}

fn report(expected: &str, actual: &str) -> i32 {
    match compare(expected, actual) {
        Verdict::Match => {
            println!("一致");
            0
        }
        Verdict::Mismatch {
            line,
            expected,
            actual,
        } => {
            println!("不一致: {line} 行目\n  期待: {expected}\n  実際: {actual}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos: Vec<&str> = Vec::new();
    let (mut file, mut profile, mut services): (Option<&str>, Option<&str>, Option<&str>) =
        (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--file" => file = it.next().map(String::as_str),
            "--profile" => profile = it.next().map(String::as_str),
            "--services" => services = it.next().map(String::as_str),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => pos.push(other),
        }
    }
    let Some(cmd) = pos.first().copied() else {
        die(USAGE, 2)
    };
    let services_path = services
        .map(PathBuf::from)
        .or_else(default_services_file)
        .unwrap_or_else(|| die("services.toml の場所を決められない(--services を渡す)", 2));
    let text = std::fs::read_to_string(&services_path)
        .unwrap_or_else(|e| die(&format!("{} を読めない: {e}", services_path.display()), 2));
    let all = parse_services(&text).unwrap_or_else(|e| die(&e, 2));
    if cmd == "list" {
        for name in all.keys() {
            println!("{name}");
        }
        return;
    }
    let Some(name) = pos.get(1).copied() else {
        die(USAGE, 2)
    };
    let service = all.get(name).unwrap_or_else(|| {
        die(
            &format!("サービス `{name}` が services.toml に無い(list で一覧)"),
            2,
        )
    });
    let profile = profile
        .map(PathBuf::from)
        .or_else(default_profile_dir)
        .unwrap_or_else(|| die("プロファイルの場所を決められない(--profile を渡す)", 2));
    let want = || -> String {
        let f = file.unwrap_or_else(|| die("--file が要る", 2));
        std::fs::read_to_string(f).unwrap_or_else(|e| die(&format!("{f} を読めない: {e}"), 2))
    };
    let code = match cmd {
        "login" => {
            eprintln!("窓でログインを済ませて、この端末で Enter を押すと閉じます(Cookie は専用プロファイルに残ります)");
            chrome::login(&profile, service, || {
                let mut s = String::new();
                let _ = std::io::stdin().read_line(&mut s);
            })
            .map_or_else(|e| die(&e, 2), |()| 0)
        }
        "read" => match chrome::read(&profile, service).unwrap_or_else(|e| die(&e, 2)) {
            Some(v) => {
                println!("{v}");
                0
            }
            None => die(&format!("項目 `{}` が見つからない", service.field), 2),
        },
        "paste" => {
            let text = want();
            let got = chrome::paste(&profile, service, &text)
                .unwrap_or_else(|e| die(&e, 2))
                .unwrap_or_default();
            eprintln!("入力欄に入れました。保存は人が行います(この CLI は保存しません)");
            report(&text, &got)
        }
        "verify" => {
            let text = want();
            let got = chrome::read(&profile, service)
                .unwrap_or_else(|e| die(&e, 2))
                .unwrap_or_default();
            report(&text, &got)
        }
        _ => die(USAGE, 2),
    };
    exit(code);
}
