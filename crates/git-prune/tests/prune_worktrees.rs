//! `git-prune-worktrees` の結合テスト。`scripts/git-prune-worktrees --selftest`
//! (selftest + selftest_auto)の全ケースを移植し、bash 版の暗黙の挙動
//! (終了コード・フラグ・tty 確認・log 列)も追加で固定する。
//!
//! bash ケース → Rust テスト:
//!   selftest   --dry-run                              → interactive_dry_run_lists_both_and_removes_nothing
//!   selftest   非対話 + --yes 無しで拒否               → non_interactive_without_yes_is_refused
//!   selftest   TOCTOU(wt2 が再スキャンで消える)        → interactive_toctou_skips_vanished_row
//!   selftest   prunable(metadata のみ削除・branch 残)  → prunable_prunes_registration_keeps_branch
//!   selftest   detached prunable(#547 列ずれ)          → prunable_detached_dry_run_columns_do_not_shift
//!   selftest   submodule A(clean → --force 再試行)     → submodule_clean_retries_with_force
//!   selftest   submodule B(dirty → skip)               → submodule_dirty_is_skipped
//!   selftest   submodule C(dirty + --force)            → force_removes_dirty_submodule_worktree
//!   auto       C2 の worktree 削除 + log               → auto_c2_removes_worktree_and_logs
//!   auto       C1(prunable 登録のみ)+ log              → auto_c1_prunes_registration_and_logs
//!   auto       --dry-run                               → auto_dry_run_removes_and_logs_nothing
//!   auto       detached HEAD(C3、branch 列が空)         → auto_detached_c3_empty_branch_column
//!   auto       TOCTOU                                  → auto_toctou_skips_when_row_vanishes
//! 追加:
//!   usage / 未知フラグ / audit 失敗の終了コード / tty 確認 (y / n / EOF) /
//!   --force の通常 orphaned / git 拒否の skip(auto・interactive) /
//!   log 列書式 / 空 listing / XDG_STATE_HOME 既定の log 先

mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_git-prune-worktrees");

struct Fx {
    tmp: tempfile::TempDir,
    repo: PathBuf,
    common: String,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    new_repo(&repo, false);
    let common = common_dir(&repo);
    Fx { tmp, repo, common }
}

impl Fx {
    fn t(&self) -> &Path {
        self.tmp.path()
    }
    fn add_wt(&self, name: &str, branch: &str) -> PathBuf {
        let wt = self.t().join(name);
        git(
            &self.repo,
            &["worktree", "add", "-q", path_str(&wt), "-b", branch],
        );
        wt
    }
    fn orphaned_row(&self, branch: &str, wt: &Path) -> String {
        format!(
            "{}\t{}\t{}\t{branch}\torphaned\tno-upstream, unique=0\n",
            self.common,
            self.repo.display(),
            wt.display()
        )
    }
    fn stub_porcelain(&self, rows: &str) -> PathBuf {
        let stub = self.t().join("audit-stub");
        write_exec(
            &stub,
            &format!("#!/bin/sh\ncat <<'EOF_ROWS'\n{rows}EOF_ROWS\n"),
        );
        stub
    }
    fn run(&self, stub: &Path, args: &[&str], extra: &[(&str, &str)]) -> Run {
        let mut env = vec![("GIT_PRUNE_WORKTREES_AUDIT_BIN", path_str(stub))];
        env.extend_from_slice(extra);
        run(BIN, args, self.t(), &env, None)
    }
}

// ---- interactive -------------------------------------------------------

/// wt1/wt2 を orphaned として返し、`drop-b` マーカーがあれば wt2 を返さない
/// スタブ(bash selftest の audit-stub)。
fn two_orphans() -> (Fx, PathBuf, PathBuf, PathBuf) {
    let f = fx();
    let wt1 = f.add_wt("orphan-a", "worktree/orphan-a");
    let wt2 = f.add_wt("orphan-b", "worktree/orphan-b");
    let stub = f.t().join("audit-stub");
    let drop_b = f.t().join("drop-b");
    write_exec(
        &stub,
        &format!(
            "#!/bin/sh\nprintf '%s\\t%s\\t%s\\tworktree/orphan-a\\torphaned\\tno-upstream, unique=0\\n' '{c}' '{r}' '{w1}'\n[ -e '{d}' ] || printf '%s\\t%s\\t%s\\tworktree/orphan-b\\torphaned\\tno-upstream, unique=0\\n' '{c}' '{r}' '{w2}'\n",
            c = f.common,
            r = f.repo.display(),
            w1 = wt1.display(),
            w2 = wt2.display(),
            d = drop_b.display()
        ),
    );
    (f, wt1, wt2, stub)
}

#[test]
fn interactive_dry_run_lists_both_and_removes_nothing() {
    let (f, wt1, wt2, stub) = two_orphans();
    let r = f.run(&stub, &["--dry-run"], &[]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.stdout.contains(&wt1.display().to_string()));
    assert!(r.stdout.contains(&wt2.display().to_string()));
    assert!(r.stdout.contains("--dry-run"));
    assert!(wt1.is_dir(), "--dry-run が削除した");
    assert!(r.stdout.contains("total: 2 stale worktree(s)\n"));
    assert!(r.stdout.contains(&format!(
        "orphaned worktree: repo={} path={} branch=worktree/orphan-a reason=no-upstream, unique=0\n",
        f.repo.display(),
        wt1.display()
    )));
}

#[test]
fn non_interactive_without_yes_is_refused() {
    let (f, wt1, _wt2, stub) = two_orphans();
    let r = f.run(&stub, &[], &[]); // stdin=/dev/null (非 tty)
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("非対話実行では --yes が必要です。"));
    assert!(wt1.is_dir(), "確認前に削除した");
}

#[test]
fn interactive_toctou_skips_vanished_row() {
    let (f, wt1, wt2, stub) = two_orphans();
    let hook = format!("touch '{}'", f.t().join("drop-b").display());
    let r = f.run(
        &stub,
        &["--yes"],
        &[("GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK", &hook)],
    );
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(!wt1.is_dir(), "wt1 が削除されなかった");
    assert!(wt2.is_dir(), "再検証で除外されるはずの wt2 が削除された");
    assert!(r.all().contains("skip(状態が変化したため見送り)"));
    assert!(r.stderr.contains(&format!(
        "skip(状態が変化したため見送り): {}",
        wt2.display()
    )));
    assert!(branch_exists(&f.repo, "worktree/orphan-a"));
    assert!(branch_exists(&f.repo, "worktree/orphan-b"));
    assert!(r
        .stdout
        .contains(&format!("removed: {} (worktree/orphan-a)\n", wt1.display())));
    assert!(r.stdout.contains("removed=1 skipped=1\n"));
    assert!(r
        .stdout
        .contains("[gone] ブランチが残っていれば git prune-branches で確認・削除してください。\n"));
}

#[test]
fn interactive_no_removal_prints_no_hint() {
    let (f, wt1, _w2, stub) = two_orphans();
    // 常に空の再スキャンにする: 全件見送り → removed=0 でヒント無し
    let hook = format!("touch '{}'", f.t().join("drop-b").display());
    // wt1 も消すため stub を差し替え(2 回目以降は何も返さない)
    let marker = f.t().join("seen");
    write_exec(
        &stub,
        &format!(
            "#!/bin/sh\nif [ -e '{m}' ]; then exit 0; fi\ncat <<'X'\n{row}X\n",
            m = marker.display(),
            row = f.orphaned_row("worktree/orphan-a", &wt1).trim_end()
        ),
    );
    let hook2 = format!("touch '{}'; {hook}", marker.display());
    let r = f.run(
        &stub,
        &["--yes"],
        &[("GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK", &hook2)],
    );
    assert!(r.stdout.ends_with("removed=0 skipped=1\n"), "{}", r.all());
    assert!(wt1.is_dir());
}

#[test]
fn prunable_prunes_registration_keeps_branch() {
    let f = fx();
    let wt3 = f.add_wt("prunable-a", "worktree/prunable-a");
    fs::remove_dir_all(&wt3).unwrap();
    let stub = f.stub_porcelain(&format!(
        "{}\t{}\t{}\tworktree/prunable-a\tprunable\tgitdir file points to non-existent location\n",
        f.common,
        f.repo.display(),
        wt3.display()
    ));
    let r = f.run(&stub, &["--yes"], &[]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(
        r.all().contains("prunable worktree"),
        "一覧に表示されなかった"
    );
    assert!(
        !worktree_registered(&f.common, &wt3),
        "登録メタデータが残った"
    );
    assert!(
        branch_exists(&f.repo, "worktree/prunable-a"),
        "branch まで削除された"
    );
    // prunable は removed に数えない(bash 版)
    assert!(r.stdout.contains("removed=0 skipped=0\n"));
}

#[test]
fn prunable_detached_dry_run_columns_do_not_shift() {
    let f = fx();
    let wt4 = f.t().join("prunable-detached");
    git(
        &f.repo,
        &["worktree", "add", "-q", "--detach", path_str(&wt4)],
    );
    fs::remove_dir_all(&wt4).unwrap();
    let stub = f.stub_porcelain(&format!(
        "{}\t{}\t{}\t\tprunable\tgitdir file points to non-existent location\n",
        f.common,
        f.repo.display(),
        wt4.display()
    ));
    let r = f.run(&stub, &["--dry-run"], &[]);
    assert!(
        r.all()
            .contains("branch=(detached) reason=gitdir file points to non-existent location"),
        "列がずれた: {}",
        r.all()
    );
    assert!(r.stdout.contains("total: 1 stale worktree(s)\n"));
    assert!(
        worktree_registered(&f.common, &wt4),
        "dry-run が登録を消した"
    );
}

#[test]
fn prunable_dedups_common_dirs_and_empty_listing() {
    let f = fx();
    let wa = f.add_wt("pa", "worktree/pa");
    let wb = f.add_wt("pb", "worktree/pb");
    fs::remove_dir_all(&wa).unwrap();
    fs::remove_dir_all(&wb).unwrap();
    let stub = f.stub_porcelain(&format!(
        "{c}\t{r}\t{a}\tworktree/pa\tprunable\tgone\n{c}\t{r}\t{b}\tworktree/pb\tprunable\tgone\n",
        c = f.common,
        r = f.repo.display(),
        a = wa.display(),
        b = wb.display()
    ));
    let r = f.run(&stub, &["--yes"], &[]);
    // `git worktree prune --verbose` は 1 回だけ(2 件ぶんがまとめて出る)
    assert_eq!(
        r.all().matches("Removing worktrees/").count(),
        2,
        "{}",
        r.all()
    );
    assert!(!worktree_registered(&f.common, &wa));
    assert!(!worktree_registered(&f.common, &wb));

    // 空 listing
    let stub = f.stub_porcelain("");
    let r = f.run(&stub, &[], &[]);
    assert_eq!(r.stdout, "削除対象の worktree はありません。\n");
    assert_eq!(r.code, 0);
}

struct Sub {
    f: Fx,
    repo_sub: PathBuf,
    wt_sub: PathBuf,
}

/// bash selftest の submodule フィクスチャ。gitlink を先頭に、2000 個の
/// bulk ファイルで `ls-files -s` をパイプバッファ超にする(has_submodule が
/// 出力を読み切る要件の回帰)。
fn sub_fixture(branch: &str, bulk: bool) -> Sub {
    let f = fx();
    let sub = f.t().join("sub");
    new_repo(&sub, false);
    let repo_sub = f.t().join("repo-sub");
    new_repo(&repo_sub, false);
    git(
        &repo_sub,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            path_str(&sub),
            "0submodule",
        ],
    );
    if bulk {
        let b = repo_sub.join("bulk");
        fs::create_dir_all(&b).unwrap();
        for i in 1..=2000 {
            fs::write(b.join(format!("f{i:04}")), "").unwrap();
        }
    }
    git(&repo_sub, &["add", "-A"]);
    git(&repo_sub, &["commit", "-qm", "add submodule + bulk files"]);
    let wt_sub = f.t().join("orphan-sub");
    git(
        &repo_sub,
        &["worktree", "add", "-q", path_str(&wt_sub), "-b", branch],
    );
    git(
        &wt_sub,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "-q",
            "--init",
        ],
    );
    Sub {
        f,
        repo_sub,
        wt_sub,
    }
}

impl Sub {
    fn stub(&self, branch: &str) -> PathBuf {
        self.f.stub_porcelain(&format!(
            "common\t{}\t{}\t{branch}\torphaned\tno-upstream, unique=0\n",
            self.repo_sub.display(),
            self.wt_sub.display()
        ))
    }
}

#[test]
fn submodule_clean_retries_with_force() {
    let s = sub_fixture("worktree/orphan-sub", true);
    let stub = s.stub("worktree/orphan-sub");
    let r = s.f.run(&stub, &["--yes"], &[]);
    assert!(
        !s.wt_sub.is_dir(),
        "clean な submodule worktree が削除されなかった: {}",
        r.all()
    );
    assert!(
        r.all().contains("removed(submodule のため --force)"),
        "{}",
        r.all()
    );
    assert!(r.stdout.contains("removed=1 skipped=0\n"));
}

#[test]
fn submodule_dirty_is_skipped() {
    let s = sub_fixture("worktree/orphan-sub-dirty", true);
    fs::write(s.wt_sub.join("0submodule/untracked.txt"), "dirty\n").unwrap();
    let stub = s.stub("worktree/orphan-sub-dirty");
    let r = s.f.run(&stub, &["--yes"], &[]);
    assert!(
        s.wt_sub.is_dir(),
        "dirty な submodule worktree が誤って削除された"
    );
    assert!(r.all().contains("skip(git が拒否)"), "{}", r.all());
    assert!(r.stdout.contains("removed=0 skipped=1\n"));
}

#[test]
fn force_removes_dirty_submodule_worktree() {
    let s = sub_fixture("worktree/orphan-sub-dirty", false);
    fs::write(s.wt_sub.join("0submodule/untracked.txt"), "dirty\n").unwrap();
    let stub = s.stub("worktree/orphan-sub-dirty");
    let r = s.f.run(&stub, &["--yes", "--force"], &[]);
    assert!(
        !s.wt_sub.is_dir(),
        "dirty な submodule worktree が削除されなかった: {}",
        r.all()
    );
    assert!(r.all().contains("removed(--force)"), "{}", r.all());
}

#[test]
fn force_overrides_dirty_plain_worktree_but_default_skips_it() {
    let f = fx();
    let wt = f.add_wt("dirty", "worktree/dirty");
    fs::write(wt.join("untracked.txt"), "x").unwrap();
    // untracked は plain remove でも拒否されない → tracked な変更を作る
    fs::write(wt.join("tracked.txt"), "x").unwrap();
    git(&wt, &["add", "tracked.txt"]);
    let stub = f.stub_porcelain(&f.orphaned_row("worktree/dirty", &wt));
    let r = f.run(&stub, &["--yes"], &[]);
    assert!(wt.is_dir());
    assert!(
        r.stderr
            .contains(&format!("skip(git が拒否): {}: ", wt.display())),
        "{}",
        r.stderr
    );
    assert!(r.stdout.contains("removed=0 skipped=1\n"));
    assert!(!r.stdout.contains("git prune-branches"));
    let r = f.run(&stub, &["--yes", "--force"], &[]);
    assert!(!wt.is_dir(), "{}", r.all());
    assert!(r.stdout.contains(&format!(
        "removed(--force): {} (worktree/dirty)\n",
        wt.display()
    )));
}

// ---- tty 確認(bash 版 selftest は stdin が非 tty のため未検証) --------

fn run_with_pty(f: &Fx, stub: &Path, answer: &str) -> Option<(i32, String, String)> {
    if Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("python3 が無いので tty テストを skip");
        return None;
    }
    let script = r#"
import os, subprocess, sys
bin_, answer = sys.argv[1], sys.argv[2]
m, s = os.openpty()
p = subprocess.Popen([bin_], stdin=s, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
os.close(s)
if answer != "<EOF>":
    os.write(m, answer.encode())
else:
    os.write(m, b"\x04")  # ^D (改行なしの EOF)
out, err = p.communicate()
print(p.returncode)
sys.stdout.flush()
sys.stdout.buffer.write(b"\0" + out + b"\0" + err)
"#;
    let o = Command::new("python3")
        .args(["-c", script, BIN, answer])
        .current_dir(f.t())
        .env("GIT_PRUNE_WORKTREES_AUDIT_BIN", stub)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    let mut it = s.splitn(3, '\0');
    let code: i32 = it.next()?.trim().parse().ok()?;
    Some((code, it.next()?.to_string(), it.next()?.to_string()))
}

#[test]
fn tty_prompt_y_deletes_and_other_answers_exit_2() {
    let (f, wt1, wt2, stub) = two_orphans();
    if let Some((code, out, err)) = run_with_pty(&f, &stub, "n\n") {
        assert_eq!(code, 2, "{out}{err}");
        assert!(err.contains("上記を削除しますか? [y/N] "), "{err}");
        assert!(wt1.is_dir() && wt2.is_dir());
    }
    if let Some((code, out, err)) = run_with_pty(&f, &stub, "\n") {
        assert_eq!(code, 2, "{out}{err}");
        assert!(wt1.is_dir());
    }
    if let Some((code, out, err)) = run_with_pty(&f, &stub, "<EOF>") {
        assert_eq!(code, 1, "{out}{err}");
        assert!(wt1.is_dir());
    }
    if let Some((code, out, err)) = run_with_pty(&f, &stub, "y\n") {
        assert_eq!(code, 0, "{out}{err}");
        assert!(out.contains("removed=2 skipped=0"), "{out}");
        assert!(!wt1.is_dir() && !wt2.is_dir());
    }
}

// ---- usage / audit 失敗 --------------------------------------------------

#[test]
fn usage_help_and_unknown_flag() {
    let f = fx();
    let r = run(BIN, &["--help"], f.t(), &[], None);
    assert_eq!(r.code, 0);
    assert!(r
        .stdout
        .starts_with("usage: git prune-worktrees [--dry-run|--yes|--force|--auto]"));
    let r = run(BIN, &["--bogus"], f.t(), &[], None);
    assert_eq!(r.code, 2);
    assert!(r.stdout.is_empty());
    assert!(r.stderr.starts_with("usage: git prune-worktrees"));
    let r = run(BIN, &["--help", "--bogus"], f.t(), &[], None);
    assert_eq!(r.code, 0);
}

#[test]
fn audit_failure_propagates_exit_code_and_missing_is_127() {
    let f = fx();
    let stub = f.t().join("audit-stub");
    write_exec(&stub, "#!/bin/sh\nexit 5\n");
    assert_eq!(f.run(&stub, &["--dry-run"], &[]).code, 5);
    assert_eq!(f.run(&stub, &["--auto"], &[]).code, 5);
    let r = run(
        BIN,
        &["--dry-run"],
        f.t(),
        &[("GIT_PRUNE_WORKTREES_AUDIT_BIN", "/nonexistent/audit")],
        None,
    );
    assert_eq!(r.code, 127);
}

// ---- --auto ----------------------------------------------------------------

struct Auto {
    f: Fx,
    stub: PathBuf,
    evidence: PathBuf,
    log_dir: PathBuf,
}

impl Auto {
    fn new() -> Auto {
        let f = fx();
        let stub = f.t().join("audit-stub");
        write_exec(
            &stub,
            "#!/bin/sh\nif [ \"$1\" = --evidence ]; then\n  cat \"$GIT_PRUNE_WORKTREES_TEST_EVIDENCE_FILE\"\nfi\n",
        );
        Auto {
            evidence: f.t().join("evidence.tsv"),
            log_dir: f.t().join("state/git-auto-prune"),
            stub,
            f,
        }
    }
    fn row(&self, wt: &Path, branch: &str, sha: &str, ev: &str) -> String {
        format!(
            "worktree\t{}\t{}\t{}\t{branch}\t{sha}\t{ev}\n",
            self.f.common,
            self.f.repo.display(),
            wt.display()
        )
    }
    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Run {
        let mut env: Vec<(&str, &str)> = vec![
            ("GIT_PRUNE_WORKTREES_AUDIT_BIN", path_str(&self.stub)),
            ("GIT_PRUNE_WORKTREES_LOG_DIR", path_str(&self.log_dir)),
            (
                "GIT_PRUNE_WORKTREES_TEST_EVIDENCE_FILE",
                path_str(&self.evidence),
            ),
        ];
        env.extend_from_slice(extra);
        run(BIN, args, self.f.t(), &env, None)
    }
    fn log(&self) -> String {
        fs::read_to_string(self.log_dir.join("log.tsv")).unwrap_or_default()
    }
}

fn head(wt: &Path) -> String {
    git_out(wt, &["rev-parse", "HEAD"])
}

#[test]
fn auto_c2_removes_worktree_and_logs() {
    let a = Auto::new();
    let wt = a.f.add_wt("wt-c2", "worktree/c2");
    let sha = head(&wt);
    fs::write(&a.evidence, a.row(&wt, "worktree/c2", &sha, "C2")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(!wt.is_dir(), "C2 の worktree が削除されなかった");
    assert!(
        r.all().contains(&format!("removed(C2): {}", wt.display())),
        "{}",
        r.all()
    );
    assert!(a.log().contains("\tC2"), "log.tsv に C2 の記録が無い");
    assert!(r.stdout.contains("auto: removed=1 skipped=0\n"));
    assert!(r.stdout.contains(&format!(
        "evidence worktree: repo={} path={} branch=worktree/c2 sha={sha} evidence=C2\n",
        a.f.repo.display(),
        wt.display()
    )));
    assert!(r.stdout.contains("total: 1 evidence-backed worktree(s)\n"));
    assert!(r.stdout.ends_with(
        "[gone] ブランチが残っていれば git prune-branches --auto で確認・削除してください。\n"
    ));
    // branch は残す(worktree だけ消す)
    assert!(branch_exists(&a.f.repo, "worktree/c2"));
}

#[test]
fn auto_log_line_format() {
    // 列: timestamp, kind=worktree, common, repo, path, branch, sha, evidence
    let a = Auto::new();
    let wt = a.f.add_wt("wt-fmt", "worktree/fmt");
    let sha = head(&wt);
    fs::write(&a.evidence, a.row(&wt, "worktree/fmt", &sha, "C2")).unwrap();
    a.run(&["--auto"], &[]);
    let log = a.log();
    let cols: Vec<&str> = log.trim_end_matches('\n').split('\t').collect();
    assert_eq!(cols.len(), 8, "{log:?}");
    assert_eq!(cols[1], "worktree");
    assert_eq!(cols[2], a.f.common);
    assert_eq!(cols[3], a.f.repo.to_str().unwrap());
    assert_eq!(cols[4], wt.to_str().unwrap());
    assert_eq!(cols[5], "worktree/fmt");
    assert_eq!(cols[6], sha);
    assert_eq!(cols[7], "C2");
    let ts = cols[0];
    assert_eq!(ts.len(), 24, "{ts}");
    assert_eq!(&ts[10..11], "T");
}

#[test]
fn auto_c1_prunes_registration_and_logs() {
    let a = Auto::new();
    let wt = a.f.add_wt("wt-c1", "worktree/c1");
    let sha = head(&wt);
    fs::remove_dir_all(&wt).unwrap();
    fs::write(&a.evidence, a.row(&wt, "worktree/c1", &sha, "C1")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(
        !worktree_registered(&a.f.common, &wt),
        "C1 の登録メタデータが残った"
    );
    assert!(
        branch_exists(&a.f.repo, "worktree/c1"),
        "C1 で branch まで削除された"
    );
    assert!(a.log().contains("\tC1"), "log.tsv に C1 の記録が無い");
    assert!(r.stdout.contains("auto: removed=1 skipped=0\n"));
}

#[test]
fn auto_dry_run_removes_and_logs_nothing() {
    let a = Auto::new();
    // 先に C2 を 1 件削除して log を作る
    let w0 = a.f.add_wt("wt-first", "worktree/first");
    fs::write(&a.evidence, a.row(&w0, "worktree/first", &head(&w0), "C2")).unwrap();
    a.run(&["--auto"], &[]);
    let before = a.log().lines().count();
    assert_eq!(before, 1);

    let wt = a.f.add_wt("wt-dry", "worktree/dry");
    fs::write(&a.evidence, a.row(&wt, "worktree/dry", &head(&wt), "C2")).unwrap();
    let r = a.run(&["--auto", "--dry-run"], &[]);
    assert!(wt.is_dir(), "削除してしまった");
    assert!(r.all().contains("--dry-run"));
    assert_eq!(
        a.log().lines().count(),
        before,
        "dry-run なのに log に書いた"
    );
    assert!(!r.stdout.contains("auto: removed="));
}

#[test]
fn auto_detached_c3_empty_branch_column() {
    let a = Auto::new();
    let wt = a.f.t().join("wt-detached");
    git(
        &a.f.repo,
        &["worktree", "add", "-q", "--detach", path_str(&wt), "HEAD"],
    );
    let sha = head(&wt);
    fs::write(&a.evidence, a.row(&wt, "", &sha, "C3:#7")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert!(
        !wt.is_dir(),
        "detached HEAD(branch 列が空)の worktree が削除されなかった: {}",
        r.all()
    );
    assert!(
        r.all().contains("removed(C3:#7): "),
        "フィールドがずれた: {}",
        r.all()
    );
    assert!(r.stdout.contains("(detached)"));
    assert!(
        a.log().contains("\tC3:#7"),
        "log.tsv に detached HEAD(C3)の記録が無い"
    );
}

#[test]
fn auto_toctou_skips_when_row_vanishes() {
    let a = Auto::new();
    let wt = a.f.add_wt("wt-toctou", "worktree/toctou");
    fs::write(&a.evidence, a.row(&wt, "worktree/toctou", &head(&wt), "C2")).unwrap();
    let hook = format!(": >'{}'", a.evidence.display());
    let r = a.run(
        &["--auto"],
        &[("GIT_PRUNE_WORKTREES_TEST_PRE_ACT_HOOK", &hook)],
    );
    assert!(wt.is_dir(), "状態変化を検知せず削除した");
    assert!(
        r.all().contains("skip(状態が変化したため見送り)"),
        "{}",
        r.all()
    );
    assert!(r.stdout.contains("auto: removed=0 skipped=1\n"));
    assert!(!r.stdout.contains("git prune-branches"));
    assert_eq!(a.log(), "");
}

#[test]
fn auto_git_refusal_is_skipped_and_dirty_is_kept() {
    let a = Auto::new();
    let wt = a.f.add_wt("wt-dirty", "worktree/dirty");
    fs::write(wt.join("t.txt"), "x").unwrap();
    git(&wt, &["add", "t.txt"]);
    fs::write(&a.evidence, a.row(&wt, "worktree/dirty", &head(&wt), "C2")).unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.code, 0);
    assert!(wt.is_dir());
    assert!(
        r.stderr
            .contains(&format!("skip(git が拒否): {}: ", wt.display())),
        "{}",
        r.stderr
    );
    assert!(r.stdout.contains("auto: removed=0 skipped=1\n"));
    assert_eq!(a.log(), "");
}

#[test]
fn auto_empty_listing_and_other_kinds_ignored() {
    let a = Auto::new();
    fs::write(&a.evidence, "").unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.stdout, "auto: 削除対象の worktree はありません。\n");
    fs::write(&a.evidence, "branch\tc\tr\t\tb\tsha\tC2\n").unwrap();
    let r = a.run(&["--auto"], &[]);
    assert_eq!(r.stdout, "auto: 削除対象の worktree はありません。\n");
    assert_eq!(r.code, 0);
}

#[test]
fn auto_default_log_dir_follows_xdg_state_home() {
    let a = Auto::new();
    let wt = a.f.add_wt("wt-xdg", "worktree/xdg");
    fs::write(&a.evidence, a.row(&wt, "worktree/xdg", &head(&wt), "C2")).unwrap();
    let xdg = a.f.t().join("xdg");
    let r = run(
        BIN,
        &["--auto"],
        a.f.t(),
        &[
            ("GIT_PRUNE_WORKTREES_AUDIT_BIN", path_str(&a.stub)),
            (
                "GIT_PRUNE_WORKTREES_TEST_EVIDENCE_FILE",
                path_str(&a.evidence),
            ),
            ("XDG_STATE_HOME", path_str(&xdg)),
        ],
        None,
    );
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(xdg.join("git-auto-prune/log.tsv").exists());
}
