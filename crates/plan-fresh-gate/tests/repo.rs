//! 実際の一時 git repo(upstream + clone)で hook を走らせる統合テスト
//! (旧 `plan-fresh-gate.sh --selftest` の 10 ケース + 出力のバイト一致)。
//!
//! `PLAN_FRESH_GATE_UNDER_TEST` に実行ファイルのパスを渡すと、そのファイル
//! (移植元の bash 版など)に同じケースを流せる(docs/rust-migration.md の段2 → 段3)。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn under_test() -> PathBuf {
    std::env::var_os("PLAN_FRESH_GATE_UNDER_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_plan-fresh-gate")))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

fn commit(dir: &Path, msg: &str) {
    git(
        dir,
        &[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            msg,
        ],
    );
}

struct T {
    dir: tempfile::TempDir,
}

impl T {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        T { dir }
    }
    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }
    fn upstream(&self, name: &str) -> PathBuf {
        self.dir.path().join(format!("{name}-upstream"))
    }
    /// upstream(main に 2 コミット)と、1 コミット目から clone した worktree(work ブランチ)。
    fn repo_pair(&self, name: &str, extra: Option<&str>) -> PathBuf {
        let up = self.upstream(name);
        let wt = self.dir.path().join(format!("{name}-worktree"));
        fs::create_dir_all(&up).unwrap();
        git(&up, &["init", "-q", "-b", "main"]);
        commit(&up, "base");
        let out = Command::new("git")
            .args(["clone", "-q"])
            .arg(&up)
            .arg(&wt)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(out.success());
        git(
            &wt,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(&wt, &["checkout", "-q", "-b", "work"]);
        if let Some(f) = extra {
            let p = up.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x\n").unwrap();
            git(&up, &["add", f]);
        }
        commit(&up, "ahead1");
        wt
    }
    fn run(&self, wt: &Path, sid: &str, plan: &str) -> String {
        let input = serde_json::json!({
            "cwd": wt, "session_id": sid, "hook_event_name": "PreToolUse",
            "tool_input": {"plan": plan},
        })
        .to_string();
        let mut child = Command::new(under_test())
            .env("CLAUDE_PROJECT_DIR", wt)
            .env("CLAUDE_PLAN_FRESH_GATE_DIR", self.state())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("SKIP_PLAN_FRESH_GATE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        use std::io::Write;
        // 早期終了(skip・gh 不在)では stdin を読まずに終わるので EPIPE は無視する。
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }
}

fn is_deny(out: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(out)
        .ok()
        .and_then(|v| v.pointer("/hookSpecificOutput/permissionDecision").cloned())
        == Some("deny".into())
}

fn reason(out: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(out).unwrap();
    v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn case1_pristine_intersect_ff_and_deny() {
    let t = T::new();
    let wt = t.repo_pair("case1", Some("config/foo.nix"));
    let out = t.run(&wt, "sid1", "config/foo.nix を編集する計画");
    assert!(is_deny(&out), "{out}");
    assert_eq!(
        git(&t.upstream("case1"), &["rev-parse", "HEAD"]),
        git(&wt, &["rev-parse", "HEAD"])
    );
    assert!(t.state().join("sid1.denied_sha").is_file());
    assert!(reason(&out).contains("fast-forward 済みです"));
}

#[test]
fn case2_pristine_no_intersect_ff_and_note_bytes() {
    let t = T::new();
    let wt = t.repo_pair("case2", Some("config/foo.nix"));
    let before = git(&wt, &["rev-parse", "--short", "HEAD"]);
    let out = t.run(&wt, "sid2", "無関係な docs/bar.md を編集する計画");
    let after = git(&wt, &["rev-parse", "--short", "HEAD"]);
    assert_ne!(before, after);
    assert_eq!(
        git(&t.upstream("case2"), &["rev-parse", "HEAD"]),
        git(&wt, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        out,
        format!("{{\n  \"systemMessage\": \"[plan-fresh-gate] origin/main へ {before} -> {after} まで fast-forward しました。プラン参照ファイルとの交差はありません。\"\n}}\n")
    );
}

#[test]
fn case3_dirty_intersect_deny_bytes() {
    let t = T::new();
    let wt = t.repo_pair("case3", Some("config/foo.nix"));
    fs::write(wt.join("untracked.txt"), "dirty\n").unwrap();
    let before = git(&wt, &["rev-parse", "HEAD"]);
    git(&wt, &["fetch", "-q", "origin", "main"]);
    let to = git(&wt, &["rev-parse", "origin/main"]);
    let stat = git(
        &wt,
        &["diff", "--stat", &before, &to, "--", "config/foo.nix"],
    );
    let out = t.run(&wt, "sid3", "config/foo.nix を編集する計画");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), before);
    let msg = format!("プラン作成後に origin/main が進行し、以下のプラン参照ファイルが変更されました。再読してプランが依然成立するか確認し、必要なら修正のうえ再度 ExitPlanMode してください。\n\n  - config/foo.nix\n\ndiffstat:\n{stat}\n\nworktree は動かしていません(作業中のコミットがある、または main 直上のため)。origin/main 側の内容は `git show origin/main:<path>` または `git diff HEAD...origin/main -- <path>` で確認してください。rebase は人間に依頼してください。");
    assert_eq!(
        out,
        format!(
            "{{\n  \"hookSpecificOutput\": {{\n    \"hookEventName\": \"PreToolUse\",\n    \"permissionDecision\": \"deny\",\n    \"permissionDecisionReason\": {}\n  }}\n}}\n",
            serde_json::to_string(&msg).unwrap()
        )
    );
    assert_eq!(
        fs::read_to_string(t.state().join("sid3.denied_sha"))
            .unwrap()
            .trim_end(),
        to
    );
}

#[test]
fn case4_same_base_sha_converges() {
    let t = T::new();
    let wt = t.repo_pair("case4", Some("config/foo.nix"));
    fs::write(wt.join("untracked.txt"), "dirty\n").unwrap();
    assert!(is_deny(&t.run(
        &wt,
        "sid4",
        "config/foo.nix を編集する計画"
    )));
    assert_eq!(t.run(&wt, "sid4", "config/foo.nix を編集する計画"), "");
}

#[test]
fn case5_incremental_redeny() {
    let t = T::new();
    let wt = t.repo_pair("case5", Some("config/foo.nix"));
    fs::write(wt.join("untracked.txt"), "dirty\n").unwrap();
    assert!(is_deny(&t.run(
        &wt,
        "sid5",
        "config/foo.nix を編集する計画"
    )));
    let up = t.upstream("case5");
    fs::write(up.join("config/bar.nix"), "y\n").unwrap();
    git(&up, &["add", "config/bar.nix"]);
    commit(&up, "ahead2");
    let out = t.run(
        &wt,
        "sid5",
        "config/foo.nix と config/bar.nix を編集する計画",
    );
    assert!(is_deny(&out), "{out}");
    let r = reason(&out);
    assert!(!r.contains("foo.nix"), "{r}");
    assert!(r.contains("bar.nix"));
}

#[test]
fn case6_up_to_date_allows() {
    let t = T::new();
    let wt = t.repo_pair("case6", Some("config/foo.nix"));
    git(&wt, &["fetch", "--quiet", "origin", "main"]);
    git(&wt, &["merge", "--ff-only", "--quiet", "origin/main"]);
    assert_eq!(t.run(&wt, "sid6", "config/foo.nix を編集する計画"), "");
}

#[test]
fn case7_base_branch_not_moved_but_denied() {
    let t = T::new();
    let wt = t.repo_pair("case7", Some("config/foo.nix"));
    git(&wt, &["checkout", "-q", "main"]);
    let before = git(&wt, &["rev-parse", "HEAD"]);
    let out = t.run(&wt, "sid7", "config/foo.nix を編集する計画");
    assert!(is_deny(&out), "{out}");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), before);
}

#[test]
fn case8_basename_match() {
    let t = T::new();
    let wt = t.repo_pair("case8", Some("deep/nested/path/unique-name.txt"));
    let out = t.run(&wt, "sid8", "unique-name.txt を直す計画(パスは省略)");
    assert!(is_deny(&out), "{out}");
    assert!(reason(&out).contains("  - deep/nested/path/unique-name.txt\n"));
}

#[test]
fn case9_no_origin_head_fail_open() {
    let t = T::new();
    let wt = t.repo_pair("case9", Some("config/foo.nix"));
    git(&wt, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
    let before = git(&wt, &["rev-parse", "HEAD"]);
    assert_eq!(t.run(&wt, "sid9", "config/foo.nix を編集する計画"), "");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), before);
}

#[test]
fn case10_many_files_no_internal_limit_and_display_cap() {
    let t = T::new();
    let wt = t.repo_pair("case10", None);
    let up = t.upstream("case10");
    let mut plan = String::from("config/needle.nix を編集する計画\n");
    for i in 1..=250 {
        fs::write(up.join(format!("file_{i:04}.txt")), "x").unwrap();
    }
    for i in 1..=60 {
        plan.push_str(&format!("file_{i:04}.txt\n"));
    }
    fs::create_dir_all(up.join("config")).unwrap();
    fs::write(up.join("config/needle.nix"), "needle\n").unwrap();
    git(&up, &["add", "-A"]);
    commit(&up, "manyfiles");
    let out = t.run(&wt, "sid10", &plan);
    assert!(is_deny(&out), "{out}");
    let r = reason(&out);
    // 61 件交差 → 50 件だけ表示し、残りは件数で
    assert!(r.contains("  ...他 11 件\n"), "{r}");
    assert!(r.contains("  - config/needle.nix\n"));
}

#[test]
fn no_plan_is_advisory() {
    let t = T::new();
    let wt = t.repo_pair("case11", Some("config/foo.nix"));
    fs::write(wt.join("untracked.txt"), "dirty\n").unwrap();
    let from = git(&wt, &["rev-parse", "--short=7", "HEAD"]);
    let input = serde_json::json!({"cwd": wt, "session_id": "s", "tool_input": {}}).to_string();
    let home = t.dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(under_test())
        .env("CLAUDE_PROJECT_DIR", &wt)
        .env("CLAUDE_PLAN_FRESH_GATE_DIR", t.state())
        .env("HOME", &home)
        .env_remove("SKIP_PLAN_FRESH_GATE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    // 早期終了(skip・gh 不在)では stdin を読まずに終わるので EPIPE は無視する。
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let to = git(&wt, &["rev-parse", "--short=7", "origin/main"]);
    assert_eq!(
        out,
        format!("{{\n  \"systemMessage\": \"[plan-fresh-gate] origin/main が進行していますが({from}..{to})、プラン本文を取得できず交差判定をスキップしました。\"\n}}\n")
    );
}

#[test]
fn skip_env_is_silent() {
    let t = T::new();
    let wt = t.repo_pair("case12", Some("config/foo.nix"));
    let input =
        serde_json::json!({"cwd": wt, "session_id": "s", "tool_input": {"plan": "config/foo.nix"}})
            .to_string();
    let mut child = Command::new(under_test())
        .env("CLAUDE_PROJECT_DIR", &wt)
        .env("CLAUDE_PLAN_FRESH_GATE_DIR", t.state())
        .env("SKIP_PLAN_FRESH_GATE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    // 早期終了(skip・gh 不在)では stdin を読まずに終わるので EPIPE は無視する。
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    assert_eq!(child.wait_with_output().unwrap().stdout, b"");
}
