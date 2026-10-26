//! pr-merge-settings-check(かつての `scripts/pr-merge-settings-check --selftest` の 10 件)。

mod common;
use common::{run, Stub};

const BIN: &str = env!("CARGO_BIN_EXE_pr-merge-settings-check");
const BASE: &str = r#""allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"delete_branch_on_merge":true,"default_branch":"main","has_wiki":false,"has_projects":false,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"BLANK""#;

fn check(stub: &Stub, owner_repo: &str, rest_json: &str) -> common::Run {
    stub.put(&format!("repos/{owner_repo}"), rest_json);
    run(
        BIN,
        &[("GITHUB_AUDIT_GH_BIN", stub.gh().to_str().unwrap())],
        &[owner_repo],
    )
}

fn json(extra: &str) -> String {
    format!("{{{BASE},{extra}}}")
}

#[test]
fn all_items_ok() {
    let s = Stub::new();
    let r = check(
        &s,
        "tarotene/all-ok",
        &json(
            r#""allow_auto_merge":true,"security_and_analysis":{"dependabot_security_updates":{"status":"disabled"}}"#,
        ),
    );
    assert_eq!(r.code, 0, "{}", r.both());
    assert_eq!(r.out, "pr-merge-settings-check: tarotene/all-ok OK\n");
}

#[test]
fn squash_title_drift_is_red() {
    let s = Stub::new();
    let rest = r#"{"allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"delete_branch_on_merge":true,"default_branch":"main","has_wiki":false,"has_projects":false,"squash_merge_commit_title":"COMMIT_OR_PR_TITLE","squash_merge_commit_message":"COMMIT_MESSAGES","allow_auto_merge":true}"#;
    let r = check(&s, "tarotene/single-commit-drift", rest);
    assert_eq!(r.code, 1);
    assert!(
        r.err
            .contains("squash-title-not-pr-title, squash-message-not-blank"),
        "{}",
        r.err
    );
}

#[test]
fn merge_commit_allowed_is_red() {
    let s = Stub::new();
    let rest = r#"{"allow_squash_merge":true,"allow_merge_commit":true,"allow_rebase_merge":false,"delete_branch_on_merge":true,"default_branch":"main","has_wiki":false,"has_projects":false,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"BLANK","allow_auto_merge":true}"#;
    let r = check(&s, "tarotene/merge-allowed", rest);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("merge-not-squash-only"));
}

#[test]
fn fetch_failure_is_fail_closed() {
    let s = Stub::new();
    s.fail("repos/tarotene/unreachable");
    let r = run(
        BIN,
        &[("GITHUB_AUDIT_GH_BIN", s.gh().to_str().unwrap())],
        &["tarotene/unreachable"],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("取得に失敗しました"), "{}", r.err);
}

/// GITHUB_TOKEN 相当: `security_and_analysis` ブロックが丸ごと欠落(admin 権限が無い
/// レスポンス)していても、他の全項目が満たされていれば ok(dependabot-security-updates-enabled
/// は除外対象)。
#[test]
fn missing_security_and_analysis_is_ok_when_everything_else_is() {
    let s = Stub::new();
    let r = check(
        &s,
        "tarotene/no-admin-scope",
        &json(r#""allow_auto_merge":true"#),
    );
    assert_eq!(r.code, 0, "{}", r.both());
}

/// 除外は dependabot-security-updates-enabled 単体のみで、他の drift は引き続き red にする。
#[test]
fn missing_security_and_analysis_plus_a_real_drift_stays_red() {
    let s = Stub::new();
    let r = check(
        &s,
        "tarotene/no-admin-scope-plus-drift",
        &json(r#""allow_auto_merge":false"#),
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("auto-merge-disabled"));
    assert!(
        !r.err.contains("dependabot-security-updates-enabled"),
        "{}",
        r.err
    );
}

/// `plan-free-private` 宣言(docs/agents/free-plan-declaration.md)。
#[test]
fn plan_free_private_declaration() {
    let s = Stub::new();
    let r = check(
        &s,
        "acme/free",
        &json(r#""private":true,"topics":["plan-free-private"],"allow_auto_merge":false"#),
    );
    assert_eq!(r.code, 0, "宣言あり・自動マージ無効・非公開: {}", r.both());

    let r = check(
        &s,
        "acme/undeclared",
        &json(r#""private":true,"topics":["naming-descriptive"],"allow_auto_merge":false"#),
    );
    assert_eq!(r.code, 1, "宣言なし・自動マージ無効は従来どおり red");
    assert!(r.err.contains("auto-merge-disabled"));

    let r = check(
        &s,
        "acme/upgraded",
        &json(r#""private":true,"topics":["plan-free-private"],"allow_auto_merge":true"#),
    );
    assert_eq!(r.code, 1, "Team 切替後の外し忘れ");
    assert!(r.err.contains("plan-free-private-stale"));

    let r = check(
        &s,
        "acme/now-public",
        &json(r#""private":false,"topics":["plan-free-private"],"allow_auto_merge":false"#),
    );
    assert_eq!(r.code, 1, "宣言あり・公開リポジトリ");
    assert!(r.err.contains("plan-free-private-stale"));
}

#[test]
fn usage_and_selftest_notice() {
    let r = run(BIN, &[], &["--help"]);
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with("usage: pr-merge-settings-check"));
    let r = run(BIN, &[], &[]);
    assert_eq!(r.code, 1);
    let r = run(BIN, &[], &["not-a-repo"]);
    assert_eq!(r.code, 2);
    assert!(r.err.starts_with("usage:"));
    let r = run(BIN, &[], &["--selftest"]);
    assert_eq!(r.code, 0);
    assert!(r.err.contains("cargo test"));
}
