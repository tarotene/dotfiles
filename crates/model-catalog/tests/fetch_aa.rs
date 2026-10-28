//! `model-catalog fetch-aa --input … --write` を実際のバイナリで 2 回回して、冪等であることと、
//! 手登録の部分が変わらないことを確かめる(AA の実 API は呼ばない)。

use std::process::Command;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/aa-small.json");

fn fetch(file: &std::path::Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_model-catalog"))
        .args([
            "fetch-aa",
            "--input",
            FIXTURE,
            "--retrieved",
            "2026-10-28",
            "--file",
        ])
        .arg(file)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn write_registers_the_in_range_models_and_a_second_write_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.toml");
    // 手登録だけの固定の土台（実カタログには自動登録が入っていくので使わない、#1188）。
    let original = include_str!("fixtures/catalog-hand-registered.toml");
    std::fs::write(&file, original).unwrap();

    // --write なしでは書かない。
    let dry = fetch(&file, &[]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    let said = String::from_utf8_lossy(&dry.stdout);
    assert!(
        said.contains("新しく自動登録したモデル: acme-one"),
        "{said}"
    );
    assert!(said.contains("新しく自動登録したモデル: zeta-3"), "{said}");

    let first = fetch(&file, &["--write"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let after1 = std::fs::read_to_string(&file).unwrap();
    let cut = |t: &str| t[..t.find(model_catalog::aa::BEGIN).unwrap()].to_owned();
    assert_eq!(cut(&after1), cut(original), "手登録の部分は変わらない");
    let c = model_catalog::Catalog::parse(&after1).unwrap();
    assert_eq!(c.model("acme-one").unwrap().efforts, ["low", "high"]);
    assert!(c.model("acme-old").is_none());
    assert_eq!(
        c.providers
            .iter()
            .filter(|p| p.aa_creator.is_some())
            .count(),
        4
    );

    let second = fetch(&file, &["--write"]);
    assert!(second.status.success());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        after1,
        "2 回目で差分が出ない"
    );
}

#[test]
fn write_to_the_embedded_catalog_is_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_model-catalog"))
        .args(["fetch-aa", "--input", FIXTURE, "--write"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--file"));
}
