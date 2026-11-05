//! dotfiles 自身の宣言(`.config/record-layers.toml`)が、追跡されているファイルをちょうど 1 つの glob で
//! 覆っていること。新しいパスを足したのに宣言を直さなかった PR は、ここで落ちる(CI の `cargo test`)。
//! 重なりと漏れの検査は、glob 同士の包含を調べず、実在のファイルへの当たり方に落としている(ADR-1358 record-layers)。

use record_layer::{check_files, Declaration, Layer, Lookup, DECLARATION_PATH};
use std::path::Path;

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn declaration() -> Declaration {
    let text = std::fs::read_to_string(root().join(DECLARATION_PATH)).expect("宣言ファイル");
    Declaration::parse(&text).unwrap_or_else(|e| panic!("宣言を読めません: {e}"))
}

/// 追跡されているファイルと、まだ追跡されていない(`.gitignore` の対象でない)ファイル。後者を含めるのは、
/// 新しく足したファイルが add される前でも、同じ検査が落ちるようにするため。git が無い・リポジトリの外(nix の
/// ビルドの展開先など)では `None`。
fn files() -> Option<Vec<String>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root())
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        // 削除したが、まだ index に残っているもの。
        .filter(|f| root().join(f).exists())
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    Some(v)
}

#[test]
fn every_tracked_file_is_covered_by_exactly_one_glob() {
    let decl = declaration();
    let Some(files) = files() else {
        eprintln!("git でファイルを列挙できないので、網羅の検査を飛ばす");
        return;
    };
    assert!(
        files.len() > 100,
        "ファイルの列挙が少なすぎる: {}",
        files.len()
    );
    let defects = check_files(&decl, files.iter().map(String::as_str));
    assert!(
        defects.is_empty(),
        "宣言の漏れ・重なり(.config/record-layers.toml を直す):\n  {}",
        defects
            .iter()
            .map(|d| format!("{d:?}"))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn the_declaration_covers_itself_and_the_known_special_cases() {
    let decl = declaration();
    assert_eq!(decl.lookup(DECLARATION_PATH), Lookup::One(Layer::L1));
    assert_eq!(
        decl.lookup("docs/adr/872-pipeline-map.md"),
        Lookup::One(Layer::L0)
    );
    assert_eq!(
        decl.lookup("docs/assets/pipeline-map/overview.svg"),
        Lookup::One(Layer::L4)
    );
    // 手書きと GENERATED ブロックが混在するので、分割するまで L1 と宣言している(ADR-1358 D5)。
    assert_eq!(
        decl.lookup("crates/model-catalog/catalog.toml"),
        Lookup::One(Layer::L1)
    );
}

#[test]
fn no_glob_is_dead_so_a_stale_entry_cannot_hide() {
    // 実在のファイルに 1 つも当たらない glob は、消えたパスの宣言が残っている印。
    let decl = declaration();
    let Some(files) = files() else { return };
    for e in decl.entries() {
        let hit = files.iter().any(|f| e.matches(f));
        assert!(
            hit,
            "どのファイルにも当たらない glob: {} ({})",
            e.glob, e.layer
        );
    }
}
