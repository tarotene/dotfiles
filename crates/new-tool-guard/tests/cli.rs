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
        // gate_event::default_path() は XDG_STATE_HOME を $HOME より優先する
        // ため、テストの外側(この開発機の実環境)にある実際の
        // XDG_STATE_HOME をここで確実に見えなくする(そうしないと
        // gate-events.jsonl がテスト用の tempdir ではなく実環境に書かれる)。
        .env_remove("XDG_STATE_HOME")
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
        .ok();
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

    // ADR-543 段3: deny 時に gate-events.jsonl へ記録される。
    let events = fs::read_to_string(home.path().join(".local/state/claude/gate-events.jsonl"))
        .expect("gate-events.jsonl が書かれているはず");
    assert!(events.contains("\"gate\":\"new-tool-guard\""), "{events}");
    assert!(events.contains("\"decision\":\"deny\""), "{events}");
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
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).ok();
    let out = child.wait_with_output().unwrap();
    assert!(out.stdout.is_empty());

    // ADR-543 段3: skip 時にも gate-events.jsonl へ記録される(降格候補検出
    // の入力 — skip の多用そのものが「見直し候補」の兆候になる)。
    let events = fs::read_to_string(home.path().join(".local/state/claude/gate-events.jsonl"))
        .expect("gate-events.jsonl が書かれているはず");
    assert!(events.contains("\"decision\":\"skip\""), "{events}");
}

#[test]
fn register_rejects_invalid_grammar() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let out = run_register(repo.path(), home.path(), "既存手段: p — 自前");
    assert!(!out.status.success());
}

/// `run_hook` に環境変数を足せる版。
fn run_hook_env(
    repo: &Path,
    home: &Path,
    stdin_json: &str,
    env: &[(&str, &Path)],
) -> std::process::Output {
    use std::io::Write;
    let mut cmd = Command::new(bin());
    cmd.current_dir(repo)
        .env("HOME", home)
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_json.as_bytes())
        .ok();
    child.wait_with_output().unwrap()
}

/// #661: 並列 worktree のサブエージェントでは `CLAUDE_PROJECT_DIR` が親の
/// リポジトリを指す。register は worktree の cwd から行うので、hook が
/// `CLAUDE_PROJECT_DIR` をキーにすると別の ledger を見て、register 済みの
/// Write を拒否してしまう。書き込み先の toplevel をキーにしていれば通る。
#[test]
fn hook_passes_after_register_in_parallel_worktree_with_parent_project_dir() {
    let parent = init_repo();
    let home = tempfile::tempdir().unwrap();
    let wt_base = tempfile::tempdir().unwrap();
    let wt = wt_base.path().join("agent-1");
    git(
        parent.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent-1",
            wt.to_str().unwrap(),
        ],
    );
    let wt = wt.canonicalize().unwrap();

    // 新しい crate の Cargo.toml(親ではなく worktree 側へ書く)。
    let target = wt.join("crates/new-crate/Cargo.toml");
    let target_str = target.to_str().unwrap();

    // worktree の cwd から、絶対パスで register する。
    let reg = run_register(
        &wt,
        home.path(),
        &format!("既存手段: {target_str} — 採用: cargo"),
    );
    assert!(reg.status.success(), "{:?}", reg);

    let stdin = hook_stdin(&wt, target_str, "[package]\nname = \"new-crate\"\n");
    let out = run_hook_env(
        &wt,
        home.path(),
        &stdin,
        &[("CLAUDE_PROJECT_DIR", parent.path())],
    );
    assert!(
        out.stdout.is_empty(),
        "register 済みなのに deny された: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// #661: 相対パスで register しても、絶対パスの Write と一致する。deny 文の
/// 例も repo 相対で出る(pr-gate.sh の G_prior と同じ書式)。
#[test]
fn hook_matches_relative_register_and_shows_relative_path_in_deny() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let repo_path = repo.path().canonicalize().unwrap();
    let target = repo_path.join("scripts/new.sh");

    // 未登録 → deny、メッセージの例は repo 相対。
    let stdin = hook_stdin(
        &repo_path,
        target.to_str().unwrap(),
        "#!/usr/bin/env bash\n",
    );
    let out = run_hook(&repo_path, home.path(), &stdin);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("deny"), "{stdout}");
    assert!(
        stdout.contains("register '既存手段: scripts/new.sh —"),
        "{stdout}"
    );
    assert!(!stdout.contains(repo_path.to_str().unwrap()), "{stdout}");

    // 相対パスで register → 絶対パスの Write が通る。
    let reg = run_register(
        &repo_path,
        home.path(),
        "既存手段: scripts/new.sh — 採用: jq",
    );
    assert!(reg.status.success(), "{:?}", reg);
    let out = run_hook(&repo_path, home.path(), &stdin);
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// --- ExitPlanMode の承認後に、計画の `既存手段:` 行を台帳へ転記する(PostToolUse) ---
//
// 計画の `## 先行例との対比` の `既存手段:` 行は、承認時に plan-precedent-gate が読むのに、
// 台帳には登録されず、新規 Cargo.toml の Write が deny されて、承認済みの行を手で
// `register` し直す二重入力があった。台帳を計画の派生にする。

fn plan_stdin(cwd: &Path, plan: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "ExitPlanMode",
        "tool_input": {"plan": plan},
        "cwd": cwd.to_str().unwrap(),
    })
    .to_string()
}

fn ledger_lines(home: &Path) -> Vec<String> {
    let dir = home.join(".claude/new-tool-guard/state");
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .flat_map(|e| {
            fs::read_to_string(e.path())
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn denied(out: &std::process::Output) -> bool {
    String::from_utf8_lossy(&out.stdout).contains("\"permissionDecision\":\"deny\"")
}

#[test]
fn approved_plan_registers_its_existing_means_lines() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let target = "crates/new-thing/Cargo.toml";
    let write = hook_stdin(
        repo.path(),
        repo.path().join(target).to_str().unwrap(),
        "[package]\n",
    );
    assert!(
        denied(&run_hook(repo.path(), home.path(), &write)),
        "対照: 登録が無ければ新規の Cargo.toml は deny される"
    );

    let plan = format!(
        "# 計画\n\n## 先行例との対比\n\n- D1: 新しい crate を足す\n  本命: なし — 理由\n  既存手段: {target} — 拡張: crates/herdr-status-advisory(型)\n  軸: 還元 — 句\n\n## 検証\n- テスト\n"
    );
    let out = run_hook(repo.path(), home.path(), &plan_stdin(repo.path(), &plan));
    assert!(out.stdout.is_empty(), "記録は無出力");
    assert_eq!(
        ledger_lines(home.path()),
        vec![format!(
            "既存手段: {target} — 拡張: crates/herdr-status-advisory(型)"
        )]
    );

    assert!(
        !denied(&run_hook(repo.path(), home.path(), &write)),
        "計画から転記された行で、手で register しなくても通る"
    );
}

#[test]
fn plan_transcription_skips_invalid_lines_other_sections_and_duplicates() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    let plan = "## 先行例との対比\n\n\
- D1: a\n  既存手段: scripts/a.sh — 採用: jq\n\
- D2: b\n  既存手段: scripts/b.sh — 自前\n\
- D3: c\n  既存手段: scripts/a.sh — 採用: jq\n\
- 既存手段: scripts/c.sh — 拡張: scripts/a.sh\n\n\
## 設計メモ\n  既存手段: scripts/d.sh — 採用: jq\n";
    let out = run_hook(repo.path(), home.path(), &plan_stdin(repo.path(), plan));
    assert!(out.stdout.is_empty());
    assert_eq!(
        ledger_lines(home.path()),
        vec![
            "既存手段: scripts/a.sh — 採用: jq".to_string(),
            "既存手段: scripts/c.sh — 拡張: scripts/a.sh".to_string(),
        ],
        "書式が不正な行(自前の理由なし)・別の節の行・同じ行の重複は載せない"
    );

    // 同じ計画をもう一度承認しても、台帳は増えない。
    run_hook(repo.path(), home.path(), &plan_stdin(repo.path(), plan));
    assert_eq!(ledger_lines(home.path()).len(), 2);
}

#[test]
fn plan_without_the_section_or_other_tools_record_nothing() {
    let repo = init_repo();
    let home = tempfile::tempdir().unwrap();
    run_hook(
        repo.path(),
        home.path(),
        &plan_stdin(repo.path(), "# 計画\n\n既存手段: scripts/x.sh — 採用: jq\n"),
    );
    assert!(ledger_lines(home.path()).is_empty(), "節が無い計画は対象外");

    let other = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "true", "plan": "## 先行例との対比\n既存手段: scripts/x.sh — 採用: jq\n"},
        "cwd": repo.path().to_str().unwrap(),
    })
    .to_string();
    run_hook(repo.path(), home.path(), &other);
    assert!(
        ledger_lines(home.path()).is_empty(),
        "ExitPlanMode 以外は対象外"
    );
}

#[test]
fn plan_transcription_outside_a_git_worktree_is_silent() {
    let not_git = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let plan = "## 先行例との対比\n  既存手段: scripts/x.sh — 採用: jq\n";
    let out = run_hook(
        not_git.path(),
        home.path(),
        &plan_stdin(not_git.path(), plan),
    );
    assert!(out.status.success() && out.stdout.is_empty());
    assert!(ledger_lines(home.path()).is_empty());
}
