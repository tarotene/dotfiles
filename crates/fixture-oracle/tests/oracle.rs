//! `tests/cases/<bin>/*.toml` を bash 版の実体に向けて走らせる(#391)。

use std::path::Path;

#[test]
fn bash_oracles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let t = trycmd::TestCases::new();
    for (bin, rel) in fixture_oracle::BASH_ORACLES {
        t.register_bin(*bin, root.join(rel));
        t.case(format!("tests/cases/{bin}/*.toml"));
    }
}
