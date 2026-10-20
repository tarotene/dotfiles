//! `pipeline-map check [--root <dir>]` — `pipeline-map.toml` と登録箇所の突き合わせ。
//! 判定は `pipeline_map::audit`(`cargo test` が同じ関数を呼ぶ)。
//!
//! `pipeline-map render [--root <dir>] [--no-svg]` — manifest から D2 のソース
//! (`docs/assets/pipeline-map/*.d2`)を作り、`d2 --layout=elk` で SVG にする。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: pipeline-map check [--root <dir>]\n       pipeline-map render [--root <dir>] [--no-svg]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("render") => render(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn check(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--root", Some(v)) => root = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let manifest = match pipeline_map::load(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("pipeline-map: {e}");
            return ExitCode::from(1);
        }
    };
    let r = pipeline_map::audit(&root, &manifest);
    eprintln!(
        "pipeline-map: node {}(auto {})/ edge {} / 登録箇所 {}",
        r.nodes, r.auto_nodes, r.edges, r.sites
    );
    if r.problems.is_empty() {
        println!("OK: pipeline-map.toml と登録箇所は一致しています");
        ExitCode::SUCCESS
    } else {
        for p in &r.problems {
            eprintln!("  - {p}");
        }
        eprintln!("pipeline-map: {} 件の不一致", r.problems.len());
        ExitCode::from(1)
    }
}

fn render(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut svg = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => match it.next() {
                Some(v) => root = PathBuf::from(v),
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--no-svg" => svg = false,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let manifest = match pipeline_map::load(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("pipeline-map: {e}");
            return ExitCode::from(1);
        }
    };
    // 図は検査に通った manifest からだけ作る(登録箇所との不一致があれば止める)。
    let r = pipeline_map::audit(&root, &manifest);
    if !r.problems.is_empty() {
        for p in &r.problems {
            eprintln!("  - {p}");
        }
        eprintln!(
            "pipeline-map: manifest に {} 件の不一致があるので図を作らない",
            r.problems.len()
        );
        return ExitCode::from(1);
    }
    let dir = root.join(pipeline_map::ASSET_DIR);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("pipeline-map: {}: {e}", dir.display());
        return ExitCode::from(1);
    }
    let diagrams = pipeline_map::render::render_all(&manifest);
    // 古い図(manifest から作られなくなったもの)を消す。
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or("");
            let ours = name.ends_with(".d2") || name.ends_with(".svg");
            if ours && !diagrams.iter().any(|(s, _)| s == stem) {
                let _ = fs::remove_file(e.path());
                eprintln!("pipeline-map: 古い図を消した: {name}");
            }
        }
    }
    for (stem, d2) in &diagrams {
        let d2_path = dir.join(format!("{stem}.d2"));
        if let Err(e) = fs::write(&d2_path, d2) {
            eprintln!("pipeline-map: {}: {e}", d2_path.display());
            return ExitCode::from(1);
        }
        if svg {
            if let Err(e) = d2_to_svg(&d2_path, &dir.join(format!("{stem}.svg"))) {
                eprintln!("pipeline-map: {stem}: {e}");
                return ExitCode::from(1);
            }
        }
        eprintln!("pipeline-map: {stem}");
    }
    ExitCode::SUCCESS
}

fn d2_to_svg(src: &Path, dst: &Path) -> Result<(), String> {
    let out = Command::new("d2")
        .arg("--layout=elk")
        .arg(src)
        .arg(dst)
        .output()
        .map_err(|e| {
            format!("d2 を起動できない({e})。`nix develop` の中で実行するか --no-svg を付ける")
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "d2 が失敗した: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
