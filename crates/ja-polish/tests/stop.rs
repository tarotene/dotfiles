//! 文書ファイルの関所(Stop)と、基点のスナップショットの端から端まで。
//! 環境変数を書き換えるので、1 つのテスト関数の中で順に確かめる(統合テストは別プロセス)。

use agent_session::{Agent, HookEvent, Reply, StopInput};
use ja_polish::{blocks, records, snapshot, stop};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

const OLD: &str = "承認済みの翻訳調な段落です。";
const NEW: &str = "足した段落の文章を直すことができる。";

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn stop_input(cwd: &Path, active: bool) -> String {
    json!({"session_id": "sess-1", "cwd": cwd.to_string_lossy(), "stop_hook_active": active})
        .to_string()
}

/// 入口(`run_stop`)と同じ手順: 再入の宣言を適用し、検査して、Claude の形に描く。
fn run(raw: &str) -> Option<String> {
    let input = StopInput::new(raw);
    if !input.permits(stop::REENTRY) {
        return None;
    }
    let out = Reply::from_stop(stop::hook(&input).expect("no fault"))
        .render(Agent::Claude, HookEvent::Stop);
    (!out.is_empty()).then_some(out)
}

fn blocked(out: Option<String>) -> Option<Value> {
    let v: Value = serde_json::from_str(&out?).ok()?;
    (v["decision"] == "block").then_some(v)
}

fn pre_write(tool: &str, file: &Path) -> String {
    json!({
        "tool_name": tool,
        "session_id": "sess-1",
        "tool_input": {"file_path": file.to_string_lossy()}
    })
    .to_string()
}

#[test]
fn stop_gate_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
    std::env::set_var("XDG_STATE_HOME", root.join("state"));
    std::env::remove_var("SKIP_JA_POLISH_GATE");

    // git のリポジトリ: 基点に承認済みの段落、作業ツリーに足した段落。
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "t"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("doc.md"), format!("{OLD}\n")).unwrap();
    git(&repo, &["add", "doc.md"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(repo.join("doc.md"), format!("{OLD}\n\n{NEW}\n")).unwrap();
    std::fs::write(
        repo.join("code.rs"),
        "// 日本語のコメントです。コードは対象外\n",
    )
    .unwrap();

    // 1. 設定ファイルが無い機体では何もしない。
    assert!(run(&stop_input(&repo, false)).is_none());

    // 2. 設定あり・記録なし: 足した段落だけが数えられ、block になる(基点の段落は数えない)。
    let cfg = root.join("config/dotfiles");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("ja-polish.toml"), "model = \"m\"\n").unwrap();
    let v = blocked(run(&stop_input(&repo, false))).expect("block");
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.contains("doc.md (1 段落"), "{reason}");
    assert!(reason.contains("足した段落の文章を直す"), "{reason}");
    assert!(!reason.contains("code.rs"), "文書の拡張子だけ見る");
    assert!(!reason.contains("承認済み"), "基点にある段落は数えない");

    // 3. block の直後の再呼び出し(stop_hook_active)と、逃げ道の環境変数では黙る。
    assert!(run(&stop_input(&repo, true)).is_none());
    std::env::set_var("SKIP_JA_POLISH_GATE", "1");
    assert!(run(&stop_input(&repo, false)).is_none());
    std::env::remove_var("SKIP_JA_POLISH_GATE");

    // 4. 記録に段落があれば通る。
    records::append(
        &records::records_path().unwrap(),
        &[records::Entry::polished(blocks::hash(NEW), "m")],
    )
    .unwrap();
    assert!(run(&stop_input(&repo, false)).is_none());

    // 5. git の外の文書: 書き込み前の PreToolUse で基点を残し、触れた一覧に足す。
    let vault = root.join("vault");
    std::fs::create_dir(&vault).unwrap();
    let note = vault.join("note.md");
    std::fs::write(&note, "既にあった段落で、承認済みの文章です。\n").unwrap();
    assert!(snapshot::hook(Agent::Claude, &pre_write("Edit", &note)).is_none());
    std::fs::write(
        &note,
        "既にあった段落で、承認済みの文章です。\n\nこの段落はセッションで足した。\n",
    )
    .unwrap();
    let v = blocked(run(&stop_input(&vault, false))).expect("git の外でも block");
    let reason = v["reason"].as_str().unwrap();
    assert!(
        reason.contains("note.md (1 段落") && reason.contains("セッションで足した"),
        "{reason}"
    );
    assert!(
        !reason.contains("既にあった"),
        "スナップショットにある段落は数えない"
    );

    // 6. 通したあと(記録あり)は、git の外でも通る。
    records::append(
        &records::records_path().unwrap(),
        &[records::Entry::polished(
            blocks::hash("この段落はセッションで足した。"),
            "m",
        )],
    )
    .unwrap();
    assert!(run(&stop_input(&vault, false)).is_none());
}
