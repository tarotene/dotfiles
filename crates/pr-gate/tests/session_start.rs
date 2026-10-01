//! SessionStart hook の統合テスト(旧 `pr-gate.sh --selftest` の SessionStart /
//! hygiene ケース)。各 `// ok …` コメントが旧 selftest の check 1 件に対応する。

mod common;
use common::*;

fn with_pr(fx: &Fx, sid: &str) -> common::Run {
    let rules = fx.write("rules-2.json", RULES_2);
    let checks = fx.write("checks-2pass.json", CHECKS_2PASS);
    fx.session_start(
        sid,
        &[
            ("PR_GATE_STUB_HEAD_OID", &fx.real_head),
            ("PR_GATE_STUB_RULES_FILE", &rules),
            ("PR_GATE_STUB_CHECKS_FILE", &checks),
        ],
    )
}

fn no_pr(fx: &Fx, sid: &str) -> common::Run {
    fx.session_start(sid, &[("PR_GATE_STUB_NO_PR", "1")])
}

#[test]
fn summary_with_pr() {
    let fx = Fx::new();
    let r = with_pr(&fx, "ss-sid");
    assert_eq!(r.code, 0); // ok   session-start: exit 0
    let ctx = r.ctx();
    assert!(ctx.contains("PR #37"), "{ctx}"); // ok   session-start: PR 番号が出る
    assert!(ctx.contains("ahead 2")); // ok   session-start: base 追従の ahead が出る(2 commit 進めた)
    assert!(ctx.contains("未 push: 0 件")); // ok   session-start: 未 push 0 件
    assert!(ctx.contains("Issue リンク: closing keyword あり")); // ok   session-start: Issue リンクの状態が出る
    assert!(ctx.contains("視覚証跡: No-Visual: 宣言あり")); // ok   session-start: 視覚証跡の状態が出る
}

#[test]
fn summary_output_is_jq_shaped() {
    // 移植時に追加: `jq -n --arg ctx …` の整形出力とバイト一致すること。
    let fx = Fx::new();
    let r = with_pr(&fx, "ss-shape-sid");
    assert!(r
        .stdout
        .starts_with("{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"SessionStart\",\n    \"additionalContext\": \"[pr-gate] PR #37 (base: main) — CI: pass: 2\\nIssue リンク: closing keyword あり\\n"));
    assert!(
        r.stdout.ends_with("未 push: 0 件\"\n  }\n}\n"),
        "{}",
        r.stdout
    );
}

#[test]
fn no_pr_without_hygiene_is_silent() {
    let fx = Fx::new();
    let r = no_pr(&fx, "ss-nopr-sid");
    assert_eq!(r.code, 0); // ok   session-start: PR 無しは完全沈黙
    assert_eq!(r.stdout, ""); // ok   session-start: PR 無しは stdout 空
}

// --- hygiene advisories(事故①古い base / 事故④残骸) ---
// [gone] は本物の fetch --prune がなくても、branch.*.merge が指す remote-tracking
// ref が存在しないだけで git 自身が判定してくれる。

fn add_gone_branch(fx: &Fx, name: &str) {
    fx.git(&["branch", name]);
    fx.git(&["config", &format!("branch.{name}.remote"), "origin"]);
    fx.git(&[
        "config",
        &format!("branch.{name}.merge"),
        &format!("refs/heads/{name}-nonexistent"),
    ]);
}

#[test]
fn hygiene_gone_branches_without_pr() {
    let fx = Fx::new();
    add_gone_branch(&fx, "hygiene-gone-branch");
    let r = no_pr(&fx, "hyg-gone-sid");
    assert_eq!(r.code, 0); // ok   hygiene: [gone] ありでも session-start exit 0
    assert!(r.ctx().contains("残骸ブランチ: [gone] が 1 本")); // ok   hygiene: PR 無しでも [gone] 本数が出る
}

#[test]
fn hygiene_gone_branches_with_pr() {
    let fx = Fx::new();
    add_gone_branch(&fx, "hygiene-gone-branch");
    add_gone_branch(&fx, "hygiene-gone-branch2");
    let r = with_pr(&fx, "hyg-withpr-sid");
    assert!(r.ctx().contains("残骸ブランチ: [gone] が 2 本")); // ok   hygiene: PR ありでも [gone] 本数が付く
}

#[test]
fn hygiene_stale_worktree_clean_and_dirty() {
    let fx = Fx::new();
    add_gone_branch(&fx, "hygiene-gone-branch");
    let wt = fx.dir().join("hygiene-wt");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        &wt.display().to_string(),
        "hygiene-gone-branch",
    ]);
    let r = no_pr(&fx, "hyg-wt-sid");
    // ok   hygiene: 残骸 worktree 数が出る
    assert!(r
        .ctx()
        .contains("残骸 worktree: [gone] かつ未変更の worktree が 1 個"));

    // dirty にすると数えない(false positive を出さない側に倒す)。
    std::fs::write(wt.join("dirty.txt"), "").unwrap();
    let r = no_pr(&fx, "hyg-wt-dirty-sid");
    assert!(!r.ctx().contains("残骸 worktree")); // ok   hygiene: dirty な worktree は残骸として数えない
}

#[test]
fn hygiene_stale_base() {
    // 「ahead」と「behind」は別の事象。origin/main を HEAD と共通祖先を持つ別の
    // 1 commit へ付け替えて、本物の divergence(ahead 2 / behind 1)を作る。
    let fx = Fx::new();
    let base = fx.rev("HEAD~2");
    let other = fx.git(&[
        "-c",
        "user.email=t@example.com",
        "-c",
        "user.name=t",
        "commit-tree",
        &format!("{base}^{{tree}}"),
        "-p",
        &base,
        "-m",
        "other-team-commit",
    ]);
    fx.set_origin_head("main", Some(&other));
    let r = no_pr(&fx, "hyg-base-sid");
    // ok   hygiene: stale base 行が出る(behind 1)
    assert!(r
        .ctx()
        .contains("base 追従: origin/main から 1 コミット遅れています"));
}
