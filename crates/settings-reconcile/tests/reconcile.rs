//! settings-reconcile の characterization / reconcile fixture。
//!
//! `tests/fixtures/<case>/` に before.json / steps.json / after.json を置く。
//! steps.json は `[[<subcommand>, <arg>...], ...]`(対象ファイルは自動で
//! subcommand の直後に挿入する。文字列以外の arg は JSON 文字列にして渡す)。
//!
//! - 旧実装(scripts/register-{codex,copilot}-hooks と claude.nix /
//!   claude-mcp-servers.nix に埋め込まれていた jq)を oracle にして after.json
//!   を生成したもの: 挙動を変えていないことを固定する(移植、#414)。
//! - `claude-hooks-updates-*` / `*-adds-matcher-*` / `*-moves-handler-*` /
//!   `*-collapses-duplicate-*` / `codex-updates-*` / `copilot-updates-*` は
//!   手書き: 宣言を正とする reconcile への意図的な変更(旧 bash では command
//!   一致だけで存在判定していたので反映されなかった)。
//!
//! どの case も「もう一度全 step を流しても内容が変わらない」(冪等)を検査する。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_settings-reconcile");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn run(sub_args: &[String], file: &Path) -> Output {
    let mut args = vec![sub_args[0].clone(), file.to_string_lossy().into_owned()];
    args.extend_from_slice(&sub_args[1..]);
    Command::new(BIN).args(&args).output().unwrap()
}

fn step_args(step: &serde_json::Value) -> Vec<String> {
    step.as_array()
        .unwrap()
        .iter()
        .map(|a| match a {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

#[test]
fn fixtures_match_and_are_idempotent() {
    let mut n = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        if dir.join("before.json").exists() {
            std::fs::copy(dir.join("before.json"), &file).unwrap();
        }
        let steps: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("steps.json")).unwrap())
                .unwrap();
        let steps: Vec<Vec<String>> = steps.as_array().unwrap().iter().map(step_args).collect();
        let want = std::fs::read_to_string(dir.join("after.json")).unwrap();

        for s in &steps {
            let out = run(s, &file);
            assert!(
                out.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let got = std::fs::read_to_string(&file).unwrap();
        assert_eq!(got, want, "{name}: 1 回目の結果が after.json と違う");

        // 冪等: 同じ宣言をもう一度流しても内容が変わらない。
        for s in &steps {
            let out = run(s, &file);
            assert!(
                out.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            want,
            "{name}: 2 回目の適用で内容が変わった(冪等でない)"
        );
        n += 1;
    }
    assert!(n >= 25, "fixture が足りない: {n}");
}

fn tmp_file(content: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("settings.json");
    if let Some(c) = content {
        std::fs::write(&f, c).unwrap();
    }
    (d, f)
}

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

// --- bash selftest の arity エラー(usage = exit 2、黙って no-op にしない) ---

#[test]
fn codex_leftover_args_not_multiple_of_four_is_usage_error() {
    let (_d, f) = tmp_file(Some("{}\n"));
    let out = run(
        &strs(&["codex-hooks", "SessionStart", "", "only-three-args"]),
        &f,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}\n");
}

#[test]
fn copilot_leftover_args_not_multiple_of_three_is_usage_error() {
    let (_d, f) = tmp_file(Some("{}\n"));
    let out = run(
        &strs(&["copilot-hooks", "sessionStart", "only-two-args"]),
        &f,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}\n");
}

#[test]
fn retire_without_pair_is_usage_error() {
    let (_d, f) = tmp_file(Some("{}\n"));
    let out = run(&strs(&["codex-hooks", "--retire", "PreToolUse"]), &f);
    assert_eq!(out.status.code(), Some(2));
}

// #576: 複数 pair の前に `--retire` を 1 つだけ置く形は usage エラー(以前は後続の
// 引数が 1 つずつずれて register が黙って no-op になった)。
#[test]
fn bare_retire_pairs_after_single_retire_token_is_usage_error() {
    let (_d, f) = tmp_file(Some("{}\n"));
    let out = run(
        &strs(&[
            "codex-hooks",
            "--retire",
            "Stop",
            "a",
            "Stop",
            "b",
            "--register",
            "Stop",
            "",
            "c",
            "10",
        ]),
        &f,
    );
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn bad_spec_json_is_usage_error_and_leaves_file_alone() {
    let (_d, f) = tmp_file(Some("{\"a\":1}\n"));
    let out = run(&strs(&["claude-hooks", "{not json"]), &f);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{\"a\":1}\n");
}

#[test]
fn unreadable_settings_is_runtime_error() {
    let (_d, f) = tmp_file(Some("not json"));
    let out = run(&strs(&["claude-statusline", r#"{"desired":"x"}"#]), &f);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "not json");
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let (_d, f) = tmp_file(None);
    assert_eq!(run(&strs(&["nope"]), &f).status.code(), Some(2));
}

// --- 書き込みの性質 ---

#[test]
fn unchanged_declaration_does_not_rewrite_the_file() {
    // 定常状態で mtime を汚さない。key 順・字下げが違う(= jq の既定形でない)
    // 入力でも、内容が変わらないなら書き換えない。
    let src = r#"{"statusLine":{"command":"sl","type":"command"}}"#;
    let (_d, f) = tmp_file(Some(src));
    let out = run(&strs(&["claude-statusline", r#"{"desired":"sl"}"#]), &f);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), src);
}

#[cfg(unix)]
#[test]
fn rewrite_preserves_file_mode_and_leaves_no_temp_files() {
    use std::os::unix::fs::PermissionsExt;
    let (d, f) = tmp_file(Some("{}\n"));
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    let out = run(&strs(&["claude-statusline", r#"{"desired":"sl"}"#]), &f);
    assert!(out.status.success());
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "settings.json")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn missing_file_and_parent_dir_are_created_as_empty_object() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("nested/dir/hooks.json");
    let out = run(
        &strs(&["codex-hooks", "--retire", "Stop", "x", "--register"]),
        &f,
    );
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}\n");
}

#[test]
fn mcp_removal_is_reported_on_stderr() {
    let (_d, f) = tmp_file(Some(r#"{"mcpServers":{"stray":{"command":"x"}}}"#));
    let out = run(&strs(&["claude-mcp-servers", "{}"]), &f);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("宣言外の MCP サーバーを削除します: stray"),
        "{err}"
    );
}

#[test]
fn duplicate_declaration_keeps_the_first_and_warns() {
    let (_d, f) = tmp_file(Some("{}\n"));
    let spec = r#"{"register":[
        {"event":"Stop","command":"a","timeout":1},
        {"event":"Stop","command":"a","timeout":2}]}"#;
    let out = run(&strs(&["claude-hooks", spec]), &f);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("重複した hook 宣言"));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["timeout"], 1);
}

#[test]
fn unknown_spec_field_is_rejected() {
    // 宣言のタイポ(例: `timeout` を `timout`)を黙って捨てない。
    let (_d, f) = tmp_file(Some("{}\n"));
    let out = run(
        &strs(&[
            "claude-hooks",
            r#"{"register":[{"event":"Stop","command":"a","timout":1}]}"#,
        ]),
        &f,
    );
    assert_eq!(out.status.code(), Some(2));
}
