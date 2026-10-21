//! manifest から作った D2 のソースが、コミット済みの図と一致することを検査する。
//! SVG の再生成差分は CI が `d2` で検査する(`d2` を要求しないので、ここは .d2 だけ)。

use std::path::Path;

#[test]
fn committed_diagrams_match_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m = pipeline_map::load(&root).expect("pipeline-map.toml");
    let problems = pipeline_map::asset_problems(&root, &m);
    assert!(
        problems.is_empty(),
        "docs/assets/pipeline-map が manifest と食い違っています:\n  {}",
        problems.join("\n  ")
    );
}
