//! main.rs のワイヤリング(stdin 読み・ledger 読み書き・git toplevel 解決)を
//! 実バイナリ越しに検査する。純粋な述語判定(is_new_tool_unit 等)は lib.rs
//! の単体テストで検査済みなので、ここでは配線に絞る。

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("-c")
        .arg("core.hooksPath=")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn init_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(
        d.path(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    d
}

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_new-tool-guard"))
}

fn run_hook(repo: &Path, home: &Path, stdin_json: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(bin())
        .current_dir(repo)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_json.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn run_register(repo: &Path, home: &Path, line: &str) -> std::process::Output {
    Command::new(bin())
        .current_dir(repo)
        .env("HOME", home)
        .arg("register")
        .arg(line)
        .output()
        .unwrap()
}

fn write_stub(dir: &Path, rel: &str, content: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, content).unwrap();
    p
}

fn hook_stdin(cwd: &Path, file_path: &str, content: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": file_path, "content": content},
        "cwd": cwd.to_str().unwrap(),
    })
    .to_string()
}

#[test]
fn classify_reports_yes_for_shebang_and_no_for_plain_module() {
    let d = tempfile::tempdir().unwrap();
    let script = write_stub(d.path(), "scripts/foo.sh", "#!/usr/bin/env bash\necho hi\n");
    let module = write_stub(d.path(), "src/plain.rs", "pub fn x() {}\n");

    let out = Command::new(bin())
        .arg("classify")
        .arg(&script)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "yes");

    let out = Command::new(bin())
        .arg("classify")
        .arg(&module)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "no");
}

#[test]
fn hook_denies_new_unregistered_tool_unit() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = repo.path().join("scripts/new.sh");
    let stdin = hook_stdin(
        repo.path(),
        target.to_str().unwrap(),
        "#!/usr/bin/env bash\n",
    );

    let out = run_hook(repo.path(), home.path(), &stdin);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"permissionDecision\":\"deny\""),
        "{stdout}"
    );
    assert!(stdout.contains("既存手段の前倒し接地"), "{stdout}");
}

#[test]
fn hook_passes_after_register() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = repo.path().join("scripts/new.sh");
    let target_str = target.to_str().unwrap();

    let reg = run_register(
        repo.path(),
        home.path(),
        &format!("既存手段: {target_str} — 採用: jq"),
    );
    assert!(reg.status.success(), "{:?}", reg);

    let stdin = hook_stdin(repo.path(), target_str, "#!/usr/bin/env bash\n");
    let out = run_hook(repo.path(), home.path(), &stdin);
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn hook_ignores_existing_files() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = write_stub(repo.path(), "scripts/already.sh", "#!/usr/bin/env bash\n");

    let stdin = hook_stdin(
        repo.path(),
        target.to_str().unwrap(),
        "#!/usr/bin/env bash\n",
    );
    let out = run_hook(repo.path(), home.path(), &stdin);
    assert!(out.stdout.is_empty());
}

#[test]
fn hook_ignores_plain_modules() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = repo.path().join("src/plain.rs");
    let stdin = hook_stdin(repo.path(), target.to_str().unwrap(), "pub fn x() {}\n");
    let out = run_hook(repo.path(), home.path(), &stdin);
    assert!(out.stdout.is_empty());
}

#[test]
fn skip_switch_bypasses_hook() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = repo.path().join("scripts/new.sh");
    let stdin = hook_stdin(
        repo.path(),
        target.to_str().unwrap(),
        "#!/usr/bin/env bash\n",
    );

    use std::io::Write;
    let mut child = Command::new(bin())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("SKIP_NEW_TOOL_GUARD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.stdout.is_empty());
}

#[test]
fn register_rejects_invalid_grammar() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let out = run_register(repo.path(), home.path(), "既存手段: p — 自前");
    assert!(!out.status.success());
}
