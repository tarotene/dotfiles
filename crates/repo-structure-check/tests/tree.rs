//! 実際のツリーに、不変条件を当てる。`cargo test -p repo-structure-check` が CI の `Rust workspace`
//! ジョブで走る。

use std::path::{Path, PathBuf};

use repo_structure_check::civil_time;
use repo_structure_check::deps;
use repo_structure_check::escape;
use repo_structure_check::hook_io::{self, ENVELOPE_EXCEPTIONS, STDIN_EXCEPTIONS};
use repo_structure_check::host_names;
use repo_structure_check::manifest::{check_tree, read_members, ZERO_DEPENDENCY_CRATES};
use repo_structure_check::process_env::{self, HELPER_EXCEPTIONS, XDG_EXCEPTIONS};
use repo_structure_check::side_symmetry;
use repo_structure_check::web_assets;
use repo_structure_check::web_behavior;
use repo_structure_check::web_components;
use repo_structure_check::web_principles;
use repo_structure_check::web_selection;
use repo_structure_check::web_specimen;
use repo_structure_check::web_structures;
use repo_structure_check::web_variables;

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
fn the_tree_satisfies_the_dependency_graph_invariants() {
    let violations = deps::check_tree(&repo_root()).expect("read the tree");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "依存グラフの不変条件に反する箇所がある:\n{}",
        report.join("\n")
    );
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

/// 個人と会社は対称な対(ADR「個人と会社は対称な対」): 公開の module は側(identity)で分岐しない。
#[test]
fn the_tree_does_not_branch_on_the_identity_in_public_modules() {
    let files = host_names::read_files(&repo_root()).expect("read the nix sources");
    let violations = side_symmetry::check_identity_branches(&files);
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "公開の module が側(identity)で分岐している:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_identity_check_reads_the_public_modules_it_is_about() {
    let files = host_names::read_files(&repo_root()).expect("read the nix sources");
    let modules = files
        .iter()
        .filter(|f| f.path.starts_with("home/modules/"))
        .count();
    assert!(modules > 30, "読んだ公開の module が少なすぎる: {modules}");
    for expected in [
        side_symmetry::IDENTITY_MODULE,
        "home/identities/personal.nix",
        "home/identities/company.nix",
    ] {
        assert!(
            files.iter().any(|f| f.path == expected),
            "{expected} が読めていない"
        );
    }
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
fn the_tree_loads_no_fonts_or_styles_from_third_parties() {
    let violations = web_assets::check_tree(&repo_root()).expect("read the web assets");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "第三者から書体・スタイルを読む HTML・CSS がある:\n{}",
        report.join("\n")
    );
}

/// 空のツリーを読んで「違反なし」で通ってしまわないための番兵。
#[test]
fn the_web_assets_check_reads_what_it_is_about() {
    let files = web_assets::read_files(&repo_root()).expect("read the web assets");
    for expected in [
        "config/agents/skills/inbox-adjudication/reference/verdict-page/template.html",
        "config/claude/assets/plan-view.css",
        "crates/model-equivalence/page.html",
    ] {
        assert!(
            files.iter().any(|f| f.path == expected),
            "{expected} が読まれていない"
        );
    }
}

/// デザイントークンの正本は、役割の集合が閉じていて、明暗の両方でコントラストの基準を満たす(ADR-1685)。
#[test]
fn the_design_tokens_satisfy_the_contrast_and_shape_invariants() {
    let violations = web_variables::check_tree(&repo_root()).expect("read the design tokens");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "デザイントークンの正本が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 正本を読めずに「違反なし」で通ってしまわないための番兵。役割と尺度の主な名前が、実際に読まれている。
#[test]
fn the_design_tokens_check_reads_what_it_is_about() {
    let text = std::fs::read_to_string(repo_root().join(web_variables::VARIABLES_PATH))
        .expect("read the design tokens");
    for role in web_variables::REQUIRED_ROLES {
        assert!(
            text.contains(&format!("{role}:")),
            "{role} の宣言が読まれていない"
        );
    }
    for scale in ["--font-sans:", "--fs-body:", "--target:", "--dur:"] {
        assert!(text.contains(scale), "{scale} が読まれていない");
    }
}

/// 共有の部品は、正本の役割と尺度だけを参照し、操作部の大きさと置き方の裁定を守る(ADR-1687)。
#[test]
fn the_shared_components_use_only_the_design_variables() {
    let violations = web_components::check_tree(&repo_root()).expect("read the components");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "共有の部品が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 部品を読めずに「違反なし」で通ってしまわないための番兵。
#[test]
fn the_components_check_reads_what_it_is_about() {
    let css = std::fs::read_to_string(repo_root().join(web_components::COMPONENTS_PATH))
        .expect("read the components");
    for needle in [".btn-main", ".actions", "var(--target)", "var(--accent)"] {
        assert!(css.contains(needle), "{needle} が読まれていない");
    }
}

/// 振る舞いの共有部品とその見本のスクリプトは、外部の URL・style の注入・HTML の注入・モジュールの構文・失敗の握りつぶしを持たず、
/// 3 つの関数を公開して呼ぶ(ADR-1703 D2)。
#[test]
fn the_shared_behavior_scripts_satisfy_the_invariants() {
    let violations = web_behavior::check_tree(&repo_root()).expect("read the behavior scripts");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "振る舞いの共有部品が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// スクリプトを読めずに「違反なし」で通ってしまわないための番兵。
#[test]
fn the_behavior_check_reads_what_it_is_about() {
    let root = repo_root();
    let behavior = std::fs::read_to_string(root.join(web_behavior::BEHAVIOR_PATH))
        .expect("read the behavior script");
    for needle in [
        "function saver(",
        "function keyguard(",
        "function viewstate(",
        "pageBehavior",
    ] {
        assert!(behavior.contains(needle), "{needle} が読まれていない");
    }
    let specimen = std::fs::read_to_string(root.join(web_behavior::SPECIMEN_JS_PATH))
        .expect("read the specimen script");
    assert!(
        specimen.contains("demo-save-status"),
        "動く見本が読まれていない"
    );
}

/// 選び方の対応表は、語彙が閉じていて、`platform` は Widely available だけ、`shared` は実在するファイル、
/// `not-used` の `req` は実在する要件の ID を指す(ADR-1703)。
#[test]
fn the_selection_table_is_closed_and_consistent() {
    let violations = web_selection::check_tree(&repo_root()).expect("read the selection table");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "選び方の対応表が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 対応表を読めずに「違反なし」で通ってしまわないための番兵。5 つの選び方のうち、実物が持つ行が読まれている。
#[test]
fn the_selection_check_reads_what_it_is_about() {
    let text = std::fs::read_to_string(repo_root().join(web_selection::SELECTION_PATH))
        .expect("read the selection table");
    for needle in [
        "id = \"dialog\"",
        "way = \"platform\"",
        "way = \"shared\"",
        "way = \"handwritten\"",
        "way = \"not-used\"",
        "id = \"toast\"",
    ] {
        assert!(text.contains(needle), "{needle} が読まれていない");
    }
}

/// 原則の表は、語彙が閉じていて、決定の引用は出典のファイルにそのまま含まれ、すべての要件が実在する原則を
/// 指し、どの要件にも使われない一般の原則が無い(ADR-1708)。
#[test]
fn the_principles_table_is_closed_and_every_requirement_is_grounded() {
    let violations = web_principles::check_tree(&repo_root()).expect("read the principles");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "原則の層が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 原則の表と要件を読めずに「違反なし」で通ってしまわないための番兵。外部の原則と、決定の原則の両方の行が
/// 読まれ、要件に `principles` の欄が書かれている。
#[test]
fn the_principles_check_reads_what_it_is_about() {
    let root = repo_root();
    let principles = std::fs::read_to_string(root.join(web_principles::PRINCIPLES_PATH))
        .expect("read the principles table");
    for needle in [
        "id = \"wcag-2-5-8\"",
        "kind = \"principle\"",
        "kind = \"decision\"",
        "scope = \"structure\"",
        "id = \"adr-1677-d6\"",
    ] {
        assert!(principles.contains(needle), "{needle} が読まれていない");
    }
    let reqs = web_principles::requirements(
        &std::fs::read_to_string(root.join(web_principles::PAGE_TYPES_PATH))
            .expect("read the page types"),
    )
    .expect("parse the page types");
    assert!(reqs.len() >= 41, "要件が読まれていない: {}", reqs.len());
    assert!(
        reqs.iter()
            .any(|r| r.id == "C3"
                && r.principles.as_deref() == Some(&["wcag-2-5-8".to_string()][..])),
        "C3 が原則 wcag-2-5-8 に結びついていない"
    );
}

/// 構造の表は語彙が閉じていて、リポジトリ内のすべての HTML が、語彙にある構造と、整合する軸を宣言する。
/// 宣言の無いページを足せない(ADR-0000)。
#[test]
fn the_structures_table_is_closed_and_every_html_declares_its_structure() {
    let violations = web_structures::check_tree(&repo_root()).expect("read the structures");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "構造の規則が不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 構造の表と HTML を読めずに「違反なし」で通ってしまわないための番兵。3 つの構造と既定の構造が表にあり、
/// 既知の 3 つの HTML が走査の対象で、それぞれ構造を宣言している。
#[test]
fn the_structures_check_reads_what_it_is_about() {
    let root = repo_root();
    let table = std::fs::read_to_string(root.join(web_structures::STRUCTURES_PATH))
        .expect("read the structures table");
    for needle in [
        "id = \"step\"",
        "id = \"single-column\"",
        "id = \"document\"",
        "default = true",
        "measure = \"viewport-fit\"",
    ] {
        assert!(table.contains(needle), "{needle} が読まれていない");
    }
    let files = repo_structure_check::web_assets::read_files(&root).expect("read the web files");
    for known in [
        "config/agents/skills/inbox-adjudication/reference/verdict-page/template.html",
        "config/web/specimen.html",
        "crates/model-equivalence/page.html",
    ] {
        let f = files
            .iter()
            .find(|f| f.path == known)
            .unwrap_or_else(|| panic!("{known} が走査の対象に入っていない"));
        assert!(
            f.text.contains("name=\"page-structure\""),
            "{known} が構造を宣言していない"
        );
    }
}

/// 生成した `variables.json` は、正本の CSS から作った内容と一致する(手で書かない。ADR-1700)。
#[test]
fn the_generated_tokens_json_matches_the_css() {
    let violations =
        web_variables::check_tokens_json_tree(&repo_root()).expect("read the generated tokens");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "variables.json が正本とずれている:\n{}",
        report.join("\n")
    );
}

/// `variables.json` を読めずに通ってしまわないための番兵。役割が、明暗の 16 進で実際に載っている。
#[test]
fn the_generated_tokens_json_carries_resolved_colors() {
    let json = std::fs::read_to_string(repo_root().join(web_variables::VARIABLES_JSON_PATH))
        .expect("read the generated tokens");
    for needle in ["\"light\": \"#", "\"dark\": \"#", "\"name\": \"accent\""] {
        assert!(json.contains(needle), "{needle} が読まれていない");
    }
}

/// 見本ページは、正本を実際に読み込み、役割と部品のすべてを並べ、値を複写しない(ADR-1700)。
#[test]
fn the_specimen_loads_the_canon_and_shows_every_role_and_component() {
    let violations = web_specimen::check_tree(&repo_root()).expect("read the specimen");
    let report: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(
        violations.is_empty(),
        "見本ページが不変条件を満たさない:\n{}",
        report.join("\n")
    );
}

/// 見本を読めずに「違反なし」で通ってしまわないための番兵。
#[test]
fn the_specimen_check_reads_what_it_is_about() {
    let html = std::fs::read_to_string(repo_root().join(web_specimen::HTML_PATH))
        .expect("read the specimen");
    for needle in [
        "data-role=\"accent\"",
        "class=\"btn btn-main\"",
        "table-cards",
    ] {
        assert!(html.contains(needle), "{needle} が読まれていない");
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
