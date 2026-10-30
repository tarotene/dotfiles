//! worklog の統合テスト: 一時ディレクトリの vault と git リポジトリで実バイナリを走らせる。

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 2026-10-25 09:12 JST。
const NOW: &str = "1792887120";
const TODAY_NOTE: &str = "journal/2026/2026-10-25.md";

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    vault: PathBuf,
    repo: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let vault = root.join("vault");
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    std::fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    std::fs::write(
        vault.join(".obsidian/daily-notes.json"),
        r#"{"folder":"journal","format":"YYYY/YYYY-MM-DD"}"#,
    )
    .unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
    Env {
        _dir: dir,
        root,
        vault,
        repo,
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `vault` が `Some` なら `OBSIDIAN_VAULT` で渡す。HOME と XDG_CONFIG_HOME は一時ディレクトリ。
fn run(e: &Env, cwd: &Path, vault: Option<&Path>, args: &[&str], stdin: &str) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_worklog"));
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", &e.root)
        .env("XDG_CONFIG_HOME", e.root.join("config"))
        .env("WORKLOG_NOW", NOW)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(v) = vault {
        cmd.env("OBSIDIAN_VAULT", v);
    }
    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let mut si = child.stdin.take().unwrap();
        let _ = si.write_all(stdin.as_bytes());
    }
    let out = child.wait_with_output().unwrap();
    Out {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn note(e: &Env) -> String {
    std::fs::read_to_string(e.vault.join(TODAY_NOTE)).unwrap()
}

#[test]
fn add_stamps_the_time_and_the_repo_from_the_working_directory() {
    let e = env();
    let o = run(
        &e,
        &e.repo,
        Some(&e.vault),
        &["add", "stuck", "login", "fails", "--ref", "12"],
        "",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.starts_with("追記した: "), "{}", o.stdout);
    assert_eq!(
        note(&e),
        "## 作業ログ\n- 09:12 [stuck] login fails (o/r#12)\n"
    );
    // 同じ行は重ねない。
    let again = run(
        &e,
        &e.repo,
        Some(&e.vault),
        &["add", "stuck", "login fails", "--ref", "12"],
        "",
    );
    assert!(
        again.stdout.starts_with("既に記録済み: "),
        "{}",
        again.stdout
    );
    assert_eq!(note(&e).lines().count(), 2);
}

#[test]
fn outside_git_there_is_no_reference_and_ref_needs_a_repo() {
    let e = env();
    let outside = e.root.join("plain");
    std::fs::create_dir_all(&outside).unwrap();
    let ok = run(
        &e,
        &outside,
        Some(&e.vault),
        &["add", "task", "工房に行った"],
        "",
    );
    assert_eq!(ok.code, 0, "{}", ok.stderr);
    assert_eq!(note(&e), "## 作業ログ\n- 09:12 [task] 工房に行った\n");

    let bad = run(
        &e,
        &outside,
        Some(&e.vault),
        &["add", "task", "x", "--ref", "1"],
        "",
    );
    assert_eq!(bad.code, 2);
    assert!(bad.stderr.contains("--repo"), "{}", bad.stderr);
    // --repo で明示すれば書ける。
    let ok = run(
        &e,
        &outside,
        Some(&e.vault),
        &["add", "task", "y", "--repo", "a/b", "--ref", "#2"],
        "",
    );
    assert_eq!(ok.code, 0, "{}", ok.stderr);
    assert!(note(&e).contains("- 09:12 [task] y (a/b#2)"));
}

#[test]
fn rejects_what_the_closed_vocabulary_forbids() {
    let e = env();
    let k = run(&e, &e.repo, Some(&e.vault), &["add", "memo", "x"], "");
    assert_eq!(k.code, 2);
    assert!(
        k.stderr.contains("reply|deadline|task|pr|found|stuck"),
        "{}",
        k.stderr
    );
    let empty = run(&e, &e.repo, Some(&e.vault), &["add", "task"], "");
    assert_eq!(empty.code, 2);
    let r = run(
        &e,
        &e.repo,
        Some(&e.vault),
        &["add", "task", "x", "--ref", "abc"],
        "",
    );
    assert_eq!(r.code, 2);
    assert!(!e.vault.join(TODAY_NOTE).exists());
}

#[test]
fn list_filters_by_repo_and_kind_and_reads_memos() {
    let e = env();
    let p = e.vault.join(TODAY_NOTE);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "## 作業ログ\n- 08:00 [found] 分かった (o/r#1)\n- 08:30 [stuck] 別の詰まり (o/other)\n- 手書きの行\n",
    )
    .unwrap();
    let all = run(&e, &e.repo, Some(&e.vault), &["list"], "");
    assert_eq!(all.code, 0, "{}", all.stderr);
    assert_eq!(
        all.stdout,
        "2026-10-25\t08:00\tfound\t分かった\to/r#1\n\
         2026-10-25\t08:30\tstuck\t別の詰まり\to/other\n\
         2026-10-25\t-\tmemo\t手書きの行\t-\n"
    );
    let one = run(
        &e,
        &e.repo,
        Some(&e.vault),
        &["list", "--repo", "o/r", "--kind", "found,stuck"],
        "",
    );
    assert_eq!(one.stdout, "2026-10-25\t08:00\tfound\t分かった\to/r#1\n");
    let memo = run(&e, &e.repo, Some(&e.vault), &["list", "--kind", "memo"], "");
    assert_eq!(memo.stdout, "2026-10-25\t-\tmemo\t手書きの行\t-\n");
    let bad = run(&e, &e.repo, Some(&e.vault), &["list", "--kind", "nope"], "");
    assert_eq!(bad.code, 2);
}

#[test]
fn mail_is_validated_without_a_repo_reference_and_listed_by_kind() {
    let e = env();
    let line = "draft: 面談の日程回答の下書きを作った [gmail:abc123/m1]";
    // git のリポジトリの中で呼んでも、mail には owner/repo が付かない。
    let ok = run(&e, &e.repo, Some(&e.vault), &["add", "mail", line], "");
    assert_eq!(ok.code, 0, "{}", ok.stderr);
    assert_eq!(note(&e), format!("## 作業ログ\n- 09:12 [mail] {line}\n"));
    // 別の作業ディレクトリから呼んでも同じ行は重ならない(冪等の鍵は出典の id)。
    let outside = e.root.join("plain");
    std::fs::create_dir_all(&outside).unwrap();
    let again = run(&e, &outside, Some(&e.vault), &["add", "mail", line], "");
    assert!(
        again.stdout.starts_with("既に記録済み: "),
        "{}",
        again.stdout
    );
    let agg = "read: 直近 3 日の約 490 件を読んだ(自動通知を除く処分 4 件) [none]";
    let ok = run(&e, &e.repo, Some(&e.vault), &["add", "mail", agg], "");
    assert_eq!(ok.code, 0, "{}", ok.stderr);

    // 文法に合わない mail と、--ref つきは止まり、ノートを変えない。
    let before = note(&e);
    for args in [
        &["add", "mail", "read: a@b.c [gmail:x/y]"][..],
        &["add", "mail", "skip: 読んだ [gmail:x/y]"][..],
        &["add", "mail", "read: 出典なし"][..],
        &["add", "mail", "read: 旧い文法 [gmail:x]"][..],
        &["add", "mail", "read: 読んだ [gmail:x/y]", "--ref", "1"][..],
    ] {
        let bad = run(&e, &e.repo, Some(&e.vault), args, "");
        assert_eq!(bad.code, 2, "{args:?}");
    }
    assert_eq!(note(&e), before);

    // list --kind mail で、記録済みのスレッドを id で引ける。
    let l = run(&e, &e.repo, Some(&e.vault), &["list", "--kind", "mail"], "");
    assert_eq!(l.code, 0, "{}", l.stderr);
    assert!(l.stdout.contains("[gmail:abc123/m1]"), "{}", l.stdout);
    assert_eq!(l.stdout.lines().count(), 2);
    // 文法はヘルプにも出る。
    let h = run(&e, &e.repo, Some(&e.vault), &["add"], "");
    assert!(h.stderr.contains("mail の本文"), "{}", h.stderr);
}

#[test]
fn no_vault_declared_fails_loudly_for_the_cli_and_stays_silent_for_the_hook() {
    let e = env();
    let cli = run(&e, &e.repo, None, &["add", "task", "x"], "");
    assert_eq!(cli.code, 2);
    assert!(cli.stderr.contains("obsidian-vault"), "{}", cli.stderr);

    let hook = run(&e, &e.repo, None, &["session-start"], "{}");
    assert_eq!(
        (hook.code, hook.stdout.as_str(), hook.stderr.as_str()),
        (0, "", "")
    );
}

fn declare_by_marker(e: &Env) {
    let dir = e.root.join("config/dotfiles");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("obsidian-vault"),
        format!("{}\n", e.vault.display()),
    )
    .unwrap();
}

#[test]
fn session_start_injects_the_norm_and_this_repos_recent_lines() {
    let e = env();
    declare_by_marker(&e);
    let p = e.vault.join("journal/2026/2026-10-24.md");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "## 作業ログ\n- 11:05 [stuck] ここの詰まり (o/r#12)\n- 12:00 [stuck] 別のリポジトリ (o/other#1)\n- 13:00 [task] 種類が対象外 (o/r)\n",
    )
    .unwrap();
    // 8 日前の行は窓の外。
    let old = e.vault.join("journal/2026/2026-10-17.md");
    std::fs::write(&old, "## 作業ログ\n- 11:05 [stuck] 古い詰まり (o/r)\n").unwrap();

    let stdin = format!(r#"{{"cwd":"{}"}}"#, e.repo.display());
    let o = run(&e, &e.root, None, &["session-start"], &stdin);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let v: Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("worklog add"), "{ctx}");
    assert!(
        ctx.contains("- 2026-10-24 11:05 [stuck] ここの詰まり (#12)"),
        "{ctx}"
    );
    assert!(!ctx.contains("別のリポジトリ"), "{ctx}");
    assert!(!ctx.contains("種類が対象外"), "{ctx}");
    assert!(!ctx.contains("古い詰まり"), "{ctx}");
}

#[test]
fn session_start_outside_git_carries_the_norm_only() {
    let e = env();
    declare_by_marker(&e);
    let o = run(&e, &e.root, None, &["session-start"], "{}");
    let v: Value = serde_json::from_str(&o.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        ctx.contains("worklog add") && !ctx.contains("直近"),
        "{ctx}"
    );
}

#[test]
fn session_start_for_copilot_puts_additional_context_at_the_top_level() {
    // Copilot CLI consumes only a top-level `additionalContext` for sessionStart
    // (GitHub Docs, "GitHub Copilot hooks reference", retrieved 2026-10-28).
    let e = env();
    declare_by_marker(&e);
    let stdin = format!(r#"{{"cwd":"{}"}}"#, e.repo.display());
    let o = run(
        &e,
        &e.root,
        None,
        &["session-start", "--agent", "copilot"],
        &stdin,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let v: Value = serde_json::from_str(&o.stdout).unwrap();
    assert!(v.get("hookSpecificOutput").is_none(), "{v}");
    assert!(
        v["additionalContext"]
            .as_str()
            .unwrap()
            .contains("worklog add"),
        "{v}"
    );
    assert_eq!(v.as_object().unwrap().len(), 1, "{v}");
}

#[test]
fn session_start_for_codex_keeps_the_nested_shape() {
    let e = env();
    declare_by_marker(&e);
    let o = run(
        &e,
        &e.root,
        None,
        &["session-start", "--agent", "codex"],
        "{}",
    );
    let v: Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    assert!(v.get("additionalContext").is_none(), "{v}");
}

#[test]
fn session_start_rejects_an_unknown_agent_or_argument() {
    let e = env();
    declare_by_marker(&e);
    let bad_agent = run(
        &e,
        &e.root,
        None,
        &["session-start", "--agent", "nope"],
        "{}",
    );
    assert_eq!(bad_agent.code, 2);
    let bad_arg = run(&e, &e.root, None, &["session-start", "--wat"], "{}");
    assert_eq!(bad_arg.code, 2);
}

#[test]
fn a_declared_but_broken_vault_is_reported_once_and_never_blocks() {
    let e = env();
    std::fs::remove_file(e.vault.join(".obsidian/daily-notes.json")).unwrap();
    let o = run(&e, &e.repo, Some(&e.vault), &["session-start"], "{}");
    assert_eq!(o.code, 0);
    assert_eq!(o.stdout, "");
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    assert!(o.stderr.starts_with("[worklog] "), "{}", o.stderr);
}

#[test]
fn concurrent_writers_do_not_lose_or_tear_lines() {
    let e = env();
    let n = 24;
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let (root, vault, repo) = (e.root.clone(), e.vault.clone(), e.repo.clone());
            std::thread::spawn(move || {
                let e = Env {
                    _dir: tempfile::tempdir().unwrap(),
                    root,
                    vault: vault.clone(),
                    repo: repo.clone(),
                };
                let text = format!("並行 {i}");
                let o = run(&e, &repo, Some(&vault), &["add", "found", &text], "");
                assert_eq!(o.code, 0, "{}", o.stderr);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let c = note(&e);
    assert_eq!(c.lines().filter(|l| *l == "## 作業ログ").count(), 1, "{c}");
    for i in 0..n {
        assert_eq!(
            c.lines()
                .filter(|l| *l == format!("- 09:12 [found] 並行 {i} (o/r)"))
                .count(),
            1,
            "{c}"
        );
    }
    assert_eq!(c.lines().count(), n + 1, "{c}");
}

// ---------------------------------------------------------------- slot(作業の枠)

/// 2026-11-01 09:12 JST。`slot` は 2026-10-30 より前のノートを畳み込まないので、この日を起点にする。
const SLOT_T0: u64 = 1_793_491_920;

/// `days_after` 日後の同じ時刻に `WORKLOG_NOW` を固定して走らせる。
fn slot_run(e: &Env, days_after: u64, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_worklog"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", &e.root)
        .env("XDG_CONFIG_HOME", e.root.join("config"))
        .env("OBSIDIAN_VAULT", &e.vault)
        .env("WORKLOG_NOW", (SLOT_T0 + days_after * 86_400).to_string())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(&e.repo)
        .args(args)
        .output()
        .unwrap();
    Out {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn slot_note(e: &Env, date: &str) -> String {
    std::fs::read_to_string(e.vault.join(format!("journal/{}/{date}.md", &date[..4])))
        .unwrap_or_default()
}

const SLOT_W1: [&str; 4] = [
    "--start",
    "2026-11-02T10:00+09:00",
    "--end",
    "2026-11-02T12:00+09:00",
];

#[test]
fn slot_walks_plan_resched_done_and_open_and_overdue_follow() {
    let e = env();
    let plan = [
        &["add", "slot", "plan", "reader-bot-resume"][..],
        &SLOT_W1[..],
        &["--issue", "example/repo#12"][..],
    ]
    .concat();
    let p = slot_run(&e, 0, &plan);
    assert_eq!(p.code, 0, "{}{}", p.stdout, p.stderr);
    assert!(p.stdout.starts_with("追記した: "), "{}", p.stdout);
    assert_eq!(
        slot_note(&e, "2026-11-01"),
        "## 作業ログ\n- 09:12 [slot] plan: reader-bot-resume [start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00 issue:example/repo#12]\n"
    );
    // 同じ命令の再実行は記録済み。内容の違う plan は、鍵が使い回せないので止まる。
    let again = slot_run(&e, 0, &plan);
    assert!(
        again.stdout.starts_with("既に記録済み: "),
        "{}",
        again.stdout
    );
    let other = slot_run(
        &e,
        0,
        &[
            "add",
            "slot",
            "plan",
            "reader-bot-resume",
            "--start",
            "2026-11-05T10:00+09:00",
            "--end",
            "2026-11-05T12:00+09:00",
        ],
    );
    assert_eq!(other.code, 2);
    assert!(other.stderr.contains("既にある"), "{}", other.stderr);
    assert_eq!(slot_note(&e, "2026-11-01").lines().count(), 2);

    // 未了の一覧(TSV と JSON)。
    let open = slot_run(&e, 0, &["slot", "open"]);
    assert_eq!(
        open.stdout,
        "reader-bot-resume\t2026-11-02T10:00+09:00\t2026-11-02T12:00+09:00\texample/repo#12\n"
    );
    let js: Value =
        serde_json::from_str(slot_run(&e, 0, &["slot", "open", "--json"]).stdout.trim()).unwrap();
    assert_eq!(js[0]["key"], "reader-bot-resume");
    assert_eq!(js[0]["state"], "scheduled");
    assert_eq!(js[0]["issues"][0], "example/repo#12");

    // 終了を過ぎたものだけが overdue。`--now` は別のオフセットでもよい。
    let before = slot_run(
        &e,
        0,
        &["slot", "overdue", "--now", "2026-11-02T12:00+09:00"],
    );
    assert_eq!((before.code, before.stdout.as_str()), (0, ""));
    let after = slot_run(&e, 0, &["slot", "overdue", "--now", "2026-11-02T03:00:01Z"]);
    assert_eq!(after.code, 0, "{}", after.stderr);
    assert!(
        after.stdout.starts_with("reader-bot-resume\t"),
        "{}",
        after.stdout
    );
    let after_json: Value = serde_json::from_str(
        slot_run(
            &e,
            0,
            &[
                "slot",
                "overdue",
                "--now",
                "2026-11-02T13:00+09:00",
                "--json",
            ],
        )
        .stdout
        .trim(),
    )
    .unwrap();
    assert_eq!(after_json.as_array().unwrap().len(), 1);

    // 組み直すと overdue から外れ、新しい時間枠が出る。
    let r = slot_run(
        &e,
        1,
        &[
            "add",
            "slot",
            "resched",
            "reader-bot-resume",
            "--start",
            "2026-11-04T10:00+09:00",
            "--end",
            "2026-11-04T12:00+09:00",
        ],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(slot_run(
        &e,
        1,
        &["slot", "overdue", "--now", "2026-11-03T00:00+09:00"]
    )
    .stdout
    .is_empty());
    assert!(slot_run(&e, 1, &["slot", "open"])
        .stdout
        .contains("2026-11-04T10:00+09:00"));

    // 済ませると未了から消え、終端のあとの出来事は止まる。
    let done = slot_run(&e, 3, &["add", "slot", "done", "reader-bot-resume"]);
    assert_eq!(done.code, 0, "{}{}", done.stdout, done.stderr);
    assert_eq!(slot_run(&e, 3, &["slot", "open"]).stdout, "");
    assert_eq!(slot_run(&e, 3, &["slot", "check"]).code, 0);
    for args in [
        &["add", "slot", "drop", "reader-bot-resume"][..],
        &["add", "slot", "done", "never-planned"][..],
    ] {
        let o = slot_run(&e, 3, args);
        assert_eq!(o.code, 2, "{args:?}: {}{}", o.stdout, o.stderr);
    }
}

#[test]
fn slot_rejects_what_the_grammar_forbids_and_check_reports_merged_lines() {
    let e = env();
    for args in [
        &["add", "slot", "pause", "k"][..],
        &["add", "slot", "plan", "k"][..],
        &[
            "add",
            "slot",
            "plan",
            "k",
            "--start",
            "2026-11-02T10:00",
            "--end",
            "2026-11-02T12:00",
        ][..],
        &[
            "add",
            "slot",
            "plan",
            "Bad_Key",
            "--start",
            "2026-11-02T10:00Z",
            "--end",
            "2026-11-02T12:00Z",
        ][..],
        &[
            "add",
            "slot",
            "plan",
            "k",
            "--start",
            "2026-11-02T12:00Z",
            "--end",
            "2026-11-02T10:00Z",
        ][..],
        &["add", "slot", "done", "k", "--start", "2026-11-02T10:00Z"][..],
        &["add", "slot", "done", "k", "--ref", "1"][..],
        &["slot", "overdue", "--now", "2026-11-02T10:00"][..],
        &["slot", "open", "--now", "2026-11-02T10:00Z"][..],
        &["slot", "frobnicate"][..],
    ] {
        let o = slot_run(&e, 0, args);
        assert_eq!(o.code, 2, "{args:?}: {}{}", o.stdout, o.stderr);
    }
    assert!(!e.vault.join("journal").exists());
    // 出来事の語彙は、止めるときの文言とヘルプに出る。
    let bad = slot_run(&e, 0, &["add", "slot", "pause", "k"]);
    assert!(
        bad.stderr.contains("plan|resched|done|drop"),
        "{}",
        bad.stderr
    );
    let help = slot_run(&e, 0, &["add"]);
    assert!(
        help.stderr.contains("worklog slot overdue"),
        "{}",
        help.stderr
    );

    // 合流した別の機体の行のように、状態から許されない行がノートにあると、check が食い違いに出す。
    let p = e.vault.join("journal/2026/2026-11-01.md");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "## 作業ログ\n- 09:00 [slot] plan: k [start:2026-11-02T10:00Z end:2026-11-02T12:00Z]\n- 09:01 [slot] done: k\n- 09:02 [slot] drop: k\n- 09:03 [slot] done: ghost\n- 09:04 [slot] メモ\n",
    )
    .unwrap();
    let c = slot_run(&e, 0, &["slot", "check"]);
    assert_eq!(c.code, 1, "{}{}", c.stdout, c.stderr);
    assert!(
        c.stdout.contains("食い違い\tk\t終端 done の後の drop"),
        "{}",
        c.stdout
    );
    assert!(
        c.stdout
            .contains("食い違い\tghost\t開いていない枠への done"),
        "{}",
        c.stdout
    );
    assert!(c.stdout.contains("解析できない行"), "{}", c.stdout);
    // 畳み込めた分は引ける。
    assert_eq!(slot_run(&e, 0, &["slot", "open"]).stdout, "");
    let listed = slot_run(
        &e,
        0,
        &[
            "list",
            "--kind",
            "slot",
            "--since",
            "2026-11-01",
            "--until",
            "2026-11-01",
        ],
    );
    assert_eq!(listed.stdout.lines().count(), 5, "{}", listed.stdout);
}
