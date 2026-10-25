//! workflow-naming-check(かつての `scripts/workflow-naming-check --selftest` の 4 件と、
//! 引数・default branch・単一正本の経路)。

mod common;
use common::{run, Stub};

const BIN: &str = env!("CARGO_BIN_EXE_workflow-naming-check");

const CI_OK: &str = "name: CI
jobs:
  gitleaks:
    runs-on: ubuntu-latest
    name: Secret scan (gitleaks)
    steps:
      - run: echo hi
  ci-passed:
    runs-on: ubuntu-latest
    name: CI passed
    needs: [gitleaks]
    if: always()
    steps:
      - name: Fail if any required job failed or was cancelled
        if: contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled')
        run: exit 1
";
const QUALITY_OK: &str = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI passed"},{"context":"PR title"}]}}]}"#;
const ENTRIES_OK: &str =
    r#"[{"name":"ci.yml","type":"file"},{"name":"pr-title.yml","type":"file"}]"#;

struct Case<'a> {
    owner_repo: &'a str,
    entries: &'a str,
    ci: &'a str,
    pr_title: &'a str,
    quality: &'a str,
}

fn check(stub: &Stub, c: &Case, extra_env: &[(&str, &str)], extra_args: &[&str]) -> common::Run {
    let repo = c.owner_repo;
    stub.put(
        &format!("repos/{repo}/contents/.github/workflows"),
        c.entries,
    );
    for (path, text) in [
        (".github/workflows/ci.yml", c.ci),
        (".github/workflows/pr-title.yml", c.pr_title),
        (".github/rulesets/quality.json", c.quality),
    ] {
        let api = format!("repos/{repo}/contents/{path}");
        if text.is_empty() {
            stub.remove(&api);
        } else {
            stub.put(&api, text);
        }
    }
    // 単一正本は、ライブのテンプレートの今の中身から切り離す。
    let canonical = stub.path().join("canonical-quality.json");
    std::fs::write(&canonical, QUALITY_OK).unwrap();
    let gh = stub.gh();
    let mut env = vec![
        ("WORKFLOW_NAMING_CHECK_GH_BIN", gh.to_str().unwrap()),
        (
            "WORKFLOWS_CANONICAL_QUALITY_FILE",
            canonical.to_str().unwrap(),
        ),
    ];
    env.extend_from_slice(extra_env);
    let mut args = vec![repo, "--ref", "abc"];
    args.extend_from_slice(extra_args);
    run(BIN, &env, &args)
}

#[test]
fn compliant_fixture_is_ok() {
    let s = Stub::new();
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-ok",
            entries: ENTRIES_OK,
            ci: CI_OK,
            pr_title: "",
            quality: QUALITY_OK,
        },
        &[],
        &[],
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert_eq!(r.out, "workflow-naming-check: OK\n");
}

#[test]
fn no_workflows_dir_is_ci_yml_missing() {
    let s = Stub::new();
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-no-dir",
            entries: "[]",
            ci: "",
            pr_title: "",
            quality: "",
        },
        &[],
        &[],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("  - ci-yml-missing"), "{}", r.err);
    assert!(r.err.starts_with(
        "workflow-naming-check: tarotene/wf-no-dir@abc has drifted from the CI workflow naming basis"
    ));
}

#[test]
fn bad_names_are_reported() {
    let s = Stub::new();
    let ci_bad = "name: ci
jobs:
  gitleaks:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
";
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-bad-names",
            entries: r#"[{"name":"ci.yml","type":"file"}]"#,
            ci: ci_bad,
            pr_title: "",
            quality: "",
        },
        &[],
        &[],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("job-name-missing:gitleaks"));
    assert!(r.err.contains("workflow-name-lowercase"));
}

#[test]
fn legacy_reusable_call_is_detected() {
    let s = Stub::new();
    let pt = "jobs:
  check:
    uses: tarotene/dotfiles/.github/workflows/pr-title.yml@main
";
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-legacy",
            entries: ENTRIES_OK,
            ci: CI_OK,
            pr_title: pt,
            quality: QUALITY_OK,
        },
        &[],
        &[],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("legacy-reusable-pr-title-call"));
}

#[test]
fn quality_json_differing_from_the_canonical_file_is_reported() {
    let s = Stub::new();
    let other = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Other"}]}}]}"#;
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-q",
            entries: ENTRIES_OK,
            ci: CI_OK,
            pr_title: "",
            quality: other,
        },
        &[],
        &[],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("quality-json-not-canonical"), "{}", r.err);
}

/// 単一正本が読めない(CI で sparse-checkout した実体の隣には無い)ときは、その検査だけを飛ばす。
#[test]
fn unreadable_canonical_file_skips_only_that_check() {
    let s = Stub::new();
    let other = r#"{"rules":[]}"#;
    let r = check(
        &s,
        &Case {
            owner_repo: "tarotene/wf-skip",
            entries: ENTRIES_OK,
            ci: CI_OK,
            pr_title: "",
            quality: other,
        },
        &[(
            "WORKFLOWS_CANONICAL_QUALITY_FILE",
            "/nonexistent/canonical.json",
        )],
        &[],
    );
    assert_eq!(r.code, 0, "{}", r.both());
}

#[test]
fn default_branch_is_looked_up_when_ref_is_omitted() {
    let s = Stub::new();
    s.put("repos/tarotene/wf-default", r#"{"default_branch":"trunk"}"#);
    s.put(
        "repos/tarotene/wf-default/contents/.github/workflows",
        ENTRIES_OK,
    );
    s.put(
        "repos/tarotene/wf-default/contents/.github/workflows/ci.yml",
        CI_OK,
    );
    let r = run(
        BIN,
        &[
            ("WORKFLOW_NAMING_CHECK_GH_BIN", s.gh().to_str().unwrap()),
            ("WORKFLOWS_CANONICAL_QUALITY_FILE", "/nonexistent"),
        ],
        &["tarotene/wf-default"],
    );
    assert_eq!(r.code, 0, "{}", r.both());
}

#[test]
fn default_branch_lookup_failure_is_fail_closed() {
    let s = Stub::new();
    let r = run(
        BIN,
        &[("WORKFLOW_NAMING_CHECK_GH_BIN", s.gh().to_str().unwrap())],
        &["tarotene/unreachable"],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("default branch 取得に失敗しました"));
}

#[test]
fn usage_errors() {
    let r = run(BIN, &[], &["--help"]);
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with("usage: workflow-naming-check"));
    for args in [&[][..], &["--bogus"], &["owner/repo", "--ref"]] {
        let r = run(BIN, &[], args);
        assert_eq!(r.code, 2, "{args:?}");
        assert!(r.err.starts_with("usage:"));
    }
}
