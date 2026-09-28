//! new-tool-guard — Q1(既存手段で足りないか)を「新しい道具・単位の誕生」
//! に前倒しで問う PreToolUse(Write) hook。設計は
//! `docs/adr/543-existing-means-and-deterministic-promotion.md`。
//!
//! 判定は 2 段:
//! 1. [`is_new_tool_unit`] — 対象ファイルが「新しい道具・単位」か
//!    (述語そのものは pr-gate `judge_prior` からも同じ2値判定として
//!    呼ばれる。`classify` サブコマンドが単一正本、ADR-0024 の
//!    「hook と CI が同じ判定エンジンを呼ぶ」型)。
//! 2. [`ledger_has_record`] — そのパスについて `既存手段:` が既に
//!    session ledger(`crates/hook-io::SessionLedger`、キーは
//!    session_id ではなく git toplevel、ADR-543)に登録済みか。

use std::path::Path;

/// 除外パス(テスト・fixture・scratchpad)。これらは新しい道具・単位の
/// 述語から常に除外する — テストコードは対象読者が別。
pub fn is_excluded_path(path: &str) -> bool {
    let lower = path.replace('\\', "/");
    let segs: Vec<&str> = lower.split('/').collect();
    let in_test_dir = segs
        .iter()
        .any(|s| matches!(*s, "tests" | "test" | "fixtures" | "fixture" | "spec"));
    let file_marks_test = segs
        .last()
        .map(|f| f.contains("_test.") || f.contains(".test."))
        .unwrap_or(false);
    let in_scratchpad = lower.contains("/scratchpad/") || lower.starts_with("/tmp/claude-");
    in_test_dir || file_marks_test || in_scratchpad
}

/// パスの各構成要素に `bin`/`scripts`/`hooks`/`cmd` のいずれかが
/// 完全一致で含まれるか。
pub fn has_tool_dir_component(path: &str) -> bool {
    path.replace('\\', "/")
        .split('/')
        .any(|seg| matches!(seg, "bin" | "scripts" | "hooks" | "cmd"))
}

/// 新設パッケージマニフェストのファイル名(閉集合)。
pub const MANIFEST_FILENAMES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "flake.nix",
];

pub fn is_manifest_filename(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .map(|f| MANIFEST_FILENAMES.contains(&f))
        .unwrap_or(false)
}

/// 「新しい道具・単位」の述語(ADR-543 D1)。行数閾値は設けない。
///
/// - shebang(`#!`)で始まる新規ファイル
/// - パス要素に `bin`/`scripts`/`hooks`/`cmd` を含む新規ファイル
/// - パッケージマニフェストの新設
///
/// いずれも `tests?/`・`fixtures?/`・`spec/`・scratchpad 配下は除外する。
pub fn is_new_tool_unit(path: &str, content: &str) -> bool {
    if is_excluded_path(path) {
        return false;
    }
    content.starts_with("#!") || has_tool_dir_component(path) || is_manifest_filename(path)
}

/// 節内整合性用の閉語彙。`register` の入力検査と pr-gate 側の PR 本文検査が
/// 共有する(judge_precedent 相当の判定を Rust 側でも同じ形にする)。
pub fn is_valid_kizon_line(line: &str) -> Result<(), &'static str> {
    let body = line
        .trim()
        .strip_prefix("既存手段:")
        .ok_or("「既存手段:」で始まっていません")?;
    let body = body.trim_start();
    if body.is_empty() || record_path(line).is_none() {
        return Err("パスが空です");
    }
    if !(body.contains("採用:") || body.contains("拡張:") || body.contains("自前")) {
        return Err("採用:/拡張:/自前 のいずれでもありません");
    }
    if body.contains("自前") && !(body.contains("却下:") || body.contains("探索:")) {
        return Err("自前 を選ぶ場合は 却下:/探索: の理由が必要です");
    }
    Ok(())
}

/// `既存手段:` 行から対象パスだけを取り出す。ダッシュ種(-/—/–)を問わず、
/// 前後に空白を伴う最初のダッシュより前をパスとみなす(plan-precedent-
/// gate.sh の `CITATION_RE` 相当の緩さ)。
pub fn record_path(record: &str) -> Option<String> {
    // `trim_start()` はしない: パスが空のとき「既存手段: — 採用: ...」の
    // ように区切りダッシュの直前が単一スペースだけになる。先に trim すると
    // その空パスを見失う(前後の空白ごと `body[..cut]` へ含め、最後に
    // まとめて trim する)。
    let body = record.trim().strip_prefix("既存手段:")?;
    let mut cut = body.len();
    for dash in [" - ", " — ", " – "] {
        if let Some(idx) = body.find(dash) {
            cut = cut.min(idx);
        }
    }
    let p = body[..cut].trim();
    (!p.is_empty()).then(|| p.to_string())
}

/// ledger の全レコードのうち、指定パスについて既に登録済みか。
pub fn ledger_has_record(records: &[String], path: &str) -> bool {
    records
        .iter()
        .any(|r| record_path(r).as_deref() == Some(path))
}

/// deny メッセージ(register コマンドの完全形を同梱し、往復を1回で終える —
/// plan-precedent-gate.sh の `example_block` と同じ理由)。
pub fn deny_message(path: &str) -> String {
    format!(
        "新しい道具・単位を追加しようとしています: {path}\n\n\
既存の枯れた技術で足りないか検討し、次のコマンドで登録してから再試行してください\n\
(ADR-543「既存手段の前倒し接地と、決定論への昇格導線」):\n\n\
  ~/.claude/hooks/new-tool-guard register '既存手段: {path} — 採用: <ツール名/URL>'\n\
  ~/.claude/hooks/new-tool-guard register '既存手段: {path} — 拡張: <既存パス>'\n\
  ~/.claude/hooks/new-tool-guard register '既存手段: {path} — 自前 — 却下: <候補> (<理由>)'\n\n\
`register` は現在の worktree(git toplevel)にだけ効く。PR 作成時は本文にも\n\
同じ行(またはそれと矛盾しない内容)を書く(pr-description スキル)。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shebang_new_file_is_tool_unit() {
        assert!(is_new_tool_unit(
            "scripts/foo.sh",
            "#!/usr/bin/env bash\necho hi\n"
        ));
        assert!(is_new_tool_unit(
            "crates/x/src/main.rs",
            "#!/usr/bin/env -S cargo\n"
        ));
    }

    #[test]
    fn tool_dir_component_without_shebang_is_tool_unit() {
        assert!(is_new_tool_unit(
            "config/claude/hooks/new-thing.py",
            "print('hi')\n"
        ));
        assert!(is_new_tool_unit("bin/greet", "not a script\n"));
        assert!(!is_new_tool_unit(
            "src/binder.rs",
            "// 'bin' はセグメント一致のみ\n"
        ));
    }

    #[test]
    fn manifest_filenames_are_tool_units() {
        for f in [
            "crates/new-tool-guard/Cargo.toml",
            "web/package.json",
            "py/pyproject.toml",
            "go/go.mod",
            "flake.nix",
        ] {
            assert!(is_new_tool_unit(f, "{}"), "{f}");
        }
        assert!(!is_new_tool_unit("crates/new-tool-guard/Cargo.lock", "{}"));
    }

    #[test]
    fn plain_module_is_not_tool_unit() {
        assert!(!is_new_tool_unit(
            "crates/hook-io/src/plan.rs",
            "pub fn x() {}\n"
        ));
    }

    #[test]
    fn excluded_paths_never_count() {
        for p in [
            "crates/foo/tests/cli.rs",
            "crates/foo/src/bin_test.rs",
            "config/claude/hooks/fixtures/x.sh",
            "spec/bin/x.sh",
            "/tmp/claude-1000/scratch/scripts/x.sh",
        ] {
            assert!(!is_new_tool_unit(p, "#!/bin/sh\n"), "{p}");
        }
    }

    #[test]
    fn kizon_line_validation() {
        assert!(is_valid_kizon_line("既存手段: p — 採用: jq").is_ok());
        assert!(is_valid_kizon_line("既存手段: p — 拡張: crates/x").is_ok());
        assert!(is_valid_kizon_line("既存手段: p — 自前 — 却下: jq (理由)").is_ok());
        assert!(is_valid_kizon_line("既存手段: p — 自前").is_err());
        assert!(is_valid_kizon_line("既存手段: — 採用: jq").is_err());
        assert!(is_valid_kizon_line("採用: jq").is_err());
    }

    #[test]
    fn record_path_extraction() {
        assert_eq!(
            record_path("既存手段: crates/new-tool-guard/src/main.rs — 自前 — 却下: jq"),
            Some("crates/new-tool-guard/src/main.rs".to_string())
        );
        assert_eq!(
            record_path("既存手段: scripts/x.sh — 採用: jq"),
            Some("scripts/x.sh".to_string())
        );
        assert_eq!(record_path("先行例: 何か"), None);
    }

    #[test]
    fn ledger_lookup() {
        let records = vec![
            "既存手段: scripts/x.sh — 採用: jq".to_string(),
            "既存手段: bin/y — 自前 — 却下: なし (理由)".to_string(),
        ];
        assert!(ledger_has_record(&records, "scripts/x.sh"));
        assert!(ledger_has_record(&records, "bin/y"));
        assert!(!ledger_has_record(&records, "bin/z"));
    }
}
