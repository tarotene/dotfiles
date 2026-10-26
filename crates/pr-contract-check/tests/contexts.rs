//! rulesets-context-check(かつての `scripts/rulesets-context-check --selftest` の 13 件と、
//! 「テンプレート間で PR title の context が揃っている」整合性の検査)。

mod common;
use common::{run, Stub};

const BIN: &str = env!("CARGO_BIN_EXE_rulesets-context-check");

const QUALITY_OK: &str = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"test"},{"context":"PR Title / PR title"}]}}]}"#;
const LIVE_OK: &str = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"test"},{"context":"PR Title / PR title"}]}}]"#;
const JOBS_OK: &str = r#"[{"name":"test"},{"name":"PR Title / PR title"}]"#;
const QUALITY_EXTRA: &str = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Ghost"}]}}]}"#;

struct Case<'a> {
    owner_repo: &'a str,
    /// 空なら 404(まだ播いていない)。
    declared: &'a str,
    live: &'a str,
    jobs: &'a str,
}

/// 宣言・live・実測を置いて走らせる。`rules_fail` なら branch ruleset の取得を失敗させる。
fn check(stub: &Stub, c: &Case, rules_fail: bool) -> common::Run {
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
    stub.put(
        &format!("repos/{repo}/actions/runs"),
        r#"{"workflow_runs":[{"id":1}]}"#,
    );
    stub.put(
        &format!("repos/{repo}/actions/runs/1/jobs"),
        &format!(r#"{{"jobs":{}}}"#, c.jobs),
    );
    let gh = stub.gh();
    run(
        BIN,
        &[
            ("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap()),
            ("RULESETS_CONTEXT_CHECK_INTERVAL", "0"),
            ("RULESETS_CONTEXT_CHECK_TIMEOUT", "0"),
        ],
        &[repo, "abc", "main"],
    )
}

fn case<'a>(owner_repo: &'a str, declared: &'a str, live: &'a str, jobs: &'a str) -> Case<'a> {
    Case {
        owner_repo,
        declared,
        live,
        jobs,
    }
}

#[test]
fn declared_live_and_jobs_agree() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/all-ok", QUALITY_OK, LIVE_OK, JOBS_OK),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("rulesets-context-check: OK"));
}

#[test]
fn live_context_not_reported() {
    let s = Stub::new();
    let live = r#"[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Tools ( CLI clippy + tests)"}]}}]"#;
    let r = check(
        &s,
        &case("tarotene/live-missing", QUALITY_OK, live, JOBS_OK),
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
        &case("tarotene/declared-missing", QUALITY_EXTRA, LIVE_OK, JOBS_OK),
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
        &case("tarotene/no-declaration", "", LIVE_OK, JOBS_OK),
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
        &case("tarotene/apply-pending", QUALITY_OK, live, JOBS_OK),
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
        &case("tarotene/dotfiles", q, live, r#"[{"name":"PR title"}]"#),
        false,
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("rulesets-context-check: OK"));
}

#[test]
fn empty_jobs_after_timeout_is_red() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/no-jobs", QUALITY_OK, LIVE_OK, "[]"),
        false,
    );
    assert_eq!(r.code, 1);
}

#[test]
fn live_ruleset_fetch_failure_is_fail_closed() {
    let s = Stub::new();
    let r = check(
        &s,
        &case("tarotene/unreachable", QUALITY_OK, LIVE_OK, JOBS_OK),
        true,
    );
    assert_eq!(r.code, 1);
    assert!(
        r.err.contains("branch ruleset 取得に失敗しました"),
        "{}",
        r.err
    );
}

#[test]
fn unnamed_caller_job_is_a_plain_mismatch() {
    let s = Stub::new();
    let jobs = r#"[{"name":"check / PR title"},{"name":"test"}]"#;
    let r = check(
        &s,
        &case("tarotene/unnamed-caller", QUALITY_OK, LIVE_OK, jobs),
        false,
    );
    assert_eq!(r.code, 1);
}

/// owner が tarotene 以外でも、`owner_repo` からそのまま取得先を作る(D8)。
#[test]
fn owner_other_than_tarotene() {
    let s = Stub::new();
    let r = check(&s, &case("acme/x", QUALITY_OK, LIVE_OK, JOBS_OK), false);
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
    let r = check(&s, &case("acme/free", QUALITY_OK, LIVE_OK, JOBS_OK), true);
    assert_eq!(r.code, 0, "{}", r.both());
    assert!(r.out.contains("plan-free-private"));

    let r = check(
        &s,
        &case("acme/free", QUALITY_EXTRA, LIVE_OK, JOBS_OK),
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
    let r = check(&s, &case("acme/paid", QUALITY_OK, LIVE_OK, JOBS_OK), true);
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
        &[
            ("GITHUB_AUDIT_GH_BIN", gh.to_str().unwrap()),
            ("RULESETS_CONTEXT_CHECK_TIMEOUT", "0"),
        ],
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
