//! `codex-config`(~/.codex/config.toml の宣言キーだけを合わせる)の試験。
//!
//! JSON 用の fixture 機構(reconcile.rs)は before.json / after.json 前提なので、
//! TOML は期待するバイト列をここに直接書く。どのケースも「もう一度流しても
//! 内容が変わらない」(冪等)を `apply` が検査する。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_agent-settings-reconcile");
const SPEC: &str = r#"{"tui.terminal_title":["activity","context-used","reasoning","five-hour-limit","weekly-limit"]}"#;
const LINE: &str = r#"terminal_title = ["activity", "context-used", "reasoning", "five-hour-limit", "weekly-limit"]"#;

fn run(file: &Path, spec: &str) -> Output {
    Command::new(BIN)
        .args(["codex-config", &file.to_string_lossy(), spec])
        .output()
        .unwrap()
}

/// 1 回流して成功を確かめ、結果の内容を返す。もう 1 回流して不変も確かめる。
fn apply(file: &Path, spec: &str) -> String {
    let out = run(file, spec);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let first = std::fs::read_to_string(file).unwrap();
    let out = run(file, spec);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        first,
        "2 回目の適用で内容が変わった(冪等でない)"
    );
    first
}

fn with(content: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("codex").join("config.toml");
    if let Some(c) = content {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, c).unwrap();
    }
    (d, f)
}

/// Codex が書いたままの形(コメント・空行・引用付きキー・サブテーブル)。
const CODEX_OWNED: &str = r#"model = "gpt-6.1-sol"   # 手で変えた

[projects."/home/u/.ghr/github.com/o/r"]
trust_level = "trusted"

[hooks.state."/home/u/.codex/hooks.json:session_start:0:0"]
trusted_hash = "sha256:aa"
"#;

#[test]
fn creates_missing_file_with_only_declared_key() {
    let (_d, f) = with(None);
    let got = apply(&f, SPEC);
    assert_eq!(got, format!("[tui]\n{LINE}\n"));
}

#[test]
fn adds_tui_table_after_codex_owned_content_without_touching_it() {
    let (_d, f) = with(Some(CODEX_OWNED));
    let got = apply(&f, SPEC);
    assert_eq!(got, format!("{CODEX_OWNED}\n[tui]\n{LINE}\n"));
}

#[test]
fn adds_key_to_existing_tui_table() {
    let before = "[tui]\nscreen_reader_detection_done = true\n\n[tui.model_availability_nux]\n\"gpt-6.1-sol\" = 3\n";
    let (_d, f) = with(Some(before));
    let got = apply(&f, SPEC);
    assert_eq!(
        got,
        format!(
            "[tui]\nscreen_reader_detection_done = true\n{LINE}\n\n[tui.model_availability_nux]\n\"gpt-6.1-sol\" = 3\n"
        )
    );
}

#[test]
fn overwrites_a_different_value_and_keeps_the_trailing_comment() {
    let before = "[tui]\nterminal_title = [\"model\"]  # 手で変えた\nother = 1\n";
    let (_d, f) = with(Some(before));
    let got = apply(&f, SPEC);
    assert_eq!(got, format!("[tui]\n{LINE}  # 手で変えた\nother = 1\n"));
}

#[test]
fn same_value_in_different_formatting_is_not_rewritten() {
    // 意味が同じなら、空白・クォートの違いでファイルを書き換えない。
    let before = "[tui]\nterminal_title=['activity','context-used','reasoning','five-hour-limit','weekly-limit']\n";
    let (_d, f) = with(Some(before));
    let got = apply(&f, SPEC);
    assert_eq!(got, before);
}

#[test]
fn unchanged_file_keeps_its_mtime() {
    let (_d, f) = with(Some(&format!("[tui]\n{LINE}\n")));
    let before = std::fs::metadata(&f).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(run(&f, SPEC).status.success());
    assert_eq!(std::fs::metadata(&f).unwrap().modified().unwrap(), before);
}

#[test]
fn supports_nested_new_tables_scalars_and_bools() {
    let (_d, f) = with(Some("a = 1\n"));
    let got = apply(&f, r#"{"x.y.z": "v", "x.y.n": 3, "x.b": true}"#);
    // spec の object は辞書順(serde_json の既定)なので n が z より先に並ぶ。
    assert_eq!(got, "a = 1\n\n[x]\nb = true\n\n[x.y]\nn = 3\nz = \"v\"\n");
}

#[test]
fn invalid_toml_is_a_runtime_error_and_leaves_the_file_alone() {
    let (_d, f) = with(Some("[tui\nbroken"));
    let out = run(&f, SPEC);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "[tui\nbroken");
}

#[test]
fn a_non_table_in_the_path_is_an_error() {
    let (_d, f) = with(Some("tui = 1\n"));
    let out = run(&f, SPEC);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "tui = 1\n");
}

#[test]
fn bad_specs_are_rejected_without_writing() {
    for spec in [
        "[]",
        "not json",
        r#"{"a..b": 1}"#,
        r#"{"a": null}"#,
        r#"{"a": 1.5}"#,
        r#"{"a": [[1]]}"#,
    ] {
        let (_d, f) = with(None);
        let out = run(&f, spec);
        assert!(!out.status.success(), "{spec} が通ってしまった");
        assert!(!f.exists(), "{spec}: 失敗したのにファイルができた");
    }
}

#[test]
fn wrong_arity_is_a_usage_error() {
    let out = Command::new(BIN)
        .args(["codex-config", "only-one"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn new_file_is_private_and_existing_mode_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, f) = with(None);
    assert!(run(&f, SPEC).status.success());
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let (_d2, f2) = with(Some("[tui]\n"));
    std::fs::set_permissions(&f2, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(run(&f2, SPEC).status.success());
    assert_eq!(
        std::fs::metadata(&f2).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn native_permissions_merge_literal_keys_and_only_existing_servers() {
    let (_d,f)=with(Some("model = 'owner'\n[mcp_servers.custom]\ncommand = 'server'\n[apps.connector_1.tools.search]\napproval_mode = 'approve'\n"));
    let spec = r#"{"settings":{"permissions":{"dotfiles":{"extends":":workspace","filesystem":{"~/.secret.path":"deny"}}},"apps":{"_default":{"default_tools_approval_mode":"prompt"}}},"mcp_allow":["mcp__custom__read","mcp__missing__read","mcp__codex_apps__search"]}"#;
    let invoke = || {
        Command::new(BIN)
            .args(["codex-permissions", &f.to_string_lossy(), spec])
            .output()
            .unwrap()
    };
    let out = invoke();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let warnings = String::from_utf8_lossy(&out.stderr);
    assert!(warnings.contains("custom MCP server missing absent"));
    assert!(warnings.contains("codex_apps tool search needs explicit"));
    let first = std::fs::read_to_string(&f).unwrap();
    let doc: toml_edit::DocumentMut = first.parse().unwrap();
    assert_eq!(
        doc["permissions"]["dotfiles"]["filesystem"]["~/.secret.path"].as_str(),
        Some("deny")
    );
    assert_eq!(
        doc["mcp_servers"]["custom"]["tools"]["read"]["approval_mode"].as_str(),
        Some("approve")
    );
    assert!(doc["mcp_servers"].get("missing").is_none());
    assert_eq!(
        doc["apps"]["connector_1"]["tools"]["search"]["approval_mode"].as_str(),
        Some("approve")
    );
    assert!(invoke().status.success());
    assert_eq!(std::fs::read_to_string(f).unwrap(), first);
}

#[test]
fn native_permissions_replace_owned_profile_and_legacy_selectors_with_backup() {
    let original = "model = 'owner'\nsandbox_mode = 'workspace-write'\n[sandbox_workspace_write]\nwritable_roots = ['/old']\n[permissions.dotfiles]\nextends = ':workspace'\n[permissions.dotfiles.filesystem]\n'/owner-extra' = 'write'\n[permissions.dotfiles.network.domains]\n'old.example' = 'allow'\n[permissions.other]\nextends = ':read-only'\n";
    let (_dir, file) = with(Some(original));
    let spec = r#"{"settings":{"default_permissions":"dotfiles","permissions":{"dotfiles":{"extends":":workspace","filesystem":{":root":"read"},"network":{"enabled":true,"domains":{"approved.example":"allow"}}}}},"mcp_allow":[]}"#;
    let invoke = || {
        Command::new(BIN)
            .args(["codex-permissions", &file.to_string_lossy(), spec])
            .output()
            .unwrap()
    };
    let out = invoke();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert!(doc.get("sandbox_mode").is_none());
    assert!(doc.get("sandbox_workspace_write").is_none());
    assert_eq!(doc["model"].as_str(), Some("owner"));
    assert_eq!(
        doc["permissions"]["other"]["extends"].as_str(),
        Some(":read-only")
    );
    assert!(doc["permissions"]["dotfiles"]["filesystem"]
        .get("/owner-extra")
        .is_none());
    assert!(doc["permissions"]["dotfiles"]["network"]["domains"]
        .get("old.example")
        .is_none());
    let backups: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("native-permissions-backup")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(backups[0].path()).unwrap(),
        original
    );
    assert!(invoke().status.success());
    assert_eq!(std::fs::read_to_string(file).unwrap(), text);
}
