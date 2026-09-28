//! main.rs のワイヤリング(gh/wrapup-stop-gate.sh の呼び出し・ファイル探索)
//! を、偽の `gh`/`wrapup-stop-gate.sh` を使った結合テストで検査する
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
/// ログファイルに1行追記する偽 `wrapup-stop-gate.sh`。
fn write_fake_wrapup(path: &Path, log: &Path) {
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --check-dup) exit 0 ;;\n  --add) echo \"$2 $3\" >> '{}' ;;\nesac\n",
        log.display()
    );
    write_exec(path, &script);
}

fn run(env: &[(&str, &Path)], extra_env: &[(&str, &str)], dry_run: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_promotion-detect"));
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
    let wrapup_bin = tmp.path().join("wrapup-stop-gate.sh");
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
    let wrapup_bin = tmp.path().join("wrapup-stop-gate.sh");
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
