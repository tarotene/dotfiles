//! main.rs のワイヤリング(gh/issue-inbox の呼び出し・ファイル探索)
//! を、偽の `gh`/`issue-inbox` を使った結合テストで検査する
//! (`due-remind`/`git-audit-worktrees` の「本物の代わりに固定応答を返す
//! スタブ」と同型)。純粋な検出ロジックは lib.rs の単体テストで検査済み。

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn write_exec(path: &Path, script: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(path).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(path, perm).unwrap();
    }
}

/// $1 $2 $3... をそのまま1行に追記する `gh` スタブ(issue list の応答は
/// 固定 JSON)。
fn write_fake_gh(dir: &Path, issues_json: &str) {
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  *'issue list'*) cat <<'EOF'\n{issues_json}\nEOF\n  ;;\n  *) exit 1 ;;\nesac\n"
    );
    write_exec(&dir.join("gh"), &script);
}

/// `--check-dup` は常に非重複(exit 0)を返し、`--add` は呼び出し引数を
/// ログファイルに1行追記する偽 `issue-inbox`。
fn write_fake_wrapup(path: &Path, log: &Path) {
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --check-dup) exit 0 ;;\n  --add) echo \"$2 $3\" >> '{}' ;;\nesac\n",
        log.display()
    );
    write_exec(path, &script);
}

/// 偽の `worklog`。呼ばれた引数を 1 行ログに残し、決めた標準出力・標準エラー・終了コードを返す。
fn write_fake_worklog(path: &Path, log: &Path, code: i32, stdout: &str, stderr: &str) {
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\ncat <<'EOF'\n{stdout}EOF\ncat >&2 <<'EOF'\n{stderr}EOF\nexit {code}\n",
        log.display()
    );
    write_exec(path, &script);
}

fn run(env: &[(&str, &Path)], extra_env: &[(&str, &str)], dry_run: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_promotion-detect"));
    isolate(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.output().unwrap()
}

/// 実機の vault を読まない・書かない: vault の宣言(環境変数とマーカー)は、テストが明示したときだけ。
fn isolate(cmd: &mut Command) {
    cmd.env("HOME", "/nonexistent-home")
        .env_remove("OBSIDIAN_VAULT")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME");
}

#[test]
fn dry_run_reports_recurrence_candidate_without_calling_wrapup() {
    let tmp = tempfile::tempdir().unwrap();
    let issues = serde_json::json!([
        {"number": 10, "body": "Target: skill/precedent-grounding\n本文"},
        {"number": 20, "body": "Target: skill/precedent-grounding\n本文2"},
    ])
    .to_string();
    write_fake_gh(tmp.path(), &issues);

    let claude_dir = tmp.path().join("claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let state_dir = tmp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    let inbox = tmp.path().join("inbox.jsonl");
    let wrapup_log = tmp.path().join("wrapup.log");
    let wrapup_bin = tmp.path().join("issue-inbox");
    write_fake_wrapup(&wrapup_bin, &wrapup_log);

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
            ("PROMOTION_DETECT_INBOX_BIN", &wrapup_bin),
            ("PROMOTION_DETECT_INBOX_PATH", &inbox),
        ],
        &[],
        true,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("skill/precedent-grounding"), "{stdout}");
    assert!(!wrapup_log.exists(), "dry-run は wrapup を呼ばない");
}

#[test]
fn writes_to_inbox_via_wrapup_when_not_dry_run() {
    let tmp = tempfile::tempdir().unwrap();
    let issues = serde_json::json!([
        {"number": 10, "body": "Target: hook/foo-gate\n本文"},
        {"number": 20, "body": "Target: hook/foo-gate\n本文2"},
    ])
    .to_string();
    write_fake_gh(tmp.path(), &issues);

    let claude_dir = tmp.path().join("claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let state_dir = tmp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    let inbox = tmp.path().join("inbox.jsonl");
    let wrapup_log = tmp.path().join("wrapup.log");
    let wrapup_bin = tmp.path().join("issue-inbox");
    write_fake_wrapup(&wrapup_bin, &wrapup_log);

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
            ("PROMOTION_DETECT_INBOX_BIN", &wrapup_bin),
            ("PROMOTION_DETECT_INBOX_PATH", &inbox),
        ],
        &[],
        false,
    );
    assert!(out.stdout.is_empty());
    let log = fs::read_to_string(&wrapup_log).unwrap();
    assert!(log.contains(inbox.to_str().unwrap()), "{log}");
}

#[test]
fn no_feedback_recurrence_produces_no_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    write_fake_gh(tmp.path(), "[]");

    let claude_dir = tmp.path().join("claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let state_dir = tmp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
        ],
        &[],
        true,
    );
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn stale_skip_file_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    write_fake_gh(tmp.path(), "[]");

    let claude_dir = tmp.path().join("claude");
    fs::create_dir_all(claude_dir.join("pr-gate")).unwrap();
    let skip = claude_dir.join("pr-gate/skip");
    fs::write(&skip, "").unwrap();
    // mtime を35日前に見せかける。
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(35 * 86400);
    let old_ft = filetime::FileTime::from_system_time(old);
    filetime::set_file_mtime(&skip, old_ft).unwrap();

    let state_dir = tmp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
        ],
        &[],
        true,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pr-gate"), "{stdout}");
    assert!(stdout.contains("35"), "{stdout}");
}

/// 偽の gh・wrapup・状態ディレクトリを用意して、`env` と一緒に返す。
struct World {
    tmp: tempfile::TempDir,
}

impl World {
    fn new(issues: &str) -> World {
        let tmp = tempfile::tempdir().unwrap();
        write_fake_gh(tmp.path(), issues);
        fs::create_dir_all(tmp.path().join("claude")).unwrap();
        fs::create_dir_all(tmp.path().join("state")).unwrap();
        write_fake_wrapup(
            &tmp.path().join("issue-inbox"),
            &tmp.path().join("wrapup.log"),
        );
        // 既定の worklog は、整合している(終了コード 0)。呼ばれた引数は worklog.log に残る。
        write_fake_worklog(
            &tmp.path().join("worklog"),
            &tmp.path().join("worklog.log"),
            0,
            "",
            "",
        );
        World { tmp }
    }
    fn p(&self, name: &str) -> std::path::PathBuf {
        self.tmp.path().join(name)
    }
    fn run(&self, extra: &[(&str, &str)], dry_run: bool) -> std::process::Output {
        run(
            &[
                ("PROMOTION_DETECT_GH_BIN", &self.p("gh")),
                ("PROMOTION_DETECT_CLAUDE_DIR", &self.p("claude")),
                ("PROMOTION_DETECT_STATE_DIR", &self.p("state")),
                ("PROMOTION_DETECT_INBOX_BIN", &self.p("issue-inbox")),
                ("PROMOTION_DETECT_INBOX_PATH", &self.p("inbox.jsonl")),
                ("PROMOTION_DETECT_WORKLOG_BIN", &self.p("worklog")),
            ],
            extra,
            dry_run,
        )
    }
    fn worklog_calls(&self) -> String {
        fs::read_to_string(self.p("worklog.log")).unwrap_or_default()
    }
    fn add_log(&self) -> String {
        fs::read_to_string(self.p("wrapup.log")).unwrap_or_default()
    }
}

fn recurrence_issues() -> String {
    serde_json::json!([
        {"number": 10, "body": "Target: hook/foo-gate\n本文"},
        {"number": 20, "body": "Target: hook/foo-gate\n本文2"},
    ])
    .to_string()
}

#[test]
fn the_added_line_carries_the_repo_so_a_timer_in_home_does_not_misfile_it() {
    let w = World::new(&recurrence_issues());
    w.run(&[], false);
    let log = w.add_log();
    assert!(log.contains(r#""repo":"tarotene/dotfiles""#), "{log}");
}

#[test]
fn the_same_candidate_is_not_added_again_on_the_next_day() {
    let w = World::new(&recurrence_issues());
    w.run(&[], false);
    assert_eq!(w.add_log().lines().count(), 1, "{}", w.add_log());
    // 翌日: 題も本文も同じ → ノートに行を足さない(再発の回数が膨らまない)。
    w.run(&[], false);
    assert_eq!(w.add_log().lines().count(), 1, "{}", w.add_log());
    assert!(w.p("state/promotion-detect/emitted.json").is_file());
}

#[test]
fn a_candidate_whose_add_failed_is_tried_again() {
    let w = World::new(&recurrence_issues());
    // --add が失敗する wrapup。
    write_exec(
        &w.p("issue-inbox"),
        "#!/bin/sh\ncase \"$1\" in\n  --check-dup) exit 0 ;;\n  --add) exit 1 ;;\nesac\n",
    );
    w.run(&[], false);
    // 記録されていないので、直った翌日に入れ直せる。
    write_fake_wrapup(&w.p("issue-inbox"), &w.p("wrapup.log"));
    w.run(&[], false);
    assert_eq!(w.add_log().lines().count(), 1, "{}", w.add_log());
}

#[test]
fn a_dry_run_neither_reads_nor_writes_the_dedup_state() {
    let w = World::new(&recurrence_issues());
    let first = w.run(&[], true);
    let second = w.run(&[], true);
    assert_eq!(first.stdout, second.stdout);
    assert!(!String::from_utf8_lossy(&first.stdout).is_empty());
    assert!(!w.p("state/promotion-detect/emitted.json").exists());
}

/// vault に as-code 化の候補のノートを置く(frontmatter は issue-inbox の書式)。
fn candidate_note(vault: &Path, name: &str, status: &str, reason: &str) {
    fs::create_dir_all(vault.join("inbox")).unwrap();
    fs::write(
        vault.join("inbox").join(format!("{name}.md")),
        format!(
            "---\ntitle: \"{name}\"\nrepo: \"o/r\"\nstatus: \"{status}\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"2026-11-01\"\nsignal: \"form-check\"\nmechanism: \"gate\"\nexisting: \"拡張:x\"\ndrop_reason: \"{reason}\"\n---\n- 2026-11-01 d\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_signal_dropped_again_and_again_is_flagged_from_the_vault() {
    let w = World::new("[]");
    let vault = w.p("vault");
    fs::create_dir_all(vault.join(".obsidian")).unwrap();
    fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    for n in ["a", "b", "c"] {
        candidate_note(&vault, n, "dropped", "one-off");
    }
    let vault_str = vault.to_str().unwrap().to_string();
    let out = w.run(&[("OBSIDIAN_VAULT", &vault_str)], true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("form-check の定義を直す候補"), "{stdout}");
    assert!(stdout.contains("one-off×3"), "{stdout}");
    assert!(stdout.contains(r#""repo":"tarotene/dotfiles""#), "{stdout}");
    // vault が無ければ、較正は何も出さない。
    let out = w.run(&[], true);
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// vault を宣言した機体(`OBSIDIAN_VAULT`)にして、その値を返す。
fn declare_vault(w: &World) -> String {
    let vault = w.p("vault");
    fs::create_dir_all(vault.join(".obsidian")).unwrap();
    fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    vault.to_str().unwrap().to_string()
}

#[test]
fn a_mail_discrepancy_in_the_vault_is_flagged() {
    let w = World::new("[]");
    write_fake_worklog(
        &w.p("worklog"),
        &w.p("worklog.log"),
        1,
        "食い違い\tgmail:t1/r1\t遷移が語彙に無い\n食い違い\tgmail:t2/r2\t終端の行が消えた(控え: closed / 今: -)\n",
        "",
    );
    let vault = declare_vault(&w);
    let out = w.run(&[("OBSIDIAN_VAULT", &vault)], true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("食い違いが2件"), "{stdout}");
    assert!(stdout.contains("gmail:t1/r1"), "{stdout}");
    assert!(stdout.contains(r#""repo":"tarotene/dotfiles""#), "{stdout}");
    assert_eq!(w.worklog_calls().trim(), "mail check");
}

#[test]
fn a_consistent_mail_state_adds_nothing() {
    let w = World::new("[]");
    let vault = declare_vault(&w);
    let out = w.run(&[("OBSIDIAN_VAULT", &vault)], true);
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        w.worklog_calls().trim(),
        "mail check",
        "整合していても検査は回す"
    );
}

#[test]
fn a_machine_without_a_vault_never_runs_the_mail_check() {
    let w = World::new("[]");
    write_fake_worklog(
        &w.p("worklog"),
        &w.p("worklog.log"),
        1,
        "食い違い\tgmail:t1/r1\tm\n",
        "",
    );
    let out = w.run(&[], true);
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        w.worklog_calls().is_empty(),
        "vault が無い機体で worklog を呼んだ"
    );
}

#[test]
fn a_failing_mail_check_is_flagged_instead_of_skipped() {
    // 検査が回っていないことを、黙って飛ばさない(終了コード 2 = worklog が読めなかった)。
    let w = World::new("[]");
    write_fake_worklog(
        &w.p("worklog"),
        &w.p("worklog.log"),
        2,
        "",
        "worklog: vault を読めない\n二行目は載せない\n",
    );
    let vault = declare_vault(&w);
    let out = w.run(&[("OBSIDIAN_VAULT", &vault)], true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("整合を読めませんでした"), "{stdout}");
    assert!(
        stdout.contains("終了コード 2: worklog: vault を読めない"),
        "{stdout}"
    );
    assert!(!stdout.contains("二行目"), "{stdout}");
}

#[test]
fn a_missing_worklog_binary_is_flagged_on_a_vault_machine() {
    let w = World::new("[]");
    fs::remove_file(w.p("worklog")).unwrap();
    let vault = declare_vault(&w);
    let out = w.run(&[("OBSIDIAN_VAULT", &vault)], true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("整合を読めませんでした"), "{stdout}");
}

#[test]
fn an_unchanged_discrepancy_is_not_added_again_on_the_next_day() {
    let w = World::new("[]");
    write_fake_worklog(
        &w.p("worklog"),
        &w.p("worklog.log"),
        1,
        "食い違い\tgmail:t1/r1\t遷移が語彙に無い\n",
        "",
    );
    let vault = declare_vault(&w);
    w.run(&[("OBSIDIAN_VAULT", &vault)], false);
    let first = w.add_log();
    assert!(first.contains("食い違いが1件"), "{first}");
    w.run(&[("OBSIDIAN_VAULT", &vault)], false);
    assert_eq!(w.add_log(), first, "同じ食い違いを翌日も入れ直した");
}

// ---- 気づきの失効(ADR-1381): open のまま 14 日放置された気づきを lapsed にする ----

fn days_ago(n: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    issue_inbox::vault::jst_date(now - n * 86_400)
}

/// vault に、状態と最後の行の日付を決めた気づきを置く(frontmatter は issue-inbox の書式)。
fn finding(vault: &Path, name: &str, title: &str, repo: &str, status: &str, last: &str) {
    fs::create_dir_all(vault.join("inbox")).unwrap();
    fs::write(
        vault.join("inbox").join(format!("{name}.md")),
        format!(
            "---\ntitle: \"{title}\"\nrepo: \"{repo}\"\nstatus: \"{status}\"\nissue: \"\"\npr: \"\"\nfirst_seen: \"{last}\"\n---\n- {last} d\n"
        ),
    )
    .unwrap();
}

fn status_of(vault: &Path, name: &str) -> String {
    let text = fs::read_to_string(vault.join("inbox").join(format!("{name}.md"))).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix("status: "))
        .unwrap()
        .trim_matches('"')
        .to_string()
}

#[test]
fn open_findings_of_every_repo_lapse_at_14_days_and_the_rest_are_left() {
    let w = World::new("[]");
    let vault_str = declare_vault(&w);
    let vault = w.p("vault");
    finding(&vault, "a", "a", "o/r", "open", &days_ago(14));
    finding(&vault, "b", "b", "o/other", "open", &days_ago(40));
    finding(&vault, "c", "c", "o/r", "open", &days_ago(13));
    finding(&vault, "d", "d", "o/r", "fixed", &days_ago(40));
    let out = w.run(&[("OBSIDIAN_VAULT", &vault_str)], false);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("気づきの失効: 2 件"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let statuses: Vec<_> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| status_of(&vault, n))
        .collect();
    assert_eq!(statuses, ["lapsed", "lapsed", "open", "fixed"]);
    // 翌日も同じ状態で動いても、二重に書かない。
    let before = fs::read(vault.join("inbox/a.md")).unwrap();
    let again = w.run(&[("OBSIDIAN_VAULT", &vault_str)], false);
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("気づきの失効: 0 件"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(fs::read(vault.join("inbox/a.md")).unwrap(), before);
}

#[test]
fn a_dry_run_counts_the_lapsing_findings_without_writing() {
    let w = World::new("[]");
    let vault_str = declare_vault(&w);
    let vault = w.p("vault");
    finding(&vault, "a", "a", "o/r", "open", &days_ago(30));
    let out = w.run(&[("OBSIDIAN_VAULT", &vault_str)], true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("気づきの失効: 1 件(dry-run"), "{stderr}");
    assert!(out.stdout.is_empty());
    assert_eq!(status_of(&vault, "a"), "open");
}

#[test]
fn a_machine_without_a_vault_sweeps_nothing() {
    let w = World::new("[]");
    let out = w.run(&[], false);
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("気づきの失効"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_lapsed_candidate_that_is_still_detected_is_added_again() {
    // 検出される限り続く候補は、「前回と同じ」のまま二度と入らず 14 日で黙って消えないよう、
    // 失効したら記録(emitted.json)から外して入れ直す(再発として open に戻る)。
    let w = World::new(&recurrence_issues());
    let vault_str = declare_vault(&w);
    w.run(&[("OBSIDIAN_VAULT", &vault_str)], false);
    assert_eq!(w.add_log().lines().count(), 1, "{}", w.add_log());
    // 翌日: 変化が無ければ入れ直さない。
    w.run(&[("OBSIDIAN_VAULT", &vault_str)], false);
    assert_eq!(w.add_log().lines().count(), 1, "{}", w.add_log());
    // 同じ題の気づきが、open のまま 20 日放置されている。
    let title = "同じ Target: hook/foo-gate を持つ feedback Issue が2件あります(昇格候補、ADR-543)";
    finding(
        &w.p("vault"),
        "cand",
        title,
        "tarotene/dotfiles",
        "open",
        &days_ago(20),
    );
    w.run(&[("OBSIDIAN_VAULT", &vault_str)], false);
    assert_eq!(status_of(&w.p("vault"), "cand"), "lapsed");
    assert_eq!(w.add_log().lines().count(), 2, "{}", w.add_log());
}

// ---- 記録の層(#1310): 宣言のあるチェックアウトだけを読む ----

fn git_in(dir: &Path, envs: &[(&str, &str)], args: &[&str]) {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    assert!(cmd.output().unwrap().status.success(), "git {args:?}");
}

/// `<ghr>/github.com/o/r` に git リポジトリを作り、宣言(あれば)と `tracked` のファイルを書いてコミットする。
fn declared_checkout(w: &World, declaration: Option<&str>, tracked: &[&str]) -> std::path::PathBuf {
    let repo = w.p("ghr/github.com/o/r");
    fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &[], &["init", "-q", "-b", "main"]);
    if let Some(d) = declaration {
        fs::create_dir_all(repo.join(".config")).unwrap();
        fs::write(repo.join(".config/record-layers.toml"), d).unwrap();
    }
    for f in tracked {
        fs::create_dir_all(repo.join(f).parent().unwrap()).unwrap();
        fs::write(repo.join(f), "x").unwrap();
    }
    git_in(&repo, &[], &["add", "-A"]);
    git_in(&repo, &[], &["commit", "-q", "-m", "init"]);
    repo
}

fn run_with_ghr(w: &World) -> String {
    let out = w.run(
        &[("PROMOTION_DETECT_GHR_DIR", w.p("ghr").to_str().unwrap())],
        true,
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn a_declaration_that_leaves_a_tracked_file_uncovered_is_reported() {
    let w = World::new("[]");
    declared_checkout(
        &w,
        Some("[[path]]\nglob = \".config/*\"\nlayer = \"L1\"\n[[path]]\nglob = \"docs/**\"\nlayer = \"L1\"\n"),
        &["docs/a.md", "src/b.rs"],
    );
    let stdout = run_with_ghr(&w);
    assert!(
        stdout.contains("記録の層の宣言に欠陥があります(o/r"),
        "{stdout}"
    );
    assert!(
        stdout.contains("src/b.rs: どの glob にも当たらない"),
        "{stdout}"
    );
}

#[test]
fn a_broken_declaration_is_reported_not_skipped() {
    let w = World::new("[]");
    declared_checkout(&w, Some("[[path]]\nglob = \"a\"\nlayer = \"L9\"\n"), &["a"]);
    let stdout = run_with_ghr(&w);
    assert!(
        stdout.contains("記録の層の宣言に欠陥があります"),
        "{stdout}"
    );
}

#[test]
fn an_l1_file_rewritten_on_five_days_is_reported_as_drift() {
    let w = World::new("[]");
    let repo = declared_checkout(
        &w,
        Some("[[path]]\nglob = \"**\"\nlayer = \"L1\"\n"),
        &["hot.md", "cold.md"],
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for i in 0..5u64 {
        fs::write(repo.join("hot.md"), format!("v{i}")).unwrap();
        let when = format!("{} +0000", now - i * 86_400);
        git_in(
            &repo,
            &[("GIT_COMMITTER_DATE", &when), ("GIT_AUTHOR_DATE", &when)],
            &["commit", "-q", "-a", "-m", "touch"],
        );
    }
    let stdout = run_with_ghr(&w);
    assert!(
        stdout.contains("実測の変化の速さがずれています(o/r)"),
        "{stdout}"
    );
    assert!(stdout.contains("hot.md(L1)"), "{stdout}");
    assert!(!stdout.contains("cold.md(L1)"), "{stdout}");
}

#[test]
fn a_checkout_without_a_declaration_is_not_read() {
    let w = World::new("[]");
    declared_checkout(&w, None, &["src/b.rs"]);
    let stdout = run_with_ghr(&w);
    assert!(!stdout.contains("記録の層"), "{stdout}");
}
