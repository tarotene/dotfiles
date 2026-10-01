//! 引数・出力形式・state ファイルの結合テスト(bash 版 --selftest には無かった
//! 部分)と、bash 版との差分(オラクル)テスト。
//!
//! オラクルテストは `scripts/git-audit-worktrees` が存在する間だけ走る
//! (bash 版を削除する段で自然に skip され、不要になったら消せる)。

mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn add_wt(fx: &Fx, name: &str, branch: &str) -> PathBuf {
    let wt = fx.root.join(name);
    git(
        &fx.repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", branch],
    );
    wt
}

fn stale(fx: &Fx, name: &str, branch: &str) -> PathBuf {
    let wt = add_wt(fx, name, branch);
    fs::remove_dir_all(&wt).unwrap();
    wt
}

#[test]
fn help_prints_usage_to_stdout_and_exits_0() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    for flag in ["--help", "-h"] {
        let o = fx.run(&[flag]);
        assert_eq!(o.code, 0);
        assert!(o.stdout.starts_with(
            "usage: git audit-worktrees [--notify|--context|--porcelain|--evidence]\n"
        ));
        assert!(o.stdout.contains("--evidence emits a separate TSV"));
        assert!(o.stderr.is_empty());
    }
}

#[test]
fn unknown_argument_prints_usage_to_stderr_and_exits_2() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let o = fx.run(&["--bogus"]);
    assert_eq!(o.code, 2);
    assert!(o.stdout.is_empty());
    assert!(o.stderr.starts_with("usage: git audit-worktrees"));
}

#[test]
fn no_findings_modes_are_quiet() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let o = fx.run(&[]);
    assert_eq!(
        (o.code, o.stdout.as_str()),
        (0, "stale worktree はありません。\n")
    );
    for mode in ["--porcelain", "--context", "--evidence", "--notify"] {
        let o = fx.run(&[mode]);
        assert_eq!(
            (o.code, o.stdout.as_str(), o.stderr.as_str()),
            (0, "", ""),
            "{mode}"
        );
    }
}

#[test]
fn last_mode_flag_wins() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    stale(&fx, "gone1", "test/gone1");
    let o = fx.run(&["--notify", "--porcelain"]);
    assert!(o.stdout.contains("\tprunable\t"));
}

#[test]
fn porcelain_row_shape_for_prunable() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = stale(&fx, "gone1", "test/gone1");
    let o = fx.run(&["--porcelain"]);
    assert_eq!(o.code, 0);
    let f: Vec<&str> = o.stdout.trim_end().split('\t').collect();
    assert_eq!(f.len(), 6, "{}", o.stdout);
    assert_eq!(f[0], format!("{}/.git", fx.repo.display()));
    assert_eq!(f[1], fx.repo.to_str().unwrap());
    assert_eq!(f[2], wt.to_str().unwrap());
    assert_eq!((f[3], f[4]), ("test/gone1", "prunable"));
    assert!(f[5].starts_with("gitdir file points to non-existent location"));
}

#[test]
fn context_emits_session_start_json_with_literal_backslash_n() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = stale(&fx, "gone1", "test/gone1");
    let o = fx.run(&["--context"]);
    assert_eq!(o.code, 0);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    // bash のダブルクォート内の \n は改行にならず、バックスラッシュ+n のまま。
    assert!(ctx.starts_with("[git-worktree-audit] 存在しない checkout の登録、または開いていない残骸 checkout があります。自動削除せず、git prune-worktrees で確認してください。\\nstale worktree: repo="), "{ctx}");
    assert!(ctx.contains(&format!("path={}", wt.display())));
    assert!(ctx.ends_with("total: 1 stale worktree finding(s)"));
    assert!(o.stdout.starts_with("{\n  \"hookSpecificOutput\": {\n    \"hookEventName\": \"SessionStart\",\n    \"additionalContext\": \""));
}

#[test]
fn notify_writes_state_file_and_reports_first_detection() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    stale(&fx, "gone1", "test/gone1");
    fx.set_reason("busy");
    let o = fx.run(&["--notify"]);
    assert_eq!(o.code, 0);
    assert!(
        o.stderr
            .starts_with("git-worktree-audit: first detected at "),
        "{}",
        o.stderr
    );
    assert!(o.stderr.contains("stale worktree: repo="));
    let st = fs::read_to_string(fx.state_json()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&st).unwrap();
    assert_eq!(v["notified"], false);
    assert_eq!(v["fingerprint"].as_str().unwrap().len(), 64);
    assert!(st.starts_with("{\n  \"fingerprint\": \""), "{st}");
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(fx.state_json()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // shown になると notified=true、以降は無言。
    fx.set_reason("shown");
    assert_eq!(fx.run(&["--notify"]).stderr, "");
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(fx.state_json()).unwrap()).unwrap();
    assert_eq!(v["notified"], true);
    // 検出が空になると state が消える。
    git(&fx.repo, &["worktree", "prune", "--expire=now"]);
    assert_eq!(fx.run(&["--notify"]).code, 0);
    assert!(!fx.state_json().exists());
}

#[test]
fn notify_passes_title_body_and_sound_to_herdr() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    stale(&fx, "gone1", "test/gone1");
    let argv = fx.root.join("argv");
    fx.set_herdr(&format!(
        "#!/usr/bin/env bash\nif [[ \"$1 $2\" == \"worktree list\" ]]; then printf '{{\"result\":{{\"worktrees\":[]}}}}\\n'; exit 0; fi\nprintf '%s\\0' \"$@\" >'{}'\nprintf '{{\"result\":{{\"shown\":true}}}}\\n'\n",
        argv.display()
    ));
    assert_eq!(fx.run(&["--notify"]).code, 0);
    let a = fs::read_to_string(&argv).unwrap();
    let l: Vec<&str> = a.trim_end_matches('\0').split('\0').collect();
    assert_eq!(
        &l[..3],
        ["notification", "show", "stale worktree を検出 (1件)"]
    );
    assert_eq!(l[3], "--body");
    assert!(l[4].starts_with("stale worktree: repo="));
    assert_eq!(&l[5..], ["--sound", "request"]);
}

#[test]
fn notify_body_is_truncated_to_230_chars() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    // 長い path で報告行を 230 文字超にする。
    let long = "x".repeat(60);
    stale(&fx, &format!("{long}-1"), "test/long1");
    stale(&fx, &format!("{long}-2"), "test/long2");
    let argv = fx.root.join("argv");
    fx.set_herdr(&format!(
        "#!/usr/bin/env bash\nif [[ \"$1 $2\" == \"worktree list\" ]]; then printf '{{\"result\":{{\"worktrees\":[]}}}}\\n'; exit 0; fi\nprintf '%s' \"$5\" >'{}'\nprintf '{{\"result\":{{\"shown\":true}}}}\\n'\n",
        argv.display()
    ));
    let o = finish(fx.command().env("LC_ALL", "C.UTF-8").arg("--notify"));
    assert_eq!(o.code, 0);
    let body = fs::read_to_string(&argv).unwrap();
    assert_eq!(body.chars().count(), 230, "{body}");
}

#[test]
fn herdr_with_empty_stdout_counts_as_reachable() {
    // bash 版: `jq` は空入力で成功する → 到達可能・open なし。
    let fx = Fx::new("#!/usr/bin/env bash\nexit 0\n");
    let wt = add_wt(&fx, "orphan-empty", "worktree/orphan-empty");
    let o = fx.run(&["--porcelain"]);
    assert!(o.stdout.contains(wt.to_str().unwrap()), "{}", o.stdout);
}

#[test]
fn herdr_open_workspace_protects_worktree() {
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = add_wt(&fx, "orphan-open", "worktree/orphan-open");
    fx.set_herdr(&format!(
        "#!/usr/bin/env bash\nif [[ \"$1 $2\" == \"worktree list\" ]]; then printf '{{\"result\":{{\"worktrees\":[{{\"path\":\"{}\",\"open_workspace_id\":\"w1\"}}]}}}}\\n'; exit 0; fi\nexit 1\n",
        wt.display()
    ));
    let o = fx.run(&["--porcelain"]);
    assert!(!o.stdout.contains(wt.to_str().unwrap()), "{}", o.stdout);
}

#[test]
fn detached_prunable_collapses_empty_branch_field_like_bash() {
    // bash 版の `IFS=$'\t' read` は連続タブを潰すので、branch が空の行は
    // 列が 1 つずつずれる。互換のため同じ見た目を保つ(オラクルテスト参照)。
    let fx = Fx::new(HERDR_NOTIFY_STUB);
    let wt = fx.root.join("detached-gone");
    git(
        &fx.repo,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    fs::remove_dir_all(&wt).unwrap();
    let o = fx.run(&[]);
    assert_eq!(o.code, 1);
    assert!(
        o.stdout.contains(&format!(
            "path={} branch=prunable class=gitdir file points to non-existent location reason=\n",
            wt.display()
        )),
        "{}",
        o.stdout
    );
}

// ---------------------------------------------------------------------
// bash 版オラクル
// ---------------------------------------------------------------------

fn bash_script() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/git-audit-worktrees");
    p.is_file().then_some(p)
}

/// 全クラスを含む fixture(prunable・detached prunable・orphaned・[gone]・
/// evidence の各クラス・owner 保護)を作る。
fn rich_fixture() -> Fx {
    let fx = Fx::new(HERDR_EVIDENCE_STUB);
    write_exec(&fx.bin.join("gh"), GH_STUB);
    stale(&fx, "gone1", "test/gone1");
    let d = fx.root.join("detached-gone");
    git(
        &fx.repo,
        &["worktree", "add", "-q", "--detach", d.to_str().unwrap()],
    );
    fs::remove_dir_all(&d).unwrap();
    add_wt(&fx, "orphan-empty", "worktree/orphan-empty");
    let g = add_wt(&fx, "orphan-gone", "worktree/orphan-gone");
    git(&g, &["push", "-q", "-u", "origin", "worktree/orphan-gone"]);
    git(
        &fx.repo,
        &["push", "-q", "origin", "--delete", "worktree/orphan-gone"],
    );
    git(&g, &["fetch", "-q", "--prune", "origin"]);
    let c3 = add_wt(&fx, "wt-c3", "worktree/c3");
    git(&c3, &["commit", "--allow-empty", "-qm", "c3 commit"]);
    let c3_sha = git_trim(&c3, &["rev-parse", "HEAD"]);
    add_wt(&fx, "wt-dirty", "worktree/dirty");
    fs::write(fx.root.join("wt-dirty/x"), "").unwrap();
    let o = fx.root.join("worktree-owner-moved");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "-q",
            o.to_str().unwrap(),
            "-b",
            "worktree/owner-moved",
        ],
    );
    git(&o, &["switch", "-q", "-c", "stack/s1"]);
    git(&fx.repo, &["branch", "worktree/loose-c2", "main"]);
    git(&fx.repo, &["branch", "worktree/loose-c3", &c3_sha]);
    fs::write(
        fx.root.join("prs.json"),
        format!(r#"[{{"number": 10, "state": "closed", "head": {{"sha": "{c3_sha}"}}}}]"#),
    )
    .unwrap();
    git(
        &fx.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "git@github.com:acme/repo.git",
        ],
    );
    fx
}

fn bash_run(fx: &Fx, script: &Path, args: &[&str]) -> Out {
    let mut c = Command::new("bash");
    c.arg(script)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_WORKTREE_AUDIT_GHR_DIR", fx.root.join("ghr"))
        .env("GIT_WORKTREE_AUDIT_HERDR_DIR", fx.root.join("herdr"))
        .env("GIT_WORKTREE_AUDIT_STATE_DIR", fx.root.join("state-bash"))
        .env("GIT_WORKTREE_AUDIT_HERDR_BIN", fx.bin.join("herdr"))
        .env("GIT_WORKTREE_AUDIT_GH_BIN", fx.bin.join("gh"))
        .env("GH_STUB_FIXTURE", fx.root.join("prs.json"))
        .env("GH_STUB_CALLS", fx.root.join("gh-calls-bash.log"))
        .env("GIT_WORKTREE_AUDIT_TEST_CALLS", fx.root.join("calls"))
        .env("GIT_WORKTREE_AUDIT_TEST_REASON", fx.root.join("reason"));
    finish(&mut c)
}

fn rust_run(fx: &Fx, args: &[&str]) -> Out {
    let mut c = fx.command();
    c.env("GIT_WORKTREE_AUDIT_GH_BIN", fx.bin.join("gh"))
        .env("GH_STUB_FIXTURE", fx.root.join("prs.json"))
        .env("GH_STUB_CALLS", fx.root.join("gh-calls-rust.log"))
        .args(args);
    finish(&mut c)
}

fn sorted_lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.lines().collect();
    v.sort();
    v
}

#[test]
fn oracle_scan_modes_match_bash() {
    let Some(script) = bash_script() else { return };
    let fx = rich_fixture();
    for args in [&[][..], &["--porcelain"], &["--context"], &["--evidence"]] {
        let b = bash_run(&fx, &script, args);
        let r = rust_run(&fx, args);
        assert_eq!(r.code, b.code, "{args:?}");
        // find の列挙順は実装間で保証されないので行集合で比べる(--context は全体)。
        if args == ["--context"] {
            assert_eq!(r.stdout, b.stdout, "{args:?}");
        } else {
            assert_eq!(sorted_lines(&r.stdout), sorted_lines(&b.stdout), "{args:?}");
        }
        assert_eq!(r.stderr, b.stderr, "{args:?}");
        assert!(!r.stdout.is_empty(), "fixture が空: {args:?}");
    }
    // gh に聞いた (slug, sha) の集合は一致する。回数は一致しない: bash 版の
    // per-slug@sha キャッシュは `$(gh_closed_pr …)` のサブシェル内で更新される
    // ため実際には効かず、同じ sha を worktree 行・branch 行で 2 回聞く。Rust 版
    // は元のコメントどおり sha ごとに 1 回(#586 の意図)。
    let uniq = |p: &str| {
        let mut v: Vec<String> = fs::read_to_string(fx.root.join(p))
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        v.sort();
        v.dedup();
        v
    };
    assert_eq!(uniq("gh-calls-rust.log"), uniq("gh-calls-bash.log"));
    let n = |p: &str| fs::read_to_string(fx.root.join(p)).unwrap().lines().count();
    assert!(n("gh-calls-rust.log") <= n("gh-calls-bash.log"));
}

#[test]
fn oracle_notify_matches_bash() {
    let Some(script) = bash_script() else { return };
    let fx = rich_fixture();
    fx.set_herdr(HERDR_NOTIFY_STUB);
    for reason in ["busy", "shown", "shown", "disabled", "weird"] {
        fx.set_reason(reason);
        let _ = fs::remove_dir_all(fx.root.join("state-bash"));
        let _ = fs::remove_dir_all(&fx.state);
        let mut b = bash_run(&fx, &script, &["--notify"]);
        let mut r = rust_run(&fx, &["--notify"]);
        // first_seen の時刻だけ正規化。
        for o in [&mut b, &mut r] {
            o.stderr = o
                .stderr
                .lines()
                .map(|l| {
                    if l.starts_with("git-worktree-audit: first detected at ") {
                        "first-detected".to_string()
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        assert_eq!(
            (r.code, &r.stdout, &r.stderr),
            (b.code, &b.stdout, &b.stderr),
            "{reason}"
        );
        let strip = |p: PathBuf| {
            let s = fs::read_to_string(p.join("state.json")).unwrap_or_default();
            s.lines()
                .filter(|l| !l.contains("first_seen"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(
            strip(fx.state.clone()),
            strip(fx.root.join("state-bash")),
            "{reason}"
        );
    }
    let calls = fs::read_to_string(fx.root.join("calls"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(calls % 2, 0, "bash と Rust で同数回呼ばれる");
}
