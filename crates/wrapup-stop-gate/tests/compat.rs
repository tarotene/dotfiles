//! 旧入口(互換の bin)の転送の契約。流れ全体のテストは `crates/session-wrapup/tests/` が正本の bin で
//! 持つ。ここは、旧入口に固有の振る舞い(廃止した旗の拒否、正本の入口と同じ終了コードで転送すること)だけを見る。
//! 旧入口を削除するとき(docs/agents/tooling-migration.md の削除条件)は、このファイルも削除する。

use std::io::Write;
use std::process::{Command, Stdio};

fn run(bin: &str, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// 手順書(`--procedure`)と実行中の印(`--chores-*`)は無くなった。Stop hook として動いて素通りせず、拒否する。
#[test]
fn the_removed_flags_are_refused_not_run_as_the_stop_hook() {
    for flag in ["--procedure", "--chores-begin", "--chores-end"] {
        let (code, _, stderr) = run(
            env!("CARGO_BIN_EXE_wrapup-stop-gate"),
            &[flag, "/x/y.jsonl"],
            "",
        );
        assert_eq!(code, 1, "{flag}");
        assert!(stderr.contains("was removed"), "{flag}: {stderr}");
    }
}

/// 引数なしの旧入口は Stop hook で、`stop_hook_active` なら何も読まずに素通りする。
#[test]
fn no_arguments_runs_the_stop_hook() {
    let (code, stdout, stderr) = run(
        env!("CARGO_BIN_EXE_wrapup-stop-gate"),
        &[],
        r#"{"cwd":"","stop_hook_active":true}"#,
    );
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
}

/// 旗つきの呼び出しは正本の入口(issue-inbox)と同じ検査をする: 不正な JSON は終了コード 64 で拒否する。
#[test]
fn a_flag_is_forwarded_to_the_canonical_entry() {
    let (code, stdout, stderr) = run(
        env!("CARGO_BIN_EXE_wrapup-stop-gate"),
        &["--add", "/nonexistent/compat/inbox.jsonl", "not-json"],
        "",
    );
    assert_eq!((code, stdout.as_str()), (64, ""));
    assert!(stderr.contains("--add: 不正な JSON です"), "{stderr}");
}

/// SessionStart の旧入口は、GitHub remote を持つプロジェクトで、収集の指示文を additionalContext に出す。
/// 指示文が指すコマンドは、旧入口自身(同じディレクトリの `wrapup-stop-gate`)のまま。
#[test]
fn session_start_names_the_compat_gate_next_to_itself() {
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("compat-session-start");
    let _ = std::fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(tmp.join("home")).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["remote", "add", "origin", "https://github.com/acme/x.git"],
    ] {
        let st = Command::new("git")
            .arg("-C")
            .arg(&proj)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(st.success());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_wrapup-session-start"))
        .env("HOME", tmp.join("home"))
        .env("WRAPUP_STATE_DIR", tmp.join("state"))
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("OBSIDIAN_VAULT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin
                .take()
                .unwrap()
                .write_all(format!(r#"{{"cwd":"{}"}}"#, proj.display()).as_bytes())?;
            c.wait_with_output()
        })
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains(r#"source="wrapup-session-start""#), "{ctx}");
    let gate = std::path::Path::new(env!("CARGO_BIN_EXE_wrapup-session-start"))
        .parent()
        .unwrap()
        .join("wrapup-stop-gate");
    assert!(
        ctx.contains(&format!("'{}' --add", gate.display())),
        "{ctx}"
    );
}
