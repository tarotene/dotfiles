//! このリポジトリの `docs/README.md` の索引が、`docs/` の文書の H1 と一致していることを検査する。
//! 書き直し忘れは、ここで落ちる(CI の `Rust workspace` ジョブの `cargo test --workspace`)。

use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/docs-index の 2 つ上がリポジトリの根")
}

#[test]
fn the_docs_index_matches_the_h1_of_every_document() {
    let (current, rendered) = docs_index::expected(root()).unwrap_or_else(|problems| {
        panic!("索引を作れない:\n{}", problems.join("\n"));
    });
    if let Some((n, have, want)) = docs_index::first_difference(&current, &rendered) {
        panic!(
            "docs/README.md の索引が古い(最初の違い: {n} 行目)\n  今: {have}\n  本来: {want}\n`cargo run -p docs-index -- render` で書き直す"
        );
    }
}

/// 生成の節の中で、どの文書も 1 回ずつ載る(載せ忘れ・重複がない)。手書きの散文のリンクは数えない。
#[test]
fn every_document_appears_exactly_once_in_the_generated_blocks() {
    let docs = root().join("docs");
    let blocks = docs_index::scan(&docs).expect("scan");
    let readme = std::fs::read_to_string(docs.join("README.md")).unwrap();
    let mut generated = String::new();
    let mut inside = false;
    for line in readme.lines() {
        if line.starts_with(docs_index::BEGIN_PREFIX) {
            inside = true;
        } else if line == docs_index::END_MARKER {
            inside = false;
        } else if inside {
            generated.push_str(line);
            generated.push('\n');
        }
    }
    for (_, docs_in_block) in blocks {
        for d in docs_in_block {
            let link = format!("]({})", d.rel);
            assert_eq!(
                generated.matches(&link).count(),
                1,
                "{} が生成の節に 1 回ではない",
                d.rel
            );
        }
    }
}
