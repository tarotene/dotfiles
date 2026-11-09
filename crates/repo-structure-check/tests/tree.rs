//! 実際のツリーに、不変条件を当てる。`cargo test -p repo-structure-check` が CI の `Rust workspace`
//! ジョブで走る。

use std::path::{Path, PathBuf};

use repo_structure_check::hook_io::{self, ENVELOPE_EXCEPTIONS, STDIN_EXCEPTIONS};
use repo_structure_check::manifest::{check_tree, read_members, ZERO_DEPENDENCY_CRATES};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_tree_satisfies_the_manifest_invariants() {
    let violations = check_tree(&repo_root()).expect("read the tree");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "manifest の不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_check_reads_the_members_it_is_about() {
    let members = read_members(&repo_root()).expect("read the members");
    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    for expected in ZERO_DEPENDENCY_CRATES
        .iter()
        .copied()
        .chain(["repo-structure-check", "agent-session"])
    {
        assert!(
            names.contains(&expected),
            "{expected} が members に無い: {names:?}"
        );
    }
    assert!(names.len() > 90, "members が少なすぎる: {}", names.len());
}

#[test]
fn the_tree_satisfies_the_hook_io_invariants() {
    let violations = hook_io::check_tree(&repo_root()).expect("read the sources");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "hook の入出力の不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_hook_io_check_reads_the_sources_it_is_about() {
    let sources = hook_io::read_sources(&repo_root()).expect("read the sources");
    let paths: Vec<&str> = sources.iter().map(|s| s.path.as_str()).collect();
    for expected in ENVELOPE_EXCEPTIONS
        .iter()
        .chain(STDIN_EXCEPTIONS)
        .map(|(p, _)| *p)
        .chain([
            "crates/agent-session/src/reply.rs",
            "crates/agent-session/src/entry.rs",
        ])
    {
        assert!(paths.contains(&expected), "{expected} が読まれていない");
    }
    assert!(sources.len() > 300, "ソースが少なすぎる: {}", sources.len());
}
