//! 実際のツリーに、不変条件を当てる。`cargo test -p repo-structure-check` が CI の `Rust workspace`
//! ジョブで走る。

use std::path::{Path, PathBuf};

use repo_structure_check::civil_time;
use repo_structure_check::escape;
use repo_structure_check::hook_io::{self, ENVELOPE_EXCEPTIONS, STDIN_EXCEPTIONS};
use repo_structure_check::host_names;
use repo_structure_check::manifest::{check_tree, read_members, ZERO_DEPENDENCY_CRATES};
use repo_structure_check::process_env::{self, HELPER_EXCEPTIONS, XDG_EXCEPTIONS};

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

#[test]
fn the_tree_satisfies_the_escape_hatch_invariants() {
    let violations = escape::check_tree(&repo_root()).expect("read the tree");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "hook の逃げ道の不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_escape_check_reads_what_it_is_about() {
    let files = escape::read_text_files(&repo_root());
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    for expected in [
        "docs/adr/1040-agent-tooling-names-and-responsibilities.md",
        "crates/agent-session/src/escape.rs",
        "config/claude/CLAUDE.md",
        "home/modules/agents.nix",
    ] {
        assert!(paths.contains(&expected), "{expected} が読まれていない");
    }
    assert!(files.len() > 1000, "ファイルが少なすぎる: {}", files.len());
}

/// 表(`escape::ALL`)の各項目が、説明の文書に、自分の名前で載っている(表と文書の対)。
#[test]
fn every_hatch_is_documented_under_its_own_names() {
    for h in agent_session::escape::ALL {
        let text = std::fs::read_to_string(repo_root().join(h.doc))
            .unwrap_or_else(|e| panic!("{} を読めない: {e}", h.doc));
        if h.session {
            assert!(
                text.contains(&h.session_name()),
                "{} に `{}` が書かれていない",
                h.doc,
                h.session_name()
            );
        }
        if let Some(call) = h.call_name() {
            assert!(
                text.contains(&call),
                "{} に `{call}` が書かれていない",
                h.doc
            );
        }
    }
}

#[test]
fn the_tree_satisfies_the_host_name_invariants() {
    let violations = host_names::check_tree(&repo_root()).expect("read the nix sources");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "ホスト名の不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_host_name_check_reads_the_hosts_it_is_about() {
    let files = host_names::read_files(&repo_root()).expect("read the nix sources");
    let hosts = host_names::host_names(&files);
    for expected in ["altair", "arcturus", "vega"] {
        assert!(
            hosts.iter().any(|h| h == expected),
            "home/hosts/{expected}.nix が読めていない: {hosts:?}"
        );
    }
    assert!(
        files.iter().any(|f| f.path == host_names::MARKER_WRITER),
        "{} が読めていない",
        host_names::MARKER_WRITER
    );
    assert!(files.len() > 40, "読んだ nix が少なすぎる: {}", files.len());
}

#[test]
fn the_tree_satisfies_the_process_env_invariants() {
    let violations = process_env::check_tree(&repo_root()).expect("read the sources");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "process-env の不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_process_env_check_reads_what_it_is_about() {
    let sources = hook_io::read_sources(&repo_root()).expect("read the sources");
    for expected in ["crates/process-env/src/lib.rs", "crates/hms/src/lib.rs"] {
        assert!(
            sources.iter().any(|s| s.path == expected),
            "{expected} を読んでいない"
        );
    }
    for (path, _) in XDG_EXCEPTIONS.iter().chain(HELPER_EXCEPTIONS) {
        assert!(sources.iter().any(|s| s.path == *path), "{path} が無い");
    }
}

#[test]
fn the_tree_keeps_the_calendar_conversion_in_civil_time() {
    let violations = civil_time::check_tree(&repo_root()).expect("read the sources");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "暦の変換の定数が civil-time の外にある:\n{}",
        report.join("\n")
    );
}
