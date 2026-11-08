//! `issue_inbox::add_issued`: 既に Issue にした気づきを、状態 `issued` のノートとして vault に残す。
//!
//! retro の user-correction は Inbox を通らず Issue になる(ADR-697)ので、そのままだと兆候ごとの
//! 採用・却下の集計(ADR-1276 D6)に入らない。起票と同時にこのノートを残し、集計に載せる。
//!
//! 環境変数を書き換えるので、このファイルのテストは 1 つにまとめてある(別のプロセスで走る)。

use std::fs;

#[test]
fn a_filed_issue_is_kept_as_an_issued_note_only_when_a_vault_is_declared() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(vault.join(".obsidian")).unwrap();
    fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
    std::env::set_var("HOME", "/nonexistent-home");
    std::env::set_var("WRAPUP_STATE_DIR", tmp.path().join("state"));
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_STATE_HOME");

    let line = serde_json::json!({
        "ts": "2026-11-08T00:00:00Z",
        "title": "指摘の題名",
        "detail": "evidence (retro user-correction; mechanism: gate)",
        "repo": "o/r",
        "signal": "repeated-violation",
        "mechanism": "gate",
        "existing": "拡張:hook-router",
    })
    .to_string();
    let project = tmp.path().to_str().unwrap();

    // vault を宣言していない機体では何も残さない(jsonl の行は状態を持たない)。
    std::env::remove_var("OBSIDIAN_VAULT");
    assert_eq!(issue_inbox::add_issued(project, &line, 42), Ok(false));
    assert!(!vault.join("inbox").exists());

    // vault を宣言していれば、状態 issued・Issue の URL つきのノートが 1 枚できる。
    std::env::set_var("OBSIDIAN_VAULT", &vault);
    assert_eq!(issue_inbox::add_issued(project, &line, 42), Ok(true));
    let note = fs::read_to_string(vault.join("inbox/指摘の題名.md")).unwrap();
    assert!(note.contains("status: \"issued\""), "{note}");
    assert!(
        note.contains("issue: \"https://github.com/o/r/issues/42\""),
        "{note}"
    );
    assert!(note.contains("signal: \"repeated-violation\""), "{note}");

    // 題名が無い行は、ノートを作らずエラーにする(retro は止めない側が受ける)。
    let bad = serde_json::json!({"detail": "d"}).to_string();
    assert!(issue_inbox::add_issued(project, &bad, 42).is_err());
}
