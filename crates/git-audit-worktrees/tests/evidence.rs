//! bash 版 `selftest_evidence()` の移植(`--evidence` の C1/C2/C3)。
//!
//! bash は 1 つの大きい fixture に対する逐次アサーションだったので、同じ
//! fixture(`EvFx`)を作る個別テストへ分けてある。対応:
//!
//! | bash selftest_evidence のアサーション                     | テスト                                      |
//! |-----------------------------------------------------------|---------------------------------------------|
//! | C2(ancestor)の worktree                                  | c2_ancestor_worktree                        |
//! | C3(MERGED/CLOSED PR head)の worktree                     | c3_closed_pr_head_worktree                  |
//! | detached HEAD の C3                                       | c3_detached_head_worktree                   |
//! | OPEN PR の head は拾わない                                | open_pr_head_not_picked                     |
//! | 証拠の無い worktree は拾わない                            | no_evidence_worktree_not_picked             |
//! | dirty は拾わない                                          | dirty_worktree_not_picked                   |
//! | locked は拾わない                                         | locked_worktree_not_picked                  |
//! | worktree の無い branch の C2                              | loose_branch_c2                             |
//! | worktree の無い branch の C3                              | loose_branch_c3                             |
//! | 持ち主ブランチは branch 行にしない(#610, reflog / 命名)   | owner_branches_are_protected                |
//! | gh は候補 sha ごとに 1 回・commits/<sha>/pulls のみ(#586) | gh_called_once_per_candidate_sha            |
//! | C1(prunable)                                             | c1_prunable_registration                    |
//! | herdr 到達不能: worktree 行は消え branch 行は残る          | herdr_unreachable_drops_worktree_rows_only  |
//! | gh 失敗: C3 は消え C2 は残る                              | gh_failure_drops_c3_keeps_c2                |
//! | shallow clone では C2 をスキップ                          | shallow_clone_skips_c2                      |

mod common;
use common::*;
use std::fs;
use std::path::PathBuf;

struct EvFx {
    fx: Fx,
    wt_c2: PathBuf,
    wt_c3: PathBuf,
    wt_open: PathBuf,
    wt_none: PathBuf,
    wt_dirty: PathBuf,
    wt_locked: PathBuf,
    wt_detached: PathBuf,
    c3_sha: String,
}

fn add_wt(fx: &Fx, name: &str, branch: &str) -> PathBuf {
    let wt = fx.root.join(name);
    git(
        &fx.repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", branch],
    );
    wt
}

impl EvFx {
    fn new() -> Self {
        let fx = Fx::new(HERDR_EVIDENCE_STUB);
        write_exec(&fx.bin.join("gh"), GH_STUB);

        // C2: branch tip == default branch tip。
        let wt_c2 = add_wt(&fx, "wt-c2", "worktree/c2");
        // C3: main の祖先でない commit。sha は後で MERGED な PR の head にする。
        let wt_c3 = add_wt(&fx, "wt-c3", "worktree/c3");
        git(&wt_c3, &["commit", "--allow-empty", "-qm", "c3 commit"]);
        let c3_sha = git_trim(&wt_c3, &["rev-parse", "HEAD"]);
        // OPEN な PR の head: 拾ってはいけない。
        let wt_open = add_wt(&fx, "wt-open", "worktree/open-pr");
        git(
            &wt_open,
            &["commit", "--allow-empty", "-qm", "open pr commit"],
        );
        let open_sha = git_trim(&wt_open, &["rev-parse", "HEAD"]);
        // 証拠なし。
        let wt_none = add_wt(&fx, "wt-none", "worktree/none");
        git(
            &wt_none,
            &["commit", "--allow-empty", "-qm", "no evidence commit"],
        );
        // dirty: C2 の形でも guard が優先される。
        let wt_dirty = add_wt(&fx, "wt-dirty", "worktree/dirty");
        fs::write(wt_dirty.join("dirty.txt"), "").unwrap();
        // locked: 明示的な「残せ」。
        let wt_locked = add_wt(&fx, "wt-locked", "worktree/locked");
        git(
            &fx.repo,
            &[
                "worktree",
                "lock",
                wt_locked.to_str().unwrap(),
                "--reason",
                "kept on purpose",
            ],
        );
        // C3 sha を指す detached HEAD。
        let wt_detached = fx.root.join("wt-detached");
        git(
            &fx.repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                wt_detached.to_str().unwrap(),
                &c3_sha,
            ],
        );

        // #610: セッションが `git switch -c` で持ち主ブランチから離れた worktree。
        let wt_owner = fx.root.join("worktree-owner-moved");
        git(
            &fx.repo,
            &[
                "worktree",
                "add",
                "-q",
                wt_owner.to_str().unwrap(),
                "-b",
                "worktree/owner-moved",
            ],
        );
        git(&wt_owner, &["switch", "-q", "-c", "stack/stage-1"]);
        // reflog も無い場合: 命名規約(<dir>/worktree-<name> が worktree/<name> を持つ)だけが頼り。
        let wt_nolog = fx.root.join("worktree-owner-nolog");
        git(
            &fx.repo,
            &[
                "worktree",
                "add",
                "-q",
                wt_nolog.to_str().unwrap(),
                "-b",
                "worktree/owner-nolog",
            ],
        );
        git(&wt_nolog, &["switch", "-q", "-c", "stack/stage-nolog"]);
        let gd = git_trim(&wt_nolog, &["rev-parse", "--git-dir"]);
        let _ = fs::remove_file(PathBuf::from(gd).join("logs/HEAD"));

        // worktree の無い branch。
        git(&fx.repo, &["branch", "worktree/loose-c2", "main"]);
        git(&fx.repo, &["branch", "worktree/loose-c3", &c3_sha]);

        fs::write(
            fx.root.join("prs.json"),
            format!(
                r#"[
  {{"number": 10, "state": "closed", "head": {{"sha": "{c3_sha}"}}}},
  {{"number": 11, "state": "open", "head": {{"sha": "{open_sha}"}}}}
]
"#
            ),
        )
        .unwrap();

        // C3 は GitHub にしか聞かない — origin の slug を偽装して経路に乗せる。
        // push / fetch は済んでいるので ref には触らない。
        git(
            &fx.repo,
            &[
                "remote",
                "set-url",
                "origin",
                "git@github.com:acme/repo.git",
            ],
        );

        Self {
            fx,
            wt_c2,
            wt_c3,
            wt_open,
            wt_none,
            wt_dirty,
            wt_locked,
            wt_detached,
            c3_sha,
        }
    }

    fn run(&self) -> Out {
        self.run_env(&[])
    }

    fn run_env(&self, extra: &[(&str, PathBuf)]) -> Out {
        let mut c = self.fx.command();
        c.env("GIT_WORKTREE_AUDIT_GH_BIN", self.fx.bin.join("gh"))
            .env("GH_STUB_FIXTURE", self.fx.root.join("prs.json"))
            .env("GH_STUB_CALLS", self.fx.root.join("gh-calls.log"));
        for (k, v) in extra {
            c.env(k, v);
        }
        c.arg("--evidence");
        finish(&mut c)
    }

    fn gh_calls(&self) -> String {
        fs::read_to_string(self.fx.root.join("gh-calls.log")).unwrap_or_default()
    }
}

#[test]
fn c2_ancestor_worktree() {
    let e = EvFx::new();
    let o = e.run();
    assert_eq!(o.code, 0);
    assert_eq!(
        evidence_for_path(&o.stdout, &e.wt_c2).as_deref(),
        Some("C2"),
        "{}",
        o.stdout
    );
    // 行の全体形(kind common repo path branch sha evidence)。
    let sha = git_trim(&e.wt_c2, &["rev-parse", "HEAD"]);
    let row = format!(
        "worktree\t{}/.git\t{}\t{}\tworktree/c2\t{}\tC2",
        e.fx.repo.display(),
        e.fx.repo.display(),
        e.wt_c2.display(),
        sha
    );
    assert!(o.stdout.lines().any(|l| l == row), "{}", o.stdout);
}

#[test]
fn c3_closed_pr_head_worktree() {
    let e = EvFx::new();
    let o = e.run();
    assert_eq!(
        evidence_for_path(&o.stdout, &e.wt_c3).as_deref(),
        Some("C3:#10"),
        "{}",
        o.stdout
    );
}

#[test]
fn c3_detached_head_worktree() {
    let e = EvFx::new();
    let o = e.run();
    assert_eq!(
        evidence_for_path(&o.stdout, &e.wt_detached).as_deref(),
        Some("C3:#10"),
        "{}",
        o.stdout
    );
    // detached は branch 列が空。
    let row = o
        .stdout
        .lines()
        .find(|l| l.contains(e.wt_detached.to_str().unwrap()))
        .unwrap();
    assert_eq!(row.split('\t').nth(4), Some(""));
    assert_eq!(row.split('\t').nth(5), Some(e.c3_sha.as_str()));
}

#[test]
fn open_pr_head_not_picked() {
    let e = EvFx::new();
    assert_eq!(evidence_for_path(&e.run().stdout, &e.wt_open), None);
}

#[test]
fn no_evidence_worktree_not_picked() {
    let e = EvFx::new();
    assert_eq!(evidence_for_path(&e.run().stdout, &e.wt_none), None);
}

#[test]
fn dirty_worktree_not_picked() {
    let e = EvFx::new();
    assert_eq!(evidence_for_path(&e.run().stdout, &e.wt_dirty), None);
}

#[test]
fn locked_worktree_not_picked() {
    let e = EvFx::new();
    assert_eq!(evidence_for_path(&e.run().stdout, &e.wt_locked), None);
}

#[test]
fn loose_branch_c2() {
    let e = EvFx::new();
    let o = e.run();
    assert_eq!(
        evidence_for_branch(&o.stdout, "worktree/loose-c2").as_deref(),
        Some("C2"),
        "{}",
        o.stdout
    );
    // branch 行は path 列が空。
    let main_sha = git_trim(&e.fx.repo, &["rev-parse", "main"]);
    let row = format!(
        "branch\t{}/.git\t{}\t\tworktree/loose-c2\t{}\tC2",
        e.fx.repo.display(),
        e.fx.repo.display(),
        main_sha
    );
    assert!(o.stdout.lines().any(|l| l == row), "{}", o.stdout);
}

#[test]
fn loose_branch_c3() {
    let e = EvFx::new();
    let o = e.run();
    assert_eq!(
        evidence_for_branch(&o.stdout, "worktree/loose-c3").as_deref(),
        Some("C3:#10"),
        "{}",
        o.stdout
    );
}

#[test]
fn owner_branches_are_protected() {
    // #610: 使用中 worktree の持ち主ブランチ(worktree/owner-*)を deletable な
    // branch 行として報告しない(tip は main の祖先なので保護が無ければ出る)。
    let e = EvFx::new();
    let o = e.run();
    let leaked: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            f[0] == "branch" && f[4].starts_with("worktree/owner-")
        })
        .collect();
    assert!(leaked.is_empty(), "{leaked:?}");
    // 対照: 保護の対象外である loose ブランチは同じ run に出ている。
    assert!(evidence_for_branch(&o.stdout, "worktree/loose-c2").is_some());
    // 乗り換え先の stack/* ブランチは checkout 中なので branch 行にならない。
    assert!(evidence_for_branch(&o.stdout, "stack/stage-1").is_none());
}

#[test]
fn gh_called_once_per_candidate_sha() {
    // #586: gh は C2 で決まらなかった候補 sha ごとに 1 回、
    // commits/<sha>/pulls のみ(repo の PR 数には依らない)。
    let e = EvFx::new();
    e.run();
    let log = e.gh_calls();
    for l in log.lines() {
        assert!(
            l.starts_with("api repos/acme/repo/commits/") && l.contains("/pulls "),
            "commits/<sha>/pulls 以外の gh 呼び出し: {l}"
        );
        assert!(l.contains("--paginate --jq "), "{l}");
    }
    let n = log.lines().count();
    assert!(
        (1..=12).contains(&n),
        "gh 呼び出し回数が候補数に見合わない({n} 回): {log}"
    );
    // 同じ sha(c3 は worktree 行・detached 行・branch 行で共有)は 1 回だけ。
    let c3_calls = log
        .lines()
        .filter(|l| l.contains(&format!("/commits/{}/pulls", e.c3_sha)))
        .count();
    assert_eq!(c3_calls, 1, "{log}");
}

#[test]
fn c1_prunable_registration() {
    let e = EvFx::new();
    let gone = e.fx.root.join("wt-gone");
    git(
        &e.fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            gone.to_str().unwrap(),
            "-b",
            "worktree/gone-registration",
        ],
    );
    fs::remove_dir_all(&gone).unwrap();
    let o = e.run();
    assert_eq!(
        evidence_for_path(&o.stdout, &gone).as_deref(),
        Some("C1"),
        "{}",
        o.stdout
    );
    // herdr が落ちていても C1 は残る(守るべき checkout が無い)。
    e.fx.set_herdr(HERDR_DOWN_STUB);
    let o = e.run();
    assert_eq!(
        evidence_for_path(&o.stdout, &gone).as_deref(),
        Some("C1"),
        "{}",
        o.stdout
    );
}

#[test]
fn herdr_unreachable_drops_worktree_rows_only() {
    let e = EvFx::new();
    e.fx.set_herdr(HERDR_DOWN_STUB);
    let o = e.run();
    let kinds: Vec<&str> = o
        .stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert!(
        !kinds.contains(&"worktree"),
        "herdr 到達不能なのに worktree 行が残った: {}",
        o.stdout
    );
    assert!(
        kinds.contains(&"branch"),
        "branch 行まで消えた: {}",
        o.stdout
    );
}

#[test]
fn gh_failure_drops_c3_keeps_c2() {
    let e = EvFx::new();
    let fail = e.fx.root.join("gh-fail");
    fs::write(&fail, "").unwrap();
    let o = e.run_env(&[("GH_STUB_FAIL", fail)]);
    assert_eq!(
        evidence_for_path(&o.stdout, &e.wt_c2).as_deref(),
        Some("C2"),
        "{}",
        o.stdout
    );
    assert_eq!(evidence_for_path(&o.stdout, &e.wt_c3), None, "{}", o.stdout);
    assert_eq!(evidence_for_branch(&o.stdout, "worktree/loose-c3"), None);
}

#[test]
fn shallow_clone_skips_c2() {
    let e = EvFx::new();
    let shallow = e.fx.add_shallow_repo();
    let wt = e.fx.root.join("wt-shallow");
    git(
        &shallow,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "worktree/shallow-loose",
        ],
    );
    let o = e.run();
    assert_eq!(evidence_for_path(&o.stdout, &wt), None, "{}", o.stdout);
    // 対照: 同じ run で通常 repo の C2 は検出されている。
    assert_eq!(
        evidence_for_path(&o.stdout, &e.wt_c2).as_deref(),
        Some("C2")
    );
}
