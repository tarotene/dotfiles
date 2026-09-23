//! リポジトリ本体の `rust-migration.toml` を実体と突き合わせる(#389 の完了定義)。

use std::path::Path;

#[test]
fn repository_allowlist_matches_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m = migration_audit::load(&root).expect("rust-migration.toml");
    let r = migration_audit::audit(&root, &m);
    eprintln!(
        "rust-migration: 残り {} 件(max_remaining = {})",
        r.remaining, r.max_remaining
    );
    assert!(
        r.problems.is_empty(),
        "rust-migration.toml と実体が食い違っています:\n  {}",
        r.problems.join("\n  ")
    );
}
