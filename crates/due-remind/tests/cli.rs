//! main.rs のワイヤリング(ファイル探索・herdr 起動・once-a-day state)を実バイナリ
//! 越しに検査する。lib.rs 側の純粋関数は単体テストで検査済みなので、ここでは
//! 「本物の herdr の代わりに JSON を返す偽の herdr」を使った結合テストに絞る
//! (git-audit-worktrees の `--selftest` が bash スタブの `herdr` を使うのと同型)。
use std::fs;
use std::path::Path;
use std::process::Command;

fn write_fake_herdr(dir: &Path, response_json: &str, marker: &Path) -> std::path::PathBuf {
    let path = dir.join("herdr");
    let script = format!(
        "#!/bin/sh\necho called >> '{}'\ncat <<'EOF'\n{}\nEOF\n",
        marker.display(),
        response_json
    );
    fs::write(&path, script).unwrap();
    let mut perm = fs::metadata(&path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perm.set_mode(0o755);
    }
    fs::set_permissions(&path, perm).unwrap();
    path
}

fn run_due_remind(
    tmp: &Path,
    claude_dir: &Path,
    state_dir: &Path,
    today: &str,
    herdr_bin: &Path,
) -> std::process::Output {
    let _ = tmp; // 呼び出し側の tempdir を握っておく(生存期間だけの理由)
    Command::new(env!("CARGO_BIN_EXE_due-remind"))
        .env("DUE_REMIND_CLAUDE_DIR", claude_dir)
        .env("DUE_REMIND_STATE_DIR", state_dir)
        .env("DUE_REMIND_TODAY", today)
        .env("DUE_REMIND_HERDR_BIN", herdr_bin)
        .output()
        .expect("due-remind の実行に失敗")
}

fn write_index(claude_dir: &Path, domain: &str, repo_slug: &str, rows: &[&str]) {
    let dir = claude_dir.join(domain).join(repo_slug);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("due.jsonl"), rows.join("\n") + "\n").unwrap();
}

#[test]
fn nothing_due_does_not_call_herdr() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("claude");
    let state_dir = tmp.path().join("state");
    write_index(
        &claude_dir,
        "travel",
        "repo",
        &[r#"{"slug":"2026-09b","id":"filing","due":"2026-12-01"}"#], // ウィンドウ外(遠い将来)
    );
    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(tmp.path(), r#"{"result":{"shown":true}}"#, &marker);

    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(out.status.success());
    assert!(!marker.exists(), "herdr が呼ばれてしまった");
    assert!(!state_dir.join("last-shown").exists());
}

#[test]
fn due_within_window_calls_herdr_and_writes_state_on_shown() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("claude");
    let state_dir = tmp.path().join("state");
    write_index(
        &claude_dir,
        "travel",
        "repo",
        &[r#"{"slug":"2026-09b","id":"report","due":"2026-10-01"}"#],
    );
    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(
        tmp.path(),
        r#"{"result":{"shown":true,"reason":null}}"#,
        &marker,
    );

    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(out.status.success(), "{:?}", out);
    assert!(marker.exists(), "herdr が呼ばれていない");
    let stored = fs::read_to_string(state_dir.join("last-shown")).unwrap();
    assert_eq!(stored.trim(), "2026-09-28");
}

#[test]
fn already_shown_today_skips_herdr_on_second_run() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("claude");
    let state_dir = tmp.path().join("state");
    write_index(
        &claude_dir,
        "travel",
        "repo",
        &[r#"{"slug":"2026-09b","id":"report","due":"2026-10-01"}"#],
    );
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("last-shown"), "2026-09-28\n").unwrap();

    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(tmp.path(), r#"{"result":{"shown":true}}"#, &marker);
    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(out.status.success());
    assert!(!marker.exists(), "同日2回目なのに herdr が呼ばれた");
}

#[test]
fn transient_no_foreground_client_does_not_write_state() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("claude");
    let state_dir = tmp.path().join("state");
    write_index(
        &claude_dir,
        "travel",
        "repo",
        &[r#"{"slug":"2026-09b","id":"report","due":"2026-10-01"}"#],
    );
    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(
        tmp.path(),
        r#"{"result":{"shown":false,"reason":"no_foreground_client"}}"#,
        &marker,
    );

    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(out.status.success(), "一過性の理由は exit 0 のはず");
    assert!(marker.exists());
    assert!(!state_dir.join("last-shown").exists());
}

#[test]
fn disabled_delivery_exits_nonzero() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("claude");
    let state_dir = tmp.path().join("state");
    write_index(
        &claude_dir,
        "travel",
        "repo",
        &[r#"{"slug":"2026-09b","id":"report","due":"2026-10-01"}"#],
    );
    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(
        tmp.path(),
        r#"{"result":{"shown":false,"reason":"disabled"}}"#,
        &marker,
    );

    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(!out.status.success(), "disabled は exit 1 のはず");
    assert!(!state_dir.join("last-shown").exists());
}

#[test]
fn missing_claude_dir_is_nothing_due_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join("does-not-exist");
    let state_dir = tmp.path().join("state");
    let marker = tmp.path().join("called");
    let herdr = write_fake_herdr(tmp.path(), r#"{"result":{"shown":true}}"#, &marker);

    let out = run_due_remind(tmp.path(), &claude_dir, &state_dir, "2026-09-28", &herdr);
    assert!(out.status.success());
    assert!(!marker.exists());
}
