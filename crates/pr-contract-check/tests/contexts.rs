//! rulesets-context-check(かつての `scripts/rulesets-context-check --selftest` の 13 件と、
//! 「テンプレート間で PR title の context が揃っている」整合性の検査)。
//!
//! 2026-10-27(#1014・#1065)から、実行中の job 名を待つ代わりに、head の
//! `.github/workflows/` 配下の全 workflow から読んだ job 名と静的に突き合わせる。

mod common;
use common::{run, Stub};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_rulesets-context-check");

const QUALITY_OK: &str = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"test"},{"context":"PR Title / PR title"}]}}]}"#;
const LIVE_OK: &str = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"test"},{"context":"PR Title / PR title"}]}}]"#;
const QUALITY_EXTRA: &str = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Ghost"}]}}]}"#;

/// 1 つの workflow ファイル(ファイル名, 中身)。
type Wf<'a> = (&'a str, String);

/// job 名だけを並べた最小の workflow(`jobs:` の下に、id と `name:` を持つ job)。
fn wf<'a>(file: &'a str, names: &[&str]) -> Wf<'a> {
    let mut y = String::from("name: W\non: pull_request\njobs:\n");
    for (i, n) in names.iter().enumerate() {
        y.push_str(&format!(
            "  j{i}:\n    name: {n}\n    runs-on: ubuntu-latest\n"
        ));
    }
    (file, y)
}

/// 宣言の context `test` と `PR Title / PR title` を満たす workflow 群。
fn wfs_ok<'a>() -> Vec<Wf<'a>> {
    vec![
        wf("ci.yml", &["test"]),
        wf("pr-title.yml", &["PR Title / PR title"]),
    ]
}

struct Case<'a> {
    owner_repo: &'a str,
    /// 空なら 404(まだ播いていない)。
    declared: &'a str,
    live: &'a str,
    workflows: Vec<Wf<'a>>,
}

fn case<'a>(
    owner_repo: &'a str,
    declared: &'a str,
    live: &'a str,
    workflows: Vec<Wf<'a>>,
) -> Case<'a> {
    Case {
        owner_repo,
        declared,
        live,
        workflows,
    }
}

fn put_workflows(stub: &Stub, repo: &str, workflows: &[Wf]) {
    let list: Vec<String> = workflows
        .iter()
        .map(|(f, _)| format!(r#"{{"name":"{f}","type":"file"}}"#))
        .collect();
    stub.put(
        &format!("repos/{repo}/contents/.github/workflows"),
        &format!("[{}]", list.join(",")),
    );
    for (f, text) in workflows {
        stub.put(
            &format!("repos/{repo}/contents/.github/workflows/{f}"),
            text,
        );
    }
}

/// 宣言・live・workflow を置いて走らせる。`rules_fail` なら branch ruleset の取得を失敗させる。
fn check(stub: &Stub, c: &Case, rules_fail: bool) -> common::Run {
    check_env(stub, c, rules_fail, &[])
}

fn check_env(stub: &Stub, c: &Case, rules_fail: bool, envs: &[(&str, &str)]) -> common::Run {
    let repo = c.owner_repo;
    let decl = format!("repos/{repo}/contents/.github/rulesets/quality.json");
    if c.declared.is_empty() {
        stub.remove(&decl);
    } else {
        stub.put(&decl, c.declared);
    }
    let rules = format!("repos/{repo}/rules/branches/main");
    stub.put(&rules, c.live);
    if rules_fail {
        stub.fail(&rules);
    } else {
        stub.unfail(&rules);
    }
    put_workflows(stub, repo, &c.workflows);
    let gh = stub.gh();
    let mut all = vec![("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap())];
    all.extend_from_slice(envs);
    run(BIN, &all, &[repo, "abc", "main"])
}

#[test]
fn declared_live_and_jobs_agree() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/all-ok", QUALITY_OK, LIVE_OK, wfs_ok()),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("rulesets-context-check: OK"));
}

/// #1014: required context は 1 つの workflow に収まらない。`CI passed`(ci.yml)と
/// `PR title`(別 workflow の pr-title.yml)の 2 つを宣言し、ci.yml だけに片方があっても、
/// 全 workflow を読めば OK。`PR title` の job は、この検査を走らせている job 自身である。
#[test]
fn contexts_spread_over_separate_workflows_are_all_found() {
    let s = Stub::new();
    let q = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI passed"},{"context":"PR title"}]}}]}"#;
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI passed"},{"context":"PR title"}]}}]"#;
    let wfs = vec![
        wf("ci.yml", &["Shell script validation", "CI passed"]),
        wf("pr-title.yml", &["PR title"]),
    ];
    let r = check(&s, &case("tarotene/two-files", q, live, wfs), false);
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("rulesets-context-check: OK"));

    // ci.yml だけを読んでいたら落ちる構成: pr-title.yml が無ければ `PR title` は見つからない。
    let wfs = vec![wf("ci.yml", &["CI passed"])];
    let r = check(&s, &case("tarotene/one-file", q, live, wfs), false);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("  - PR title"), "{}", r.err);
}

/// #1065: 待たない。以前の待ち方の環境変数が設定されていても(他リポジトリの action が
/// 設定したまま古い版を呼ぶ場合も含め)、見つからない context は即座に赤になる。
#[test]
fn does_not_wait_even_when_the_old_timeout_variables_are_set() {
    let s = Stub::new();
    let started = Instant::now();
    let r = check_env(
        &s,
        &case("tarotene/no-wait", QUALITY_EXTRA, LIVE_OK, wfs_ok()),
        false,
        &[
            ("RULESETS_CONTEXT_CHECK_TIMEOUT", "1200"),
            ("RULESETS_CONTEXT_CHECK_INTERVAL", "20"),
        ],
    );
    assert_eq!(r.code, 1);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "待ちが残っている: {:?}",
        started.elapsed()
    );
}

#[test]
fn live_context_not_reported() {
    let s = Stub::new();
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Tools ( CLI clippy + tests)"}]}}]"#;
    let r = check(
        &s,
        &case("tarotene/live-missing", QUALITY_OK, live, wfs_ok()),
        false,
    );
    assert_eq!(r.code, 1);
    assert!(
        r.err.contains("live な required_status_checks"),
        "{}",
        r.err
    );
    assert!(r.err.contains("  - Tools ( CLI clippy + tests)"));
}

#[test]
fn declared_context_not_reported() {
    let s = Stub::new();
    let r = check(
        &s,
        &case(
            "tarotene/declared-missing",
            QUALITY_EXTRA,
            LIVE_OK,
            wfs_ok(),
        ),
        false,
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("宣言"));
    assert!(r.err.contains("  - Ghost"));
}

#[test]
fn no_declaration_checks_live_only_and_warns() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/no-declaration", "", LIVE_OK, wfs_ok()),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("::warning::"));
}

#[test]
fn declared_differs_from_live_is_apply_pending() {
    let s = Stub::new();
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"test"}]}}]"#;
    let r = check(
        &s,
        &case("tarotene/apply-pending", QUALITY_OK, live, wfs_ok()),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("apply pending"));
}

#[test]
fn dotfiles_self_application_single_name() {
    let s = Stub::new();
    let q = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"PR title"}]}}]}"#;
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"PR title"}]}}]"#;
    let r = check(
        &s,
        &case(
            "tarotene/dotfiles",
            q,
            live,
            vec![wf("pr-title.yml", &["PR title"])],
        ),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("rulesets-context-check: OK"));
}

#[test]
fn no_workflows_after_the_directory_404s_is_red() {
    let s = Stub::new();
    let c = case("tarotene/no-workflows", QUALITY_OK, LIVE_OK, Vec::new());
    // 一覧そのものが 404(ディレクトリが無い)。
    let r = {
        let repo = c.owner_repo;
        s.put(
            &format!("repos/{repo}/contents/.github/rulesets/quality.json"),
            QUALITY_OK,
        );
        s.put(&format!("repos/{repo}/rules/branches/main"), LIVE_OK);
        let gh = s.gh();
        run(
            BIN,
            &[("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap())],
            &[repo, "abc", "main"],
        )
    };
    assert_eq!(r.code, 1, "{}", r.both());
    assert!(r.err.contains("  - test"), "{}", r.err);
}

#[test]
fn empty_workflow_list_is_red() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/no-jobs", QUALITY_OK, LIVE_OK, Vec::new()),
        false,
    );
    assert_eq!(r.code, 1);
}

#[test]
fn workflow_listing_failure_is_fail_closed() {
    let s = Stub::new();
    let repo = "tarotene/list-500";
    let list = format!("repos/{repo}/contents/.github/workflows");
    s.put(&list, "[]");
    s.fail(&list);
    s.put(
        &format!("repos/{repo}/contents/.github/rulesets/quality.json"),
        QUALITY_OK,
    );
    s.put(&format!("repos/{repo}/rules/branches/main"), LIVE_OK);
    let gh = s.gh();
    let r = run(
        BIN,
        &[("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap())],
        &[repo, "abc", "main"],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("workflows/ の読み取りに失敗"), "{}", r.err);
}

#[test]
fn workflow_file_fetch_failure_is_fail_closed() {
    let s = Stub::new();
    let repo = "tarotene/file-500";
    let c = case(repo, QUALITY_OK, LIVE_OK, wfs_ok());
    // 一度置いてから、ci.yml の取得だけを失敗させる。
    check(&s, &c, false);
    s.fail(&format!("repos/{repo}/contents/.github/workflows/ci.yml"));
    let gh = s.gh();
    let r = run(
        BIN,
        &[("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap())],
        &[repo, "abc", "main"],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("workflows/ の読み取りに失敗"), "{}", r.err);
}

#[test]
fn live_ruleset_fetch_failure_is_fail_closed() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/unreachable", QUALITY_OK, LIVE_OK, wfs_ok()),
        true,
    );
    assert_eq!(r.code, 1);
    assert!(
        r.err.contains("branch ruleset 取得に失敗しました"),
        "{}",
        r.err
    );
}

/// `name:` の無い呼び出し側 job の連結名は一致しない(ADR-591 D3 の前の形)。
#[test]
fn unnamed_caller_job_is_a_plain_mismatch() {
    let s = Stub::new();
    let wfs = vec![
        wf("ci.yml", &["test"]),
        (
            "pr-title.yml",
            "on: pull_request\njobs:\n  check:\n    uses: o/r/.github/workflows/p.yml@main\n"
                .to_string(),
        ),
    ];
    let r = check(
        &s,
        &case("tarotene/unnamed-caller", QUALITY_OK, LIVE_OK, wfs),
        false,
    );
    // `check / *` になるので、`PR Title / PR title` は一致しない。
    assert_eq!(r.code, 1, "{}", r.both());
}

/// reusable workflow を呼ぶ job は `<呼び出し側の name> / <呼ばれる job>` と報告される。
#[test]
fn reusable_caller_with_a_name_matches_the_joined_context() {
    let s = Stub::new();
    let wfs = vec![
        wf("ci.yml", &["test"]),
        (
            "pr-title.yml",
            "on: pull_request\njobs:\n  check:\n    name: PR Title\n    uses: o/r/.github/workflows/p.yml@main\n"
                .to_string(),
        ),
    ];
    let r = check(
        &s,
        &case("tarotene/named-caller", QUALITY_OK, LIVE_OK, wfs),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
}

/// matrix・式のある名前は、取りうる名前に展開して照合する。
#[test]
fn matrix_names_match_expanded_contexts() {
    let s = Stub::new();
    let q = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Build vega"}]}}]}"#;
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Build vega"}]}}]"#;
    let wfs = vec![(
        "ci.yml",
        "on: pull_request\njobs:\n  build-host:\n    name: Build ${{ matrix.host }}\n    strategy:\n      matrix:\n        host: [vega, arcturus]\n"
            .to_string(),
    )];
    let r = check(&s, &case("tarotene/matrix", q, live, wfs), false);
    assert_eq!(r.code, 0, "{}", r.both());
}

/// 読み切れない workflow があるとき、見つからない context は warning(fail にしない)。
#[test]
fn unresolved_workflow_turns_a_miss_into_a_warning() {
    let s = Stub::new();
    let wfs = vec![
        wf("ci.yml", &["test"]),
        (
            "odd.yml",
            "on: pull_request\njobs: { a: { runs-on: x } }\n".to_string(),
        ),
    ];
    let r = check(
        &s,
        &case("tarotene/odd", QUALITY_EXTRA, LIVE_OK, wfs),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("odd.yml"), "{}", r.out);
}

/// owner が tarotene 以外でも、`owner_repo` からそのまま取得先を作る(D8)。
#[test]
fn owner_other_than_tarotene() {
    let s = Stub::new();
    let r = check(&s, &case("acme/x", QUALITY_OK, LIVE_OK, wfs_ok()), false);
    assert_eq!(r.code, 0, "{}", r.both());
}

/// `plan-free-private`(docs/agents/free-plan-declaration.md): 無料プランの非公開
/// リポジトリでは branch ruleset の取得が 403 になる。
#[test]
fn plan_free_private_skips_live_but_still_checks_declared() {
    let s = Stub::new();
    s.put(
        "repos/acme/free",
        r#"{"topics":["naming-descriptive","plan-free-private"],"private":true}"#,
    );
    let r = check(&s, &case("acme/free", QUALITY_OK, LIVE_OK, wfs_ok()), true);
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("plan-free-private"));

    let r = check(
        &s,
        &case("acme/free", QUALITY_EXTRA, LIVE_OK, wfs_ok()),
        true,
    );
    assert_eq!(r.code, 1, "宣言の context が未報告なら red のまま");
    assert!(r.err.contains("宣言"));
}

#[test]
fn without_the_topic_a_missing_live_ruleset_stays_fail_closed() {
    let s = Stub::new();
    s.put(
        "repos/acme/paid",
        r#"{"topics":["naming-descriptive"],"private":true}"#,
    );
    let r = check(&s, &case("acme/paid", QUALITY_OK, LIVE_OK, wfs_ok()), true);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("取得に失敗"));
}

#[test]
fn declaration_fetch_failure_other_than_404_is_fail_closed() {
    let s = Stub::new();
    let repo = "tarotene/decl-500";
    s.put(
        &format!("repos/{repo}/contents/.github/rulesets/quality.json"),
        QUALITY_OK,
    );
    s.fail(&format!(
        "repos/{repo}/contents/.github/rulesets/quality.json"
    ));
    s.put(&format!("repos/{repo}/rules/branches/main"), LIVE_OK);
    let gh = s.gh();
    let r = run(
        BIN,
        &[("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap())],
        &[repo, "abc", "main"],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("宣言取得に失敗しました"), "{}", r.err);
}

#[test]
fn usage_errors() {
    let r = run(BIN, &[], &["--help"]);
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with("usage: rulesets-context-check"));
    let r = run(BIN, &[], &["only-one"]);
    assert_eq!(r.code, 2);
    assert_eq!(run(BIN, &[], &[]).code, 1);
}
