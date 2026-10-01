//! G_prior: PR で新設した「新しい道具・単位」に `既存手段:` 記載があるか
//! (ADR-543「既存手段の前倒し接地と、決定論への昇格導線」D1)。
//!
//! 判定エンジンは `crates/new-tool-guard` の [`new_tool_guard::is_new_tool_unit`]
//! を単一正本として使う(ADR-0024「hook と呼び出し元が同じ判定エンジンを共有
//! する」型)。bash 版は `new-tool-guard classify` を外部コマンドとして呼び、
//! バイナリが無ければ非該当(fail-open)にしていた。Rust 版は同じ関数を直接
//! リンクするので、その縮退経路は無くなった(`NEW_TOOL_GUARD_BIN` も見ない)。
//! base 側の ref が手元に無いクローンでは追加ファイルが取れず、判定不能として
//! 完全に沈黙する(断定に変えない)。

use crate::body;
use std::path::Path;

/// `<project>/<path>` が新しい道具・単位に該当するか。ファイルが読めなければ
/// 空内容として判定する(`new-tool-guard classify` と同じ)。
pub fn is_new_tool_unit_at(project: &Path, path: &str) -> bool {
    let content = std::fs::read_to_string(project.join(path)).unwrap_or_default();
    new_tool_guard::is_new_tool_unit(path, &content)
}

/// `既存手段:` 記載が欠けている追加ファイル(1 件 1 要素)。
pub fn judge(project: &Path, body_text: &str, added: &[String]) -> Vec<String> {
    let stripped = body::strip_code_spans(body_text);
    added
        .iter()
        .filter(|f| !f.is_empty())
        .filter(|f| is_new_tool_unit_at(project, f))
        .filter(|f| !body::body_has_kizon_for(&stripped, f))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_nothing() {
        // ok   G_prior: 空入力は何も出さない
        let cwd = std::env::current_dir().unwrap();
        assert!(judge(&cwd, "", &[]).is_empty());
    }

    #[test]
    fn new_unit_without_kizon_is_reported() {
        // 移植時に追加: bash の selftest は環境依存として検査しなかった経路。
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("scripts")).unwrap();
        std::fs::write(d.path().join("scripts/foo"), "#!/bin/sh\necho hi\n").unwrap();
        let added = vec!["scripts/foo".to_string()];
        assert!(is_new_tool_unit_at(d.path(), "scripts/foo"));
        assert_eq!(judge(d.path(), "本文", &added), added);
        assert!(judge(d.path(), "既存手段: scripts/foo — 採用: sh", &added).is_empty());
        // コードスパン内の記載は数えない(GitHub と同じ読み方)。
        assert_eq!(
            judge(d.path(), "`既存手段: scripts/foo — 採用: sh`", &added),
            added
        );
    }
}
