//! `git-prune-branches` の結合テスト。`scripts/git-prune-branches --selftest`
//! (selftest + selftest_auto)の全ケースを移植し、bash 版にあった暗黙の
//! 挙動(終了コード・フラグ処理・log.tsv の列)も追加で固定する。
//!
//! bash ケース → Rust テスト:
//!   selftest   --dry-run                 → gone_dry_run_lists_and_deletes_nothing
//!   selftest   確認を断る(空入力)         → gone_declined_aborts_and_keeps_branch
//!   selftest   y で確認                   → gone_accepted_deletes_gone_keeps_in_use_and_live
//!   auto       C2 の branch 削除 + log    → auto_deletes_c2_branch_via_git_dir_and_logs
//!   auto       --dry-run                  → auto_dry_run_deletes_and_logs_nothing
//!   auto       TOCTOU                     → auto_toctou_skips_when_row_vanishes
//! 追加(bash 版の暗黙挙動):
//!   usage / 未知フラグ / audit 失敗の終了コード / 空 listing / log 列書式 /
//!   XDG_STATE_HOME 既定の log 先 / 確認 `yes` 受理 / git 拒否の skip

mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};

const BIN: &str = env!("CARGO_BIN_EXE_git-prune-branches");

struct Gone {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    wt: PathBuf,
}

/// bash selftest() のフィクスチャ: remote 付き repo、削除済み upstream を
/// 持つ gone-a、worktree で checkout 中の gone-in-use、無関係な untouched。
fn gone_fixture() -> Gone {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let repo = t.join("repo");
    new_repo(&repo, true);
    let remote = t.join("remote.git");
    git(t, &["init", "-q", "--bare", path_str(&remote)]);
    git(&repo, &["remote", "add", "origin", path_str(&remote)]);
    git(&repo, &["push", "-q", "origin", "main"]);

    git(&repo, &["branch", "gone-a"]);
    git(&repo, &["push", "-q", "-u", "origin", "gone-a"]);
    git(&repo, &["push", "-q", "origin", "--delete", "gone-a"]);

    let wt = t.join("wt-inuse");
    git(
        &repo,
        &["worktree", "add", "-q", path_str(&wt), "-b", "gone-in-use"],
    );
    git(&repo, &["push", "-q", "-u", "origin", "gone-in-use"]);
    git(&repo, &["push", "-q", "origin", "--delete", "gone-in-use"]);

    git(&repo, &["branch", "untouched"]);
    Gone {
        _tmp: tmp,
        repo,
        wt,
    }
}

#[test]
fn gone_dry_run_lists_and_deletes_nothing() {
    let f = gone_fixture();
    let r = run(BIN, &["--dry-run"], &f.repo, &[], None);
    assert!(r.all().contains("gone-a"), "{}", r.all());
    assert!(r.all().contains("--dry-run"), "{}", r.all());
    assert!(branch_exists(&f.repo, "gone-a"));
    assert_eq!(r.code, 0);
    // worktree でブロックされた方は別枠で報告される
    assert!(r.stdout.contains("remove the worktree first"));
    assert!(r.stdout.contains("  gone-in-use\n"));
    assert!(r
        .stdout
        .contains("will delete these [gone] branches:\n  gone-a\n"));
    let _ = &f.wt;
}

#[test]
fn gone_declined_aborts_and_keeps_branch() {
    let f = gone_fixture();
    let r = run(BIN, &[], &f.repo, &[], Some("\n"));
    assert!(r.all().contains("aborted"), "{}", r.all());
    assert!(r.stdout.contains("prune-branches: aborted.\n"));
    assert!(branch_exists(&f.repo, "gone-a"));
    assert_eq!(r.code, 0);
}

#[test]
fn gone_accepted_deletes_gone_keeps_in_use_and_live() {
    let f = gone_fixture();
    let r = run(BIN, &[], &f.repo, &[], Some("y\n"));
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(!branch_exists(&f.repo, "gone-a"), "y で確認したのに残った");
    assert!(
        branch_exists(&f.repo, "gone-in-use"),
        "worktree checkout 中まで削除された"
    );
    assert!(r.all().contains("remove the worktree first"));
    assert!(branch_exists(&f.repo, "untouched"));
    assert!(branch_exists(&f.repo, "main"));
}

#[test]
fn gone_accepts_yes_and_trims_whitespace_and_partial_last_line() {
    // bash: `read -r` は前後の空白を落とし、改行なし EOF でも変数に入る。
    for ans in ["yes\n", "YES\n", "Y\n", "  y  \n", "y"] {
        let f = gone_fixture();
        let r = run(BIN, &[], &f.repo, &[], Some(ans));
        assert!(!branch_exists(&f.repo, "gone-a"), "{ans:?}: {}", r.all());
    }
    for ans in ["n\n", "yy\n", "", "no\n"] {
        let f = gone_fixture();
        let r = run(BIN, &[], &f.repo, &[], Some(ans));
        assert!(branch_exists(&f.repo, "gone-a"), "{ans:?}");
        assert!(r.stdout.contains("prune-branches: aborted."), "{ans:?}");
    }
}

#[test]
fn gone_none_and_only_in_use_messages() {
    // [gone] が無い
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    new_repo(&repo, true);
    let remote = tmp.path().join("remote.git");
    git(tmp.path(), &["init", "-q", "--bare", path_str(&remote)]);
    git(&repo, &["remote", "add", "origin", path_str(&remote)]);
    git(&repo, &["push", "-q", "origin", "main"]);
    let r = run(BIN, &[], &repo, &[], None);
    assert_eq!(r.stdout, "prune-branches: no [gone] branches.\n");
    assert_eq!(r.code, 0);

    // 全部 worktree で使用中 → nothing left to delete
    let wt = tmp.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", path_str(&wt), "-b", "only-in-use"],
    );
    git(&repo, &["push", "-q", "-u", "origin", "only-in-use"]);
    git(&repo, &["push", "-q", "origin", "--delete", "only-in-use"]);
    let r = run(BIN, &[], &repo, &[], None);
    assert!(r.stdout.contains("  only-in-use\n"));
    assert!(r
        .stdout
        .ends_with("prune-branches: nothing left to delete.\n"));
    assert_eq!(r.code, 0);
}

#[test]
fn usage_help_and_unknown_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let r = run(BIN, &["--help"], tmp.path(), &[], None);
    assert_eq!(r.code, 0);
    assert!(r
        .stdout
        .starts_with("usage: git prune-branches [--dry-run|--auto]"));
    let r = run(BIN, &["-h"], tmp.path(), &[], None);
    assert_eq!(r.code, 0);
    let r = run(BIN, &["--bogus"], tmp.path(), &[], None);
    assert_eq!(r.code, 2);
    assert!(r.stdout.is_empty());
    assert!(r.stderr.starts_with("usage: git prune-branches"));
    // フラグは前から順に処理される: --help の後ろの未知フラグは見られない
    let r = run(BIN, &["--help", "--bogus"], tmp.path(), &[], None);
    assert_eq!(r.code, 0);
    let r = run(BIN, &["--dry-run", "--bogus"], tmp.path(), &[], None);
    assert_eq!(r.code, 2);
}

// ---- --auto -------------------------------------------------------------

struct Auto {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    common: String,
    stub: PathBuf,
    evidence: PathBuf,
    log_dir: PathBuf,
}

impl Auto {
    fn new() -> Auto {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let repo = t.join("repo");
        new_repo(&repo, false);
        let common = common_dir(&repo);
        let stub = t.join("audit-stub");
        write_exec(
            &stub,
            "#!/bin/sh\nif [ \"$1\" = --evidence ]; then\n  cat \"$GIT_PRUNE_BRANCHES_TEST_EVIDENCE_FILE\"\nfi\n",
        );
        Auto {
            repo,
            common,
            stub,
            evidence: t.join("evidence.tsv"),
            log_dir: t.join("state/git-auto-prune"),
            _tmp: tmp,
        }
    }

    fn row(&self, branch: &str, ev: &str) -> String {
        let sha = git_out(&self.repo, &["rev-parse", branch]);
        format!(
            "branch\t{}\t{}\t\t{branch}\t{sha}\t{ev}\n",
            self.common,
            self.repo.display()
        )
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Run {
        let mut env: Vec<(&str, &str)> = vec![
            ("GIT_PRUNE_BRANCHES_AUDIT_BIN", path_str(&self.stub)),
            ("GIT_PRUNE_BRANCHES_LOG_DIR", path_str(&self.log_dir)),
            (
                "GIT_PRUNE_BRANCHES_TEST_EVIDENCE_FILE",
                path_str(&self.evidence),
            ),
        ];
        env.extend_from_slice(extra);
        // repo の外から実行 — `--git-dir` で消せることの検証(bash 版と同じ)
        run(BIN, args, self.repo.parent().unwrap(), &env, None)
    }

    fn log(&self) -> String {
        fs::read_to_string(self.log_dir.join("log.tsv")).unwrap_or_default()
    }
}

#[test]
fn auto_deletes_c2_branch_via_git_dir_and_logs() {
    let a = Auto::new();
    git(&a.repo, &["branch", "worktree/loose-c2"]);
    fs::write(&a.evidence, a.row("worktree/loose-c2", "C2")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(
        !branch_exists(&a.repo, "worktree/loose-c2"),
        "C2 の branch が削除されなかった"
    );
    assert!(
        r.all().contains("removed(C2): worktree/loose-c2"),
        "{}",
        r.all()
    );
    assert!(a.log().contains("\tC2"), "log.tsv に C2 の記録が無い");
    assert!(r.stdout.contains("auto: removed=1 skipped=0\n"));
    assert!(r.stdout.contains("evidence branch: repo="));
    assert!(r.stdout.contains("total: 1 evidence-backed branch(es)\n"));
}

#[test]
fn auto_log_line_format() {
    // 列: timestamp, kind=branch, common, repo, path(空), branch, sha, evidence
    let a = Auto::new();
    git(&a.repo, &["branch", "worktree/fmt"]);
    let sha = git_out(&a.repo, &["rev-parse", "worktree/fmt"]);
    fs::write(&a.evidence, a.row("worktree/fmt", "C3:#7")).unwrap();
    a.run(&["--auto"], &[]);
    let log = a.log();
    let cols: Vec<&str> = log.trim_end_matches('\n').split('\t').collect();
    assert_eq!(cols.len(), 8, "{log:?}");
    assert_eq!(cols[1], "branch");
    assert_eq!(cols[2], a.common);
    assert_eq!(cols[3], a.repo.to_str().unwrap());
    assert_eq!(cols[4], "");
    assert_eq!(cols[5], "worktree/fmt");
    assert_eq!(cols[6], sha);
    assert_eq!(cols[7], "C3:#7");
    // %Y-%m-%dT%H:%M:%S%z
    let ts = cols[0];
    assert_eq!(ts.len(), 24, "{ts}");
    assert_eq!(&ts[10..11], "T");
    assert!(ts[19..20] == *"+" || ts[19..20] == *"-");
}

#[test]
fn auto_dry_run_deletes_and_logs_nothing() {
    let a = Auto::new();
    // 先に 1 件削除して log を作り、dry-run で行数が変わらないことを見る
    git(&a.repo, &["branch", "worktree/first"]);
    fs::write(&a.evidence, a.row("worktree/first", "C2")).unwrap();
    a.run(&["--auto"], &[]);
    let before = a.log().lines().count();
    assert_eq!(before, 1);

    git(&a.repo, &["branch", "worktree/loose-dry"]);
    fs::write(&a.evidence, a.row("worktree/loose-dry", "C2")).unwrap();
    let r = a.run(&["--auto", "--dry-run"], &[]);
    assert!(
        branch_exists(&a.repo, "worktree/loose-dry"),
        "dry-run で削除した"
    );
    assert!(r.all().contains("--dry-run"));
    assert_eq!(
        a.log().lines().count(),
        before,
        "dry-run なのに log に書いた"
    );
    assert!(!r.stdout.contains("auto: removed="));
    // --dry-run --auto の順でも同じ
    let r2 = a.run(&["--dry-run", "--auto"], &[]);
    assert!(r2.stdout.contains("(--dry-run: 削除はしていません)\n"));
    assert!(branch_exists(&a.repo, "worktree/loose-dry"));
}

#[test]
fn auto_toctou_skips_when_row_vanishes() {
    let a = Auto::new();
    git(&a.repo, &["branch", "worktree/loose-toctou"]);
    fs::write(&a.evidence, a.row("worktree/loose-toctou", "C2")).unwrap();
    let hook = format!(": >'{}'", a.evidence.display());
    let r = a.run(
        &["--auto"],
        &[("GIT_PRUNE_BRANCHES_TEST_PRE_ACT_HOOK", &hook)],
    );
    assert!(
        branch_exists(&a.repo, "worktree/loose-toctou"),
        "状態変化を検知せず削除した"
    );
    assert!(
        r.all().contains("skip(状態が変化したため見送り)"),
        "{}",
        r.all()
    );
    // skip は stderr
    assert!(r
        .stderr
        .contains("skip(状態が変化したため見送り): worktree/loose-toctou"));
    assert!(r.stdout.contains("auto: removed=0 skipped=1\n"));
    assert_eq!(a.log(), "");
}

#[test]
fn auto_git_refusal_is_skipped_not_fatal() {
    // 現在 checkout 中の branch は git が拒否する → skip(git が拒否)
    let a = Auto::new();
    let cur = git_out(&a.repo, &["symbolic-ref", "--short", "HEAD"]);
    fs::write(&a.evidence, a.row(&cur, "C2")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 0);
    assert!(
        r.stderr.contains(&format!("skip(git が拒否): {cur}: ")),
        "{}",
        r.stderr
    );
    assert!(r.stdout.contains("auto: removed=0 skipped=1\n"));
    assert!(branch_exists(&a.repo, &cur));
    assert_eq!(a.log(), "");
}

#[test]
fn auto_empty_listing_and_only_branch_kind_rows() {
    let a = Auto::new();
    fs::write(&a.evidence, "").unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.stdout, "auto: 削除対象の branch はありません。\n");
    assert_eq!(r.code, 0);
    // worktree 行だけなら対象なし
    fs::write(&a.evidence, "worktree\tc\tr\tp\tb\tsha\tC2\n").unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.stdout, "auto: 削除対象の branch はありません。\n");
}

#[test]
fn auto_audit_failure_propagates_exit_code() {
    let a = Auto::new();
    write_exec(&a.stub, "#!/bin/sh\necho boom >&2\nexit 7\n");
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 7);
    assert!(r.stderr.contains("boom"));
    // 見つからない audit は 127
    let r = run(
        BIN,
        &["--auto"],
        a.repo.as_path(),
        &[("GIT_PRUNE_BRANCHES_AUDIT_BIN", "/nonexistent/audit")],
        None,
    );
    assert_eq!(r.code, 127);
}

#[test]
fn auto_default_log_dir_follows_xdg_state_home() {
    let a = Auto::new();
    git(&a.repo, &["branch", "worktree/xdg"]);
    fs::write(&a.evidence, a.row("worktree/xdg", "C2")).unwrap();
    let xdg = a._tmp.path().join("xdg");
    let r = run(
        BIN,
        &["--auto"],
        a.repo.as_path(),
        &[
            ("GIT_PRUNE_BRANCHES_AUDIT_BIN", path_str(&a.stub)),
            (
                "GIT_PRUNE_BRANCHES_TEST_EVIDENCE_FILE",
                path_str(&a.evidence),
            ),
            ("XDG_STATE_HOME", path_str(&xdg)),
        ],
        None,
    );
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(Path::new(&xdg.join("git-auto-prune/log.tsv")).exists());
}
