//! bash 版 `--selftest` のうち、judge_* を直接呼ぶ検査(bash#49〜#162)。
//! `bash#N` は bash 版 selftest の N 番目の FAIL 検査に 1 対 1 で対応する。

mod common;

use common::Fx;
use github_audit::domains::docs::judge_docs;
use github_audit::domains::lifecycle::judge_lifecycle;
use github_audit::domains::naming::{judge_naming, NamingVocab};
use github_audit::domains::releaser::{judge_releaser, releaser_workflow_refs, ReleaserInput};
use github_audit::domains::renovate::judge_renovate;
use github_audit::domains::routines::judge_routines;
use github_audit::domains::rulesets::judge_rulesets;
use github_audit::domains::settings::judge_settings;
use github_audit::domains::titles::judge_titles;
use github_audit::domains::workflows::judge_workflows;
use github_audit::vocab::read_closed_set;
use github_audit::{
    any_drift, findings_from_json, Detail, Finding, RepoGql, RepoMeta, RepoRest, Verdict,
};
use serde_json::{json, Value};

const PRESET: &str = "github>tarotene/dotfiles//renovate/policy";

fn gql(s: &str) -> RepoGql {
    RepoGql::from_json_str(s)
}

fn gqlv(v: Value) -> RepoGql {
    RepoGql::from_value(&v)
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn rulesets_direct() {
    let fx = Fx::new();
    let gh = fx.gh();
    let none = RepoGql::default();

    // ADR-0020: CI の有無が ci-absent / required_status_checks を決める。
    let f = judge_rulesets(&gh, "ruleset-ci-test", false, &none, &[]);
    assert!(
        f.verdict == Verdict::Drifted && f.has("ci-absent") && !f.has("required_status_checks"),
        "bash#49: {f:?}"
    );
    let f = judge_rulesets(&gh, "ruleset-ci-test", true, &none, &[]);
    assert!(
        f.verdict == Verdict::Drifted && f.has("required_status_checks") && !f.has("ci-absent"),
        "bash#50: {f:?}"
    );

    // ADR-503: repo_gql 省略(既定 {})なら宣言なし。
    let f = judge_rulesets(&gh, "ruleset-decl-test", true, &none, &[]);
    assert!(f.has("rulesets-declaration-missing"), "bash#51: {f:?}");

    let quality = |enforcement: &str| {
        json!({"name":"Quality","target":"branch","enforcement":enforcement,"conditions":{"ref_name":{"include":["~DEFAULT_BRANCH"],"exclude":[]}},"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI"},{"context":"Ghost"}]}}],"bypass_actors":[]}).to_string()
    };
    let decl = |q: String| {
        gqlv(
            json!({"declSecurity": {"text": "{}"}, "declQuality": {"text": q}, "declWorkflow": {"text": "{}"}}),
        )
    };
    let mismatch = decl(quality("disabled"));
    let f = judge_rulesets(&gh, "ruleset-decl-test", true, &mismatch, &[]);
    assert!(
        f.has("rulesets-declaration-drift:Quality"),
        "bash#52: {f:?}"
    );

    // 正規化後に一致すれば Quality の drift は出ない(Security/Workflow は
    // 同名の live が無いので出る)。
    let ok = decl(quality("active"));
    let f = judge_rulesets(&gh, "ruleset-decl-test", true, &ok, &[]);
    assert!(
        !f.has("rulesets-declaration-drift:Quality"),
        "bash#53: {f:?}"
    );

    // live は CI と Ghost を要求、報告されるのは CI だけ → Ghost が報告不能。
    let f = judge_rulesets(&gh, "ruleset-decl-test", true, &ok, &s(&["CI"]));
    assert!(
        f.has("required-context-unreportable:Ghost"),
        "bash#54: {f:?}"
    );

    // job_names=[](PR がまだ無い)なら判定しない。
    let f = judge_rulesets(&gh, "ruleset-decl-test", true, &ok, &[]);
    assert!(
        !f.missing
            .iter()
            .any(|m| m.starts_with("required-context-unreportable:")),
        "bash#55: {f:?}"
    );
}

#[test]
fn renovate_direct() {
    let r = |j: &str, lang: &str| judge_renovate(&gql(j), lang, PRESET);
    // #3: flake.nix 単独でも manifest。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted && f.has("renovate-config-missing"),
        "bash#56"
    );
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]}"}, "renovateDashboard": {"totalCount": 1}}"#,
        "",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#57");
    // #465: Dashboard Issue が一度も無い。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]}"}, "renovateDashboard": {"totalCount": 0}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("renovate-dashboard-missing")
            && !f.has("renovate-policy-preset-missing"),
        "bash#58"
    );
    // renovateDashboard 欠落も 0 件扱い(fail-safe)。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]}"}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted && f.has("renovate-dashboard-missing"),
        "bash#59"
    );
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"dependencyDashboard\": false, \"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]}"}, "renovateDashboard": {"totalCount": 0}}"#,
        "",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#60");
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson5": {"text": "{ // comment\n  \"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]\n}"}, "renovateDashboard": {"totalCount": 1}}"#,
        "",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#61");
    // D4: extends が無い。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{}"}, "renovateDashboard": {"totalCount": 1}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted && f.missing == ["renovate-policy-preset-missing"],
        "bash#62"
    );
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{}"}, "renovateDashboard": {"totalCount": 0}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("renovate-dashboard-missing")
            && f.has("renovate-policy-preset-missing"),
        "bash#63"
    );
    // D4: #tag 固定は浮動でないので drift。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\"github>tarotene/dotfiles//renovate/policy#v1\"]}"}, "renovateDashboard": {"totalCount": 1}}"#,
        "",
    );
    assert!(
        f.verdict == Verdict::Drifted && f.has("renovate-policy-preset-missing"),
        "bash#64"
    );
    // `:disableDependencyDashboard` 省略形。
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\":disableDependencyDashboard\", \"github>tarotene/dotfiles//renovate/policy\"]}"}, "renovateDashboard": {"totalCount": 0}}"#,
        "",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#65");
    let f = r(
        r#"{"flakeNix": {"id": "x"}, "workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson5": {"text": "{ // comment\n  \"extends\": [\":disableDependencyDashboard\", \"github>tarotene/dotfiles//renovate/policy\"]\n}"}, "renovateDashboard": {"totalCount": 0}}"#,
        "",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#66");
    // Typst は manifest を持たないので primaryLanguage で判定する。
    let f = r(
        r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}]}}"#,
        "TeX",
    );
    assert_eq!(f.verdict, Verdict::NotApplicable, "bash#67");
    let f = r(
        r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}]}}"#,
        "Typst",
    );
    assert!(
        f.verdict == Verdict::Drifted && f.has("renovate-config-missing"),
        "bash#68"
    );
    let f = r(
        r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}]}, "renovateJson": {"text": "{\"extends\": [\"github>tarotene/dotfiles//renovate/policy\"]}"}, "renovateDashboard": {"totalCount": 1}}"#,
        "Typst",
    );
    assert_eq!(f.verdict, Verdict::Ok, "bash#69");
}

fn candidates(f: &Finding) -> Vec<String> {
    match &f.detail {
        Detail::Naming {
            lifecycle_candidates,
            ..
        } => lifecycle_candidates.clone(),
        _ => panic!("naming の finding ではない"),
    }
}

#[test]
fn naming_direct() {
    let vocab = NamingVocab {
        species: s(&["toolbox"]),
        registry: s(&["regcode"]),
        site_domains: s(&["known.site"]),
        lifecycle_species: Vec::new(),
    };
    let pre = "2020-01-01T00:00:00Z";
    let post = "2027-01-01T00:00:00Z";
    let n = |repo: &str, topics: &[&str], created: &str, v: &NamingVocab| {
        judge_naming(repo, &s(topics), created, v, "", false)
    };
    assert_eq!(
        n("foo-toolbox", &["naming-descriptive"], post, &vocab).verdict,
        Verdict::Ok,
        "bash#70"
    );
    assert!(
        n("foo-cleanup", &["naming-descriptive"], post, &vocab).has("species-unrecognized:cleanup"),
        "bash#71"
    );
    let no_species = NamingVocab {
        species: Vec::new(),
        ..vocab.clone()
    };
    assert_eq!(
        n("foo-ancient", &["naming-descriptive"], pre, &no_species).verdict,
        Verdict::Ok,
        "bash#72"
    );
    assert_eq!(
        n("regcode", &["naming-codename"], pre, &vocab).verdict,
        Verdict::Ok,
        "bash#73"
    );
    assert!(
        n("roguecode", &["naming-codename"], pre, &vocab).has("codename-not-registered"),
        "bash#74"
    );
    assert_eq!(
        n("known.site", &["naming-site"], post, &vocab).verdict,
        Verdict::Ok,
        "bash#75"
    );
    assert!(
        n("rogue.site", &["naming-site"], post, &vocab).has("domain-unrecognized"),
        "bash#76"
    );

    // #278(ADR-0026): naming-coined と lifecycle 軸。
    let v = NamingVocab {
        lifecycle_species: s(&["study", "research"]),
        ..vocab.clone()
    };
    let nl = |repo: &str, topics: &[&str], desc: &str, archived: bool| {
        judge_naming(repo, &s(topics), pre, &v, desc, archived)
    };
    let f = judge_naming("somecoinage", &s(&["naming-coined"]), post, &v, "", false);
    assert!(
        f.verdict == Verdict::Ok
            && matches!(&f.detail, Detail::Naming { class, .. } if class == "naming-coined"),
        "bash#77"
    );
    let f = judge_naming("some-coinage", &s(&["naming-coined"]), post, &v, "", false);
    assert!(f.has("pattern-mismatch:naming-coined"), "bash#78");
    let notes = "My graduate school study notes";
    let c = candidates(&nl("foo-archive", &["naming-descriptive"], notes, false));
    assert!(
        c.iter()
            .any(|x| x.starts_with("lifecycle-study-candidate:")),
        "bash#79"
    );
    assert!(!c.is_empty(), "bash#80");
    let c = candidates(&nl(
        "foo-archive",
        &["naming-descriptive", "lifecycle-study"],
        notes,
        false,
    ));
    assert!(
        !c.iter()
            .any(|x| x.starts_with("lifecycle-study-candidate:")),
        "bash#81"
    );
    let c = candidates(&nl(
        "plain-tool",
        &["naming-descriptive"],
        "A CLI for managing widgets",
        false,
    ));
    assert!(c.is_empty(), "bash#82");
    let c = candidates(&nl("pj-thing", &["naming-pj"], "", false));
    assert!(
        c.contains(&"lifecycle-timeboxed-candidate".to_string()),
        "bash#83"
    );
    let c = candidates(&nl(
        "pj-thing",
        &["naming-pj", "lifecycle-timeboxed"],
        "",
        false,
    ));
    assert!(
        !c.contains(&"lifecycle-timeboxed-candidate".to_string()),
        "bash#84"
    );
    let c = candidates(&nl(
        "pj-done",
        &["naming-pj", "lifecycle-timeboxed"],
        "",
        true,
    ));
    assert!(
        c.contains(&"lifecycle-timeboxed-removal-candidate".to_string()),
        "bash#85"
    );
    let c = candidates(&nl(
        "pj-live",
        &["naming-pj", "lifecycle-timeboxed"],
        "",
        false,
    ));
    assert!(
        !c.contains(&"lifecycle-timeboxed-removal-candidate".to_string()),
        "bash#86"
    );
}

fn score(f: &Finding) -> i64 {
    match f.detail {
        Detail::Lifecycle { score } => score,
        _ => panic!("lifecycle の finding ではない"),
    }
}

#[test]
fn lifecycle_direct() {
    let now = Some("2026-09-23T00:00:00Z");
    let release_recent = r#"{"latestRelease":{"nodes":[{"createdAt":"2026-08-01T00:00:00Z"}]}}"#;
    let release_stale_issue_stale = r#"{"latestRelease":{"nodes":[{"createdAt":"2025-01-01T00:00:00Z"}]},"openIssueActivity":{"totalCount":1,"nodes":[{"updatedAt":"2026-01-01T00:00:00Z"}]}}"#;
    let l =
        |pushed: &str, wf: bool, g: &str, ci: &str| judge_lifecycle(pushed, wf, &gql(g), ci, now);

    let f = l("2026-09-20T00:00:00Z", true, release_recent, "success");
    assert!(
        f.verdict == Verdict::Ok && score(&f) == 0 && f.missing.is_empty(),
        "bash#87: {f:?}"
    );
    // push 400 日停滞だけでは閾値未満(候補提示であって drift ではない)。
    let f = l("2025-08-19T00:00:00Z", true, release_recent, "success");
    assert!(
        f.verdict == Verdict::Ok && score(&f) == 2 && f.has("stale-push:400d"),
        "bash#88: {f:?}"
    );
    let f = l("2025-08-19T00:00:00Z", false, release_stale_issue_stale, "");
    assert!(
        f.verdict == Verdict::DormancyCandidate
            && score(&f) >= 3
            && f.has("stale-push:400d")
            && f.missing.iter().any(|m| m.starts_with("stale-release:"))
            && f.missing.iter().any(|m| m.starts_with("stale-issues:"))
            && f.has("ci-absent"),
        "bash#89: {f:?}"
    );
    let f = l("2026-09-20T00:00:00Z", true, "{}", "success");
    assert!(
        !f.missing.iter().any(|m| m.starts_with("stale-release:")),
        "bash#90: {f:?}"
    );
    let f = l(
        "2026-09-20T00:00:00Z",
        true,
        r#"{"openIssueActivity":{"totalCount":0,"nodes":[]}}"#,
        "success",
    );
    assert!(
        !f.missing.iter().any(|m| m.starts_with("stale-issues:")),
        "bash#91: {f:?}"
    );
    let f = l("2026-09-20T00:00:00Z", true, release_recent, "failure");
    assert!(f.has("ci-failing") && !f.has("ci-absent"), "bash#92: {f:?}");

    // any_drift: dormancy-candidate は drift ではない。
    let only = findings_from_json(r#"[{"repo":"x","language":"-","visibility":"PUBLIC","domains":{"lifecycle":{"verdict":"dormancy-candidate","missing":["stale-push:400d"],"score":4}}}]"#).unwrap();
    assert!(!any_drift(&only), "bash#93");
    let mixed = findings_from_json(r#"[{"repo":"x","language":"-","visibility":"PUBLIC","domains":{"lifecycle":{"verdict":"dormancy-candidate","missing":[],"score":4},"charters":{"verdict":"drifted","missing":["readme-missing"]}}}]"#).unwrap();
    assert!(any_drift(&mixed), "bash#94");
}

#[test]
fn settings_direct() {
    let meta: RepoMeta = serde_json::from_str(r#"{"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"deleteBranchOnMerge":true,"defaultBranchRef":{"name":"main"},"hasWikiEnabled":false,"hasProjectsEnabled":false}"#).unwrap();
    let rest = |v: Value| RepoRest::from_value(&v);
    let ok = rest(
        json!({"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"BLANK","allow_auto_merge":true,"security_and_analysis":{"dependabot_security_updates":{"status":"disabled"}}}),
    );
    let drift = rest(
        json!({"squash_merge_commit_title":"COMMIT_OR_PR_TITLE","squash_merge_commit_message":"COMMIT_MESSAGES","allow_auto_merge":false,"security_and_analysis":{"dependabot_security_updates":{"status":"enabled"}}}),
    );
    assert_eq!(judge_settings(&meta, &ok).verdict, Verdict::Ok, "bash#95");
    let f = judge_settings(&meta, &drift);
    assert!(f.has("squash-title-not-pr-title"), "bash#96");
    assert!(f.has("squash-message-not-blank"), "bash#97");
    assert!(f.has("auto-merge-disabled"), "bash#98");
    assert!(f.has("dependabot-security-updates-enabled"), "bash#99");
    // 取得不能({})は REST 由来の全項目を drift に倒す(黙って通さない)。
    let f = judge_settings(&meta, &rest(json!({})));
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("squash-title-not-pr-title")
            && f.has("squash-message-not-blank")
            && f.has("auto-merge-disabled")
            && f.has("dependabot-security-updates-enabled")
            && !f.has("merge-not-squash-only"),
        "bash#100: {f:?}"
    );
}

#[test]
fn titles_direct() {
    let fx = Fx::new();
    let gh = fx.gh();
    let both =
        gql(r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}, {"name": "pr-title.yml"}]}}"#);
    let ci_only = gql(r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}]}}"#);
    let t = |repo: &str, g: &RepoGql, jobs: &[&str]| judge_titles(&gh, repo, g, &s(jobs));

    assert_eq!(
        t("no-ci-repo", &gql(r#"{"workflowsDir": null}"#), &[]).verdict,
        Verdict::NotApplicable,
        "bash#101"
    );
    assert_eq!(t("titles-ok", &both, &[]).verdict, Verdict::Ok, "bash#102");
    assert!(
        t("titles-missing-check", &both, &[]).has("pr-title-check-not-required"),
        "bash#103"
    );
    assert!(
        t("titles-ok", &ci_only, &[]).has("pr-title-workflow-missing"),
        "bash#104"
    );
    // #337: workflow_call の連結名 "PR Title / PR title" を受ける。
    assert_eq!(
        t("titles-connected-ok", &both, &[]).verdict,
        Verdict::Ok,
        "bash#105"
    );
    // 2026-09-26: 接尾辞一致だけの "CI / PR title" は受けない。
    assert!(
        t("titles-old-suffix-form", &both, &[]).has("pr-title-check-not-required"),
        "bash#106"
    );
    // ground truth との突き合わせ(ADR-0031 Amendment)。
    assert!(
        t("titles-connected-ok", &both, &["check / PR title"]).has("pr-title-context-mismatch"),
        "bash#107"
    );
    assert!(
        t(
            "titles-connected-ok",
            &both,
            &["PR title (Conventional Commits)"]
        )
        .has("pr-title-context-mismatch"),
        "bash#108"
    );
    assert!(
        !t("titles-connected-ok", &both, &["PR Title / PR title"]).has("pr-title-context-mismatch"),
        "bash#109"
    );
    assert!(
        !t("titles-connected-ok", &both, &[]).has("pr-title-context-mismatch"),
        "bash#110"
    );
    assert!(
        !t("titles-ok", &both, &["PR title"]).has("pr-title-context-mismatch"),
        "bash#111"
    );
}

#[test]
fn workflows_direct() {
    let fx = Fx::new();
    // 正本はライブのテンプレートではなく固定の fixture(テンプレの中身が
    // 変わってもこのテストは壊れない)。
    let canonical = fx.p("fixtures/workflows-canonical-quality.json");
    let w = |g: &RepoGql| judge_workflows(g, &canonical);

    let wf_ok_ci = "name: CI\njobs:\n  gitleaks:\n    runs-on: ubuntu-latest\n    name: Secret scan (gitleaks)\n    steps:\n      - run: echo hi\n  ci-passed:\n    runs-on: ubuntu-latest\n    name: CI passed\n    needs: [gitleaks]\n    if: always()\n    steps:\n      - run: echo done\n";
    let wf_bad_ci = "name: ci\njobs:\n  gitleaks:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n  shellcheck:\n    runs-on: ubuntu-latest\n    name: shell check\n    steps:\n      - run: echo hi\n";
    let wf_incomplete_needs = "name: CI\njobs:\n  a:\n    name: A\n    steps: []\n  b:\n    name: B\n    steps: []\n  ci-passed:\n    name: CI passed\n    needs: [a]\n    steps: []\n";

    let f = w(&gql(r#"{"workflowsDir": {"entries": []}}"#));
    assert!(
        f.verdict == Verdict::Drifted && f.has("ci-yml-missing"),
        "bash#112"
    );
    let f = w(&gql(
        r#"{"workflowsDir": {"entries": [{"name": "Build.yml"}, {"name": "deploy.yaml"}]}}"#,
    ));
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("file-not-yml:deploy.yaml")
            && f.has("file-not-kebab:Build.yml"),
        "bash#113: {f:?}"
    );
    let q = r#"{"rules":[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI passed","integration_id":15368},{"context":"PR title","integration_id":15368}]}}]}"#;
    let f = w(&gqlv(
        json!({"workflowsDir": {"entries": [{"name": "ci.yml"}]}, "declCi": {"text": wf_ok_ci}, "declQuality": {"text": q}}),
    ));
    assert!(
        f.verdict == Verdict::Ok && f.missing.is_empty(),
        "bash#114: {f:?}"
    );

    let bad = w(&gqlv(
        json!({"workflowsDir": {"entries": [{"name": "ci.yml"}]}, "declCi": {"text": wf_bad_ci}}),
    ));
    assert!(bad.has("workflow-name-lowercase"), "bash#115: {bad:?}");
    assert!(bad.has("ci-passed-job-missing"), "bash#116: {bad:?}");
    assert!(bad.has("job-name-missing:gitleaks"), "bash#117: {bad:?}");
    assert!(
        bad.has("job-name-lowercase:shellcheck"),
        "bash#118: {bad:?}"
    );

    let f = w(&gqlv(
        json!({"workflowsDir": {"entries": [{"name": "ci.yml"}]}, "declCi": {"text": wf_incomplete_needs}}),
    ));
    assert!(f.has("ci-passed-needs-incomplete:b"), "bash#119: {f:?}");

    let f = w(&gql(
        r#"{"workflowsDir": {"entries": [{"name": "ci.yml"}]}, "declQuality": {"text": "{\"rules\":[{\"type\":\"required_status_checks\",\"parameters\":{\"required_status_checks\":[{\"context\":\"PR Title / PR title\"}]}}]}"}}"#,
    ));
    assert!(f.has("quality-json-not-canonical"), "bash#120: {f:?}");
    let f = w(&gql(
        r#"{"workflowsDir": {"entries": [{"name": "pr-title.yml"}]}, "declPrTitle": {"text": "jobs:\n  check:\n    uses: tarotene/dotfiles/.github/workflows/pr-title.yml@main\n"}}"#,
    ));
    assert!(f.has("legacy-reusable-pr-title-call"), "bash#121: {f:?}");
    let f = w(&gql(
        r#"{"workflowsDir": {"entries": [{"name": "pr-title.yml"}]}, "declPrTitle": {"text": "jobs:\n  pr-title:\n    steps:\n      - uses: tarotene/dotfiles/.github/actions/pr-title@main\n"}}"#,
    ));
    assert!(!f.has("legacy-reusable-pr-title-call"), "bash#122: {f:?}");
}

#[test]
fn docs_direct() {
    let none = RepoRest::default();
    let d = |repo: &str, vis: &str, g: &str| judge_docs(repo, vis, &gql(g), &none);
    let ci_rust = r#""declCi": {"text": "jobs:\n  docs:\n    steps:\n      - uses: tarotene/dotfiles/.github/actions/docs-rust@main\n"}"#;

    assert_eq!(
        d("docs-none", "PUBLIC", r#"{"declCi": {"text": "jobs: {}"}}"#).verdict,
        Verdict::NotApplicable,
        "bash#123"
    );
    assert_eq!(
        d(
            "docs-rust-ok",
            "PUBLIC",
            &format!(r#"{{{ci_rust}, "cargoToml": {{"id": "x"}}}}"#)
        )
        .verdict,
        Verdict::Ok,
        "bash#124"
    );
    let f = d(
        "docs-rust-absent",
        "PUBLIC",
        r#"{"cargoToml": {"id": "x"}, "declCi": {"text": "jobs: {}"}}"#,
    );
    assert!(
        f.verdict == Verdict::Drifted && f.has("docs-absent:rust"),
        "bash#125"
    );
    let f = d("docs-rust-no-ci", "PUBLIC", r#"{"cargoToml": {"id": "x"}}"#);
    assert!(
        f.verdict == Verdict::Drifted && f.has("docs-absent:rust"),
        "bash#126"
    );
    let f = d(
        "docs-rust-ref",
        "PUBLIC",
        r#"{"cargoToml": {"id": "x"}, "declCi": {"text": "      - uses: tarotene/dotfiles/.github/actions/docs-rust@v1\n"}}"#,
    );
    assert!(f.has("docs-wrong-ref:rust"), "bash#127");
    // dotfiles は自前の action を相対パスで適用する(ADR-640 D2)。
    let rel = r#"{"cargoToml": {"id": "x"}, "declCi": {"text": "      - uses: ./.github/actions/docs-rust\n"}}"#;
    assert_eq!(
        d("dotfiles", "PUBLIC", rel).verdict,
        Verdict::Ok,
        "bash#128"
    );
    assert!(
        d("docs-rust-relative", "PUBLIC", rel).has("docs-absent:rust"),
        "bash#129"
    );
    assert!(
        d(
            "docs-python",
            "PUBLIC",
            r#"{"pyprojectToml": {"id": "x"}, "declCi": {"text": "jobs: {}"}}"#
        )
        .has("docs-absent:python"),
        "bash#130"
    );
    // package.json は exports / main / types を宣言するライブラリだけが対象。
    assert!(
        d("docs-ts-lib", "PUBLIC", r#"{"packageJson": {"id": "x", "text": "{\"name\":\"a\",\"types\":\"i.d.ts\"}"}, "declCi": {"text": "jobs: {}"}}"#)
            .has("docs-absent:typescript"),
        "bash#131"
    );
    assert_eq!(
        d("docs-ts-site", "PUBLIC", r#"{"packageJson": {"id": "x", "text": "{\"name\":\"site\",\"type\":\"module\"}"}, "declCi": {"text": "jobs: {}"}}"#).verdict,
        Verdict::NotApplicable,
        "bash#132"
    );
    assert_eq!(
        d(
            "docs-ts-garbage",
            "PUBLIC",
            r#"{"packageJson": {"id": "x", "text": "{not json"}, "declCi": {"text": "jobs: {}"}}"#
        )
        .verdict,
        Verdict::NotApplicable,
        "bash#133"
    );
    let f = d(
        "docs-multi",
        "PUBLIC",
        &format!(r#"{{{ci_rust}, "cargoToml": {{"id": "x"}}, "pyprojectToml": {{"id": "y"}}}}"#),
    );
    assert!(
        f.has("docs-absent:python") && !f.has("docs-absent:rust"),
        "bash#134: {f:?}"
    );
    // PRIVATE の Pages 有効はスタックに関わらず drift(ADR-640 D7)。
    let pages = |on: bool| RepoRest {
        has_pages: Some(on),
        ..Default::default()
    };
    let jobs = gql(r#"{"declCi": {"text": "jobs: {}"}}"#);
    let f = judge_docs("docs-private-pages", "PRIVATE", &jobs, &pages(true));
    assert!(
        f.verdict == Verdict::Drifted && f.has("private-pages-enabled"),
        "bash#135"
    );
    assert_eq!(
        judge_docs("docs-public-pages", "PUBLIC", &jobs, &pages(true)).verdict,
        Verdict::NotApplicable,
        "bash#136"
    );
    assert_eq!(
        judge_docs("docs-private-nopages", "PRIVATE", &jobs, &pages(false)).verdict,
        Verdict::NotApplicable,
        "bash#137"
    );
}

fn rel(has_workflow: bool, secrets: &[&str]) -> ReleaserInput {
    ReleaserInput {
        has_workflow,
        secrets: s(secrets),
        ..Default::default()
    }
}

#[test]
fn releaser_and_routines_direct() {
    let f = judge_releaser(&rel(false, &[]));
    assert!(
        f.verdict == Verdict::NotApplicable && f.missing.is_empty(),
        "bash#138"
    );
    let ok = ["RELEASER_APP_CLIENT_ID", "RELEASER_APP_PRIVATE_KEY"];
    let f = judge_releaser(&rel(true, &ok));
    assert!(f.verdict == Verdict::Ok && f.missing.is_empty(), "bash#139");
    let f = judge_releaser(&rel(
        true,
        &["RELEASE_PLZ_APP_ID", "RELEASE_PLZ_APP_PRIVATE_KEY"],
    ));
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-secret-name-legacy"),
        "bash#140"
    );
    let f = judge_releaser(&rel(
        true,
        &["RELEASE_PLEASE_APP_ID", "RELEASE_PLEASE_APP_PRIVATE_KEY"],
    ));
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-secret-name-legacy"),
        "bash#141"
    );
    let f = judge_releaser(&rel(true, &[]));
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-app-secrets-missing"),
        "bash#142"
    );
    let f = judge_releaser(&rel(true, &["RELEASER_APP_CLIENT_ID"]));
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-app-secrets-missing"),
        "bash#143"
    );

    // routines(ADR-519)。sources には tarotene/routines-ok だけが入っている。
    let fx = Fx::new();
    let sources = read_closed_set(
        &fx.p("config/routines-auditor-sources.tsv"),
        Some(&fx.p("config/routines-auditor-sources.local.tsv")),
    );
    let r = |repo: &str, g: &str| judge_routines("tarotene", repo, &gql(g), &sources);
    let f = r("no-routines-dir", "{}");
    assert!(
        f.verdict == Verdict::NotApplicable && f.missing.is_empty(),
        "bash#144"
    );
    let f = r(
        "routines-dir-no-json",
        r#"{"routinesDir":{"entries":[{"name":"README.md","type":"blob"}]}}"#,
    );
    assert_eq!(f.verdict, Verdict::NotApplicable, "bash#145");
    let decl = r#"{"routinesDir":{"entries":[{"name":"nightly-check.json","type":"blob"}]}}"#;
    let f = r("routines-ok", decl);
    assert!(f.verdict == Verdict::Ok && f.missing.is_empty(), "bash#146");
    let f = r("routines-orphaned", decl);
    assert!(
        f.verdict == Verdict::Drifted && f.has("routines-sources-missing"),
        "bash#147"
    );

    // #615 / #633
    let f = judge_releaser(&rel(true, &["RELEASER_APP_ID", "RELEASER_APP_PRIVATE_KEY"]));
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("releaser-app-id-deprecated")
            && !f.has("releaser-app-secrets-missing"),
        "bash#148: {f:?}"
    );
    let all3 = [
        "RELEASER_APP_ID",
        "RELEASER_APP_CLIENT_ID",
        "RELEASER_APP_PRIVATE_KEY",
    ];
    let f = judge_releaser(&rel(true, &all3));
    assert!(
        f.verdict == Verdict::Advisory && f.missing == ["releaser-app-id-leftover"],
        "bash#149: {f:?}"
    );
    let f = judge_releaser(&ReleaserInput {
        snapshot_present: true,
        installed: false,
        ..rel(true, &all3)
    });
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("releaser-app-not-installed")
            && f.has("releaser-app-id-leftover"),
        "bash#150: {f:?}"
    );

    // #613: claim / 標準外の参照
    let f = judge_releaser(&ReleaserInput {
        refs: s(&["vars.RELEASE_APP_ID", "secrets.RELEASE_APP_PRIVATE_KEY"]),
        ..rel(true, &ok)
    });
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-workflow-refs-nonstandard"),
        "bash#151"
    );
    let f = judge_releaser(&ReleaserInput {
        refs: s(&[
            "secrets.RELEASER_APP_CLIENT_ID",
            "secrets.RELEASER_APP_PRIVATE_KEY",
        ]),
        ..rel(true, &ok)
    });
    assert_eq!(f.verdict, Verdict::Ok, "bash#152");

    // #673: release workflow の最新の完了 run。
    let concl = |secrets: &[&str], has: bool, c: &str| {
        judge_releaser(&ReleaserInput {
            release_conclusion: c.into(),
            ..rel(has, secrets)
        })
    };
    let f = concl(&ok, true, "failure");
    assert!(
        f.verdict == Verdict::Drifted && f.has("releaser-release-failing"),
        "bash#153"
    );
    assert_eq!(concl(&ok, true, "success").verdict, Verdict::Ok, "bash#154");
    assert_eq!(concl(&ok, true, "").verdict, Verdict::Ok, "bash#155");
    let f = concl(&[], true, "failure");
    assert!(
        f.verdict == Verdict::Drifted
            && f.has("releaser-release-failing")
            && f.has("releaser-app-secrets-missing"),
        "bash#156: {f:?}"
    );
    assert_eq!(
        concl(&[], false, "failure").verdict,
        Verdict::NotApplicable,
        "bash#157"
    );
    let refs_gql = gql(
        r#"{"workflowsDir":{"entries":[{"name":"ci.yml","object":{"text":"jobs: {}"}},{"name":"other.yml","object":{"text":"uses: actions/create-github-app-token@v2\n with:\n  app-id: ${{ secrets.OTHER_APP_ID }}\n  private-key: ${{ secrets.OTHER_APP_KEY }}"}}]}}"#,
    );
    assert!(releaser_workflow_refs(&refs_gql).is_empty(), "bash#158");

    // app-snapshot.json による install 判定(ADR-590、ADR-436 Amendment)。
    let snap = |has: bool, secrets: &[&str], installed: bool| {
        judge_releaser(&ReleaserInput {
            snapshot_present: true,
            installed,
            ..rel(has, secrets)
        })
    };
    let f = snap(true, &ok, false);
    assert!(
        f.verdict == Verdict::Drifted
            && !f.has("releaser-app-secrets-missing")
            && f.has("releaser-app-not-installed"),
        "bash#159: {f:?}"
    );
    let f = snap(true, &ok, true);
    assert!(
        f.verdict == Verdict::Ok && f.missing.is_empty(),
        "bash#160: {f:?}"
    );
    let f = snap(false, &[], true);
    assert!(
        f.verdict == Verdict::Advisory && f.has("releaser-app-installed-unclaimed"),
        "bash#161: {f:?}"
    );
    assert_eq!(
        snap(false, &[], false).verdict,
        Verdict::NotApplicable,
        "bash#162"
    );
}
