//! bash 版 `--selftest` のうち、バイナリを通しで動かす検査(bash#1〜#48、
//! bash#163〜#178)。`bash#N` は bash 版 selftest の N 番目の FAIL 検査に
//! 1 対 1 で対応する(番号は selftest() 内の出現順)。

mod common;

use common::{write_exec, Fx, Out};
use github_audit::{findings_from_json, Domain, Finding, RepoFindings, ReviewLayer, Verdict};
use std::fs;

fn parse(out: &Out) -> Vec<RepoFindings> {
    findings_from_json(out.stdout.trim_end()).unwrap_or_else(|| {
        panic!(
            "--json の出力が読めない: stdout={} stderr={}",
            out.stdout, out.stderr
        )
    })
}

fn get<'a>(fs: &'a [RepoFindings], repo: &str, d: Domain) -> &'a Finding {
    fs.iter()
        .find(|r| r.repo == repo)
        .unwrap_or_else(|| panic!("repo {repo} が無い"))
        .domain(d)
        .unwrap_or_else(|| panic!("{repo} に {} が無い", d.as_str()))
}

#[test]
fn main_fixture_pipeline() {
    let fx = Fx::new();
    let out = fx.rust(&["--json"], &[]);
    let fs = parse(&out);
    let f = |repo: &str, d: Domain| get(&fs, repo, d).clone();

    // naming
    assert_eq!(f("all-ok", Domain::Naming).verdict, Verdict::Ok, "bash#1");
    assert!(
        f("all-drifted", Domain::Naming).has("class-undeclared"),
        "bash#2"
    );
    assert!(
        f("pj-ambiguous", Domain::Naming).has("class-ambiguous"),
        "bash#3"
    );
    assert!(
        f("not-pj-prefixed", Domain::Naming).has("pattern-mismatch:naming-pj"),
        "bash#4"
    );
    assert_eq!(
        f(".github", Domain::Naming).verdict,
        Verdict::Exempt,
        "bash#5"
    );

    // settings
    assert_eq!(f("all-ok", Domain::Settings).verdict, Verdict::Ok, "bash#6");
    let s = f("all-drifted", Domain::Settings);
    assert!(s.has("merge-not-squash-only"), "bash#7");
    assert!(s.has("default-branch-not-main"), "bash#8");
    // all-drifted の REST fixture は has_pages しか持たない — フィールド欠落は
    // drift 側(ADR-568 D2/D5b)。
    assert!(s.has("auto-merge-disabled"), "bash#9");
    assert!(s.has("dependabot-security-updates-enabled"), "bash#10");

    // docs(GraphQL の cargoToml → docs_stacks_for、REST の has_pages は PRIVATE のみ)
    let d = f("all-ok", Domain::Docs);
    assert!(
        d.verdict == Verdict::Drifted
            && d.has("docs-absent:rust")
            && !d.has("private-pages-enabled"),
        "bash#11: {d:?}"
    );
    let d = f("all-drifted", Domain::Docs);
    assert!(
        d.has("docs-absent:rust") && d.has("private-pages-enabled"),
        "bash#12: {d:?}"
    );

    // renovate
    assert_eq!(
        f("all-ok", Domain::Renovate).verdict,
        Verdict::Ok,
        "bash#13"
    );
    assert_eq!(
        f("not-pj-prefixed", Domain::Renovate).verdict,
        Verdict::Drifted,
        "bash#14"
    );
    assert_eq!(
        f("no-renovate-target", Domain::Renovate).verdict,
        Verdict::NotApplicable,
        "bash#15"
    );

    // charters
    let c = f("all-ok", Domain::Charters);
    assert_eq!(c.verdict, Verdict::Ok, "bash#16: {c:?}");
    let c = f("all-drifted", Domain::Charters);
    assert!(c.has("readme-missing"), "bash#17");
    assert!(c.has("contributing-missing"), "bash#18");
    assert!(c.has("root-doc-not-allowlisted:CONTEXT.md"), "bash#19");
    assert!(c.has("claude-md-not-routed"), "bash#20");
    assert!(c.has("skills-not-routed:bar"), "bash#21");
    // ADR-0017: 廃止された "## Issue litmus" は許可外の見出しとして拾う。
    let c = f("pj-ambiguous", Domain::Charters);
    assert!(c.has("contributing-stray-heading:Issue litmus"), "bash#22");
    assert!(
        c.has("contributing-schema-incomplete-or-out-of-order"),
        "bash#23"
    );

    // nav-doc(ADR-0033)。all-ok は使われている path-inventory 除外を持つ —
    // ok のままなら、除外が効くことと unused と誤判定しないことの両方が言える。
    let c = f("all-ok", Domain::Charters);
    assert_eq!(c.verdict, Verdict::Ok, "bash#24: {c:?}");
    let c = f("all-drifted", Domain::Charters);
    assert!(
        c.has("nav-doc-path-inventory:CLAUDE.md:Structure:4"),
        "bash#25: {:?}",
        c.missing
    );
    let c = f("no-renovate-target", Domain::Charters);
    assert!(
        c.has("nav-doc-tree-fence:README.md"),
        "bash#26: {:?}",
        c.missing
    );
    assert!(
        c.has("nav-doc-exempt-unused:README.md:tree-fence"),
        "bash#27: {:?}",
        c.missing
    );
    assert!(
        !c.missing
            .iter()
            .any(|m| m.starts_with("nav-doc-path-inventory:README.md:Scope")),
        "bash#28: {:?}",
        c.missing
    );
    assert!(
        c.has("nav-doc-exempt-malformed:CLAUDE.md"),
        "bash#29: {:?}",
        c.missing
    );

    // rulesets(ADR-0021 review layer)
    assert_eq!(
        f("all-ok", Domain::Rulesets).review_layer(),
        Some(ReviewLayer::Complete),
        "bash#30"
    );
    // ADR-503: 宣言の無い fixture は rulesets-declaration-missing も出る(正しい)。
    let r = f("rulesets-core-only", Domain::Rulesets);
    assert!(
        r.verdict == Verdict::Drifted
            && r.review_layer() == Some(ReviewLayer::Absent)
            && r.missing == ["rulesets-declaration-missing"],
        "bash#31: {r:?}"
    );
    let r = f("rulesets-partial-review", Domain::Rulesets);
    assert!(
        r.verdict == Verdict::Drifted && r.review_layer() == Some(ReviewLayer::PartialDrift),
        "bash#32"
    );
    assert!(
        r.has("review_layer.required_review_thread_resolution"),
        "bash#33"
    );
    assert!(!r.has("review_layer.copilot_code_review"), "bash#34");

    // 2 つの active ruleset にまたがる集約(OR と積集合)。
    let r = f("rulesets-review-split", Domain::Rulesets);
    assert!(
        r.verdict == Verdict::Drifted
            && r.review_layer() == Some(ReviewLayer::Complete)
            && r.missing == ["rulesets-declaration-missing"],
        "bash#35: {r:?}"
    );
    let r = f("rulesets-squash-conflict", Domain::Rulesets);
    assert!(
        r.verdict == Verdict::Drifted && r.has("pull_request.allowed_merge_methods"),
        "bash#36: {r:?}"
    );
    // #349: 別々の 2 ruleset が pull_request を持ち review layer は無い → 重複。
    assert_eq!(
        r.missing
            .iter()
            .filter(|m| m.starts_with("duplicate-ruleset:pull_request:"))
            .count(),
        1,
        "bash#37: {:?}",
        r.missing
    );
    let split = f("rulesets-review-split", Domain::Rulesets);
    assert!(
        !split
            .missing
            .iter()
            .any(|m| m.starts_with("duplicate-ruleset:")),
        "bash#38: {:?}",
        split.missing
    );
    assert_eq!(r.review_layer(), Some(ReviewLayer::Absent), "bash#39");

    // exempt の短絡
    let ex = fs.iter().find(|r| r.repo == "exempted").unwrap();
    assert!(
        ex.domains.iter().all(|(_, f)| f.verdict == Verdict::Exempt),
        "bash#40"
    );

    // overrides を空にすると exempted も自前の drift で判定される(短絡して
    // いるだけで、たまたま同じ結果だったわけではないことの回帰)。
    fx.write("config/overrides.tsv", "");
    let fs2 = parse(&fx.rust(&["--json"], &[]));
    let c = get(&fs2, "exempted", Domain::Charters);
    assert!(
        c.verdict == Verdict::Drifted && c.has("readme-missing"),
        "bash#41"
    );
    let n = get(&fs2, "exempted", Domain::Naming);
    assert!(
        n.verdict == Verdict::Drifted && n.has("class-undeclared"),
        "bash#42"
    );
    fx.write("config/overrides.tsv", "exempted\t*\texempt\n");

    // ledger + human report
    let ledger = fx.p("state/ledger.json");
    assert!(ledger.is_file(), "bash#43");
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    assert_eq!(v["repos"].as_array().map(Vec::len), Some(11), "bash#44");

    let rep = fx.rust(&[], &[]);
    assert_ne!(rep.code, 0, "bash#45");
    assert!(rep.stdout.contains("repo=all-ok"), "bash#46");

    // 要求したドメインだけを判定する
    let only = parse(&fx.rust(&["--json", "rulesets"], &[]));
    assert_eq!(
        get(&only, "all-ok", Domain::Rulesets).verdict,
        Verdict::Ok,
        "bash#47: {:?}",
        get(&only, "all-ok", Domain::Rulesets)
    );
    let keys: Vec<Domain> = only
        .iter()
        .find(|r| r.repo == "all-ok")
        .unwrap()
        .domains
        .iter()
        .map(|(d, _)| *d)
        .collect();
    assert_eq!(keys, vec![Domain::Rulesets], "bash#48");
}

/// app-snapshot.json(ADR-590、ADR-436 Amendment 2026-09-30)を audit() が
/// 実際に読み、releaser の verdict に反映すること。
#[test]
fn snapshot_pipeline() {
    let fx = Fx::new();
    fx.overlay("snapshot");
    let snap = fx.p("state/app-snapshot.json");
    let out = fx.rust(
        &["--json", "releaser"],
        &[("GITHUB_AUDIT_APP_SNAPSHOT_FILE", snap.to_str().unwrap())],
    );
    let fs = parse(&out);
    let r = get(&fs, "snap-installed", Domain::Releaser);
    assert_eq!(r.verdict, Verdict::Ok, "bash#163: {r:?}");
    let r = get(&fs, "snap-not-installed", Domain::Releaser);
    assert!(
        r.verdict == Verdict::Drifted && r.has("releaser-app-not-installed"),
        "bash#164: {r:?}"
    );
}

/// REST 経由の secret 名取得を含む releaser の配線(#613 の改名 workflow も)。
#[test]
fn releaser_pipeline() {
    let fx = Fx::new();
    fx.overlay("snapshot");
    // bash 版は次の段の前に snapshot を消す(以後は snapshot 無しの
    // secret-only フォールバックが前提)。
    fs::remove_file(fx.p("state/app-snapshot.json")).unwrap();
    fx.overlay("releaser");
    let fs = parse(&fx.rust(&["--json"], &[]));
    let r = get(&fs, "releaser-ok", Domain::Releaser);
    assert_eq!(r.verdict, Verdict::Ok, "bash#165: {r:?}");
    let r = get(&fs, "releaser-legacy", Domain::Releaser);
    assert!(
        r.verdict == Verdict::Drifted && r.has("releaser-secret-name-legacy"),
        "bash#166: {r:?}"
    );
    let r = get(&fs, "releaser-missing", Domain::Releaser);
    assert!(
        r.verdict == Verdict::Drifted && r.has("releaser-app-secrets-missing"),
        "bash#167: {r:?}"
    );
    let r = get(&fs, "releaser-renamed", Domain::Releaser);
    assert!(
        r.verdict == Verdict::Drifted
            && r.has("releaser-workflow-refs-nonstandard")
            && r.has("releaser-app-secrets-missing"),
        "bash#168: {r:?}"
    );
    let r = get(&fs, "releaser-na", Domain::Releaser);
    assert_eq!(r.verdict, Verdict::NotApplicable, "bash#169: {r:?}");
}

/// #278: archived は naming 以外 not-applicable、naming は lifecycle 候補を出す。
#[test]
fn archived_pipeline() {
    let fx = Fx::new();
    fx.overlay("archived");
    let fs = parse(&fx.rust(&["--json"], &[]));
    assert_eq!(
        get(&fs, "archived-repo", Domain::Rulesets).verdict,
        Verdict::NotApplicable,
        "bash#170"
    );
    assert_eq!(
        get(&fs, "archived-repo", Domain::Charters).verdict,
        Verdict::NotApplicable,
        "bash#171"
    );
    assert_eq!(
        get(&fs, "archived-repo", Domain::Lifecycle).verdict,
        Verdict::NotApplicable,
        "bash#172"
    );
    let n = get(&fs, "archived-repo", Domain::Naming);
    let study = matches!(&n.detail, github_audit::Detail::Naming { lifecycle_candidates, .. }
        if lifecycle_candidates.iter().any(|c| c.starts_with("lifecycle-study-candidate:")));
    assert!(n.verdict == Verdict::Ok && study, "bash#173: {n:?}");
}

/// bash 版の gh-viewer-stub。ログの書き先は埋め込む(ライブラリ直呼びで
/// 環境変数を渡せないため)。
fn viewer_stub(log: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
if [[ "$1" == "repo" && "$2" == "list" ]]; then
  printf '%s\n' "$*" >>"{log}"
  printf '[{{"name":"admin-repo","viewerPermission":"ADMIN"}},{{"name":"write-repo","viewerPermission":"WRITE"}}]\n'
else
  exit 1
fi
"#
    )
}

/// #533: GITHUB_AUDIT_VIEWER_PERMISSION / GITHUB_AUDIT_REPO_LIMIT を
/// list_repos_meta 単体で検査する(bash 版も関数だけを source して叩く)。
#[test]
fn list_repos_meta_knobs() {
    let fx = Fx::new();
    let stub = fx.p("bin/gh-viewer-stub");
    let log = fx.p("gh-viewer.log");
    write_exec(&stub, &viewer_stub(log.to_str().unwrap()));
    let gh = github_audit::Gh::new(stub.to_str().unwrap(), "tarotene");

    let got = gh.list_repos_meta("500", Some("ADMIN")).unwrap();
    let want = vec![github_audit::RepoMeta {
        name: "admin-repo".into(),
        viewer_permission: Some("ADMIN".into()),
        ..Default::default()
    }];
    assert_eq!(got, want, "bash#174");
    assert!(
        fs::read_to_string(&log).unwrap().contains("--limit 500"),
        "bash#175"
    );

    fs::write(&log, "").unwrap();
    // REPO_LIMIT は Config::from_env が読む値 — バイナリ経由で環境変数から通す。
    fx.rust(
        &["--json", "naming"],
        &[
            ("GITHUB_AUDIT_GH_BIN", stub.to_str().unwrap()),
            ("GITHUB_AUDIT_REPO_LIMIT", "37"),
        ],
    );
    assert!(
        fs::read_to_string(&log).unwrap().contains("--limit 37"),
        "bash#176"
    );
}

/// #614: gh repo list の失敗は「0 repo で正常終了」ではなく abort。
#[test]
fn list_failure_aborts() {
    let fx = Fx::new();
    let stub = fx.p("bin/gh-list-fail-stub");
    write_exec(
        &stub,
        "#!/usr/bin/env bash\nprintf 'GraphQL: API rate limit already exceeded\\n' >&2\nexit 1\n",
    );
    // ライブラリ直呼び(bash 版も list_repos_meta を source して直接叩く)。
    let gh = github_audit::Gh::new(stub.to_str().unwrap(), "tarotene");
    assert!(gh.list_repos_meta("500", None).is_err(), "bash#177");
    // stderr の中身はバイナリ経由で見る(gh の理由と abort の両方)。
    let out = fx.rust(
        &["--json", "naming"],
        &[("GITHUB_AUDIT_GH_BIN", stub.to_str().unwrap())],
    );
    assert!(
        out.code == 1
            && out.stderr.contains("rate limit already exceeded")
            && out.stderr.contains("aborting"),
        "bash#178: {}",
        out.stderr
    );
    assert!(!fx.p("state/ledger.json").exists());
}
