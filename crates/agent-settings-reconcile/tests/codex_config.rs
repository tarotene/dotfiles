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
        r#"{"a": {"b": 1}}"#,
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
