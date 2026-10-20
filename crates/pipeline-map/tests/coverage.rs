//! リポジトリ本体の `pipeline-map.toml` を登録箇所と突き合わせる。
//! 自動発火する機構を足したら manifest にノードを足し、登録箇所にポインタを書く
//! (Claude hook は `node`、それ以外は `pipeline-node:`)。

use std::path::Path;

#[test]
fn repository_manifest_matches_registrations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m = pipeline_map::load(&root).expect("pipeline-map.toml");
    let r = pipeline_map::audit(&root, &m);
    eprintln!(
        "pipeline-map: node {}(auto {})/ edge {} / 登録箇所 {}",
        r.nodes, r.auto_nodes, r.edges, r.sites
    );
    assert!(
        r.problems.is_empty(),
        "pipeline-map.toml と登録箇所が食い違っています:\n  {}",
        r.problems.join("\n  ")
    );
}
