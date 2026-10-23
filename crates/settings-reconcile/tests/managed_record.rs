//! `claude-hooks` の所有の記録(`state`)のテスト。
//!
//! 「前回書いたが今回の宣言に無い hook を消す」「記録に無い他ツールのエントリには、
//! 同じディレクトリ配下の command でも触れない」「記録は settings.json の書き込みが
//! 成功したあとにだけ更新する」を、実際の settings-reconcile のバイナリで固定する。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_settings-reconcile");

/// herdr 自身が settings.json に書く SessionStart の統合 hook(home/modules/claude.nix の
/// 退役の一覧の注記を参照)。`~/.claude/hooks/` の下にあるが、dotfiles の所有ではない。
const HERDR_OWN: &str = "bash '/home/u/.claude/hooks/herdr-agent-state.sh' session";

struct Env {
    _dir: tempfile::TempDir,
    settings: PathBuf,
    state: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let state = dir.path().join("state/dotfiles/managed-hooks.json");
    Env {
        _dir: dir,
        settings,
        state,
    }
}

fn reg(event: &str, command: &str) -> Value {
    json!({"event": event, "command": command, "timeout": 10})
}

fn run(env: &Env, register: &[Value], retire: &[Value]) -> Output {
    let spec = json!({
        "register": register,
        "retire": retire,
        "state": env.state.to_string_lossy(),
    });
    Command::new(BIN)
        .arg("claude-hooks")
        .arg(&env.settings)
        .arg(spec.to_string())
        .output()
        .unwrap()
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commands(settings: &Path, event: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    v["hooks"][event]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .flat_map(|g| g["hooks"].as_array().cloned().unwrap_or_default())
                .filter_map(|h| h["command"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn recorded(env: &Env) -> Vec<(String, String)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&env.state).unwrap()).unwrap();
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["event"].as_str().unwrap().to_owned(),
                e["command"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn first_run_writes_entries_and_the_record() {
    let e = env();
    ok(&run(
        &e,
        &[reg("Stop", "'/h/a'"), reg("PreToolUse", "'/h/b'")],
        &[],
    ));
    assert_eq!(commands(&e.settings, "Stop"), ["'/h/a'"]);
    assert_eq!(
        recorded(&e),
        [
            ("PreToolUse".to_string(), "'/h/b'".to_string()),
            ("Stop".to_string(), "'/h/a'".to_string()),
        ]
    );
}

#[test]
fn dropping_a_declaration_removes_exactly_what_we_wrote() {
    let e = env();
    ok(&run(
        &e,
        &[reg("Stop", "'/h/a'"), reg("Stop", "'/h/gone-later'")],
        &[],
    ));
    // 宣言から外す。退役の一覧(retire)には書かない。
    ok(&run(&e, &[reg("Stop", "'/h/a'")], &[]));
    assert_eq!(commands(&e.settings, "Stop"), ["'/h/a'"]);
    assert_eq!(recorded(&e), [("Stop".to_string(), "'/h/a'".to_string())]);
}

#[test]
fn entries_we_did_not_write_are_untouched_even_under_the_same_directory() {
    let e = env();
    // herdr が先に自分の統合 hook を書いている。
    std::fs::write(
        &e.settings,
        serde_json::to_string_pretty(&json!({
            "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": HERDR_OWN}]}]}
        }))
        .unwrap(),
    )
    .unwrap();
    ok(&run(
        &e,
        &[reg("SessionStart", "'/home/u/.claude/hooks/desk-start'")],
        &[],
    ));
    // 宣言を空にしても、記録に無い herdr のエントリは残る。
    ok(&run(&e, &[], &[]));
    assert_eq!(commands(&e.settings, "SessionStart"), [HERDR_OWN]);
    assert!(recorded(&e).is_empty());
}

#[test]
fn the_frozen_retire_list_still_works_without_a_record() {
    let e = env();
    std::fs::write(
        &e.settings,
        serde_json::to_string_pretty(&json!({
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "bash '/h/old.sh'"}]}]}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(!e.state.exists());
    ok(&run(
        &e,
        &[reg("Stop", "'/h/new'")],
        &[json!({"event": "Stop", "command": "bash '/h/old.sh'"})],
    ));
    assert_eq!(commands(&e.settings, "Stop"), ["'/h/new'"]);
    assert!(e.state.exists());
}

#[test]
fn steady_state_touches_nothing() {
    let e = env();
    ok(&run(&e, &[reg("Stop", "'/h/a'")], &[]));
    let settings = std::fs::metadata(&e.settings).unwrap().modified().unwrap();
    let state = std::fs::metadata(&e.state).unwrap().modified().unwrap();
    ok(&run(&e, &[reg("Stop", "'/h/a'")], &[]));
    assert_eq!(
        std::fs::metadata(&e.settings).unwrap().modified().unwrap(),
        settings
    );
    assert_eq!(
        std::fs::metadata(&e.state).unwrap().modified().unwrap(),
        state
    );
}

#[test]
fn a_corrupt_record_is_treated_as_a_first_run() {
    let e = env();
    std::fs::create_dir_all(e.state.parent().unwrap()).unwrap();
    std::fs::write(&e.state, "{").unwrap();
    let out = run(&e, &[reg("Stop", "'/h/a'")], &[]);
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("所有の記録が壊れている"));
    assert_eq!(commands(&e.settings, "Stop"), ["'/h/a'"]);
    assert_eq!(recorded(&e), [("Stop".to_string(), "'/h/a'".to_string())]);
}

#[test]
fn the_record_is_not_written_when_settings_cannot_be_updated() {
    let e = env();
    std::fs::write(&e.settings, "not json").unwrap();
    let out = run(&e, &[reg("Stop", "'/h/a'")], &[]);
    assert!(!out.status.success());
    assert!(!e.state.exists());
}

#[test]
fn without_state_nothing_is_recorded() {
    let e = env();
    let spec = json!({"register": [reg("Stop", "'/h/a'")]});
    ok(&Command::new(BIN)
        .arg("claude-hooks")
        .arg(&e.settings)
        .arg(spec.to_string())
        .output()
        .unwrap());
    assert_eq!(commands(&e.settings, "Stop"), ["'/h/a'"]);
    assert!(!e.state.exists());
}
