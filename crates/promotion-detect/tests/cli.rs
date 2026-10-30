//! main.rs のワイヤリング(gh/wrapup-stop-gate の呼び出し・ファイル探索)
//! を、偽の `gh`/`wrapup-stop-gate` を使った結合テストで検査する
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
/// ログファイルに1行追記する偽 `wrapup-stop-gate`。
fn write_fake_wrapup(path: &Path, log: &Path) {
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --check-dup) exit 0 ;;\n  --add) echo \"$2 $3\" >> '{}' ;;\nesac\n",
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
    let wrapup_bin = tmp.path().join("wrapup-stop-gate");
    write_fake_wrapup(&wrapup_bin, &wrapup_log);

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
            ("PROMOTION_DETECT_WRAPUP_BIN", &wrapup_bin),
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
    let wrapup_bin = tmp.path().join("wrapup-stop-gate");
    write_fake_wrapup(&wrapup_bin, &wrapup_log);

    let out = run(
        &[
            ("PROMOTION_DETECT_GH_BIN", &tmp.path().join("gh")),
            ("PROMOTION_DETECT_CLAUDE_DIR", &claude_dir),
            ("PROMOTION_DETECT_STATE_DIR", &state_dir),
            ("PROMOTION_DETECT_WRAPUP_BIN", &wrapup_bin),
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
            &tmp.path().join("wrapup-stop-gate"),
            &tmp.path().join("wrapup.log"),
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
                ("PROMOTION_DETECT_WRAPUP_BIN", &self.p("wrapup-stop-gate")),
                ("PROMOTION_DETECT_INBOX_PATH", &self.p("inbox.jsonl")),
            ],
            extra,
            dry_run,
        )
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
        &w.p("wrapup-stop-gate"),
        "#!/bin/sh\ncase \"$1\" in\n  --check-dup) exit 0 ;;\n  --add) exit 1 ;;\nesac\n",
    );
    w.run(&[], false);
    // 記録されていないので、直った翌日に入れ直せる。
    write_fake_wrapup(&w.p("wrapup-stop-gate"), &w.p("wrapup.log"));
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
