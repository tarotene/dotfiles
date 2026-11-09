//! `docs-index render [--root DIR]` — `docs/README.md` の生成の節を書き直す。
//! `docs-index --check [--root DIR]` — 書き直す必要があれば、最初に違う行を出して終了コード 1。
//! 使い方の誤りは 2。`--root` の既定は cwd(リポジトリの根)。

use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage: docs-index render|--check [--root DIR]";

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next() else {
        eprintln!("{USAGE}");
        exit(2);
    };
    let mut root = PathBuf::from(".");
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--root", Some(d)) => root = PathBuf::from(d),
            _ => {
                eprintln!("{USAGE}");
                exit(2);
            }
        }
    }
    let (current, rendered) = match docs_index::expected(&root) {
        Ok(pair) => pair,
        Err(problems) => {
            for p in problems {
                eprintln!("docs-index: {p}");
            }
            exit(1);
        }
    };
    match mode.as_str() {
        "render" => {
            if current != rendered {
                let path = root.join("docs/README.md");
                if let Err(e) = std::fs::write(&path, &rendered) {
                    eprintln!("docs-index: {}: 書けない: {e}", path.display());
                    exit(1);
                }
            }
        }
        "--check" => {
            if let Some((n, have, want)) = docs_index::first_difference(&current, &rendered) {
                eprintln!(
                    "docs-index: docs/README.md の索引が古い(最初の違い: {n} 行目)。\n  今: {have}\n  本来: {want}\n`cargo run -p docs-index -- render` で書き直す。"
                );
                exit(1);
            }
        }
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    }
}
