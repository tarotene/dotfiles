//! `gh` の実行。stdin は閉じ、stderr は捨てる(認証・ネットワークの失敗は区別しない)。
//!
//! 各 hook・CLI が同じ形の呼び出しを private に持っていた(`pr-gate` の `gh::gh`・`stack-base-guard` の
//! `stacks` と `open_prs`・`issue-create-guard` の `gh_visibility` など)。違いは、成功か失敗かを
//! 区別するか、cwd を指定するかだけなので、核をここに置く。timeout が要る呼び出しと、stdin や stderr を
//! 引き継ぐ呼び出し(`feedback-trial`・`detect-drift` など)は、挙動が違うので対象外で、各 crate が持つ。

use std::path::Path;
use std::process::{Command, Stdio};

/// `gh <args>`(cwd は `dir` か、継承)の (成功したか, stdout の末尾の改行を除いたもの)。
/// 起動できなければ `None`。
pub fn gh_in(dir: Option<&Path>, args: &[&str]) -> Option<(bool, String)> {
    let mut cmd = Command::new("gh");
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let out = cmd.output().ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string(),
    ))
}

/// [`gh_in`] の cwd 継承版。
pub fn gh(args: &[&str]) -> Option<(bool, String)> {
    gh_in(None, args)
}

/// 成功時の stdout(末尾の改行を除いたもの)。失敗・起動不能は `None`。空出力は `Some("")`。
pub fn gh_ok_in(dir: Option<&Path>, args: &[&str]) -> Option<String> {
    match gh_in(dir, args)? {
        (true, out) => Some(out),
        _ => None,
    }
}

/// [`gh_ok_in`] の cwd 継承版。
pub fn gh_ok(args: &[&str]) -> Option<String> {
    gh_ok_in(None, args)
}

/// `gh api repos/<nwo>/stacks` の生 JSON。取得不能(404・機能撤収・認証・ネットワーク障害のいずれか
/// 区別しない)なら `None`。空出力は `Some("")` で、空を失敗と読むかは呼び出し側が決める。
pub fn stacks(nwo: &str) -> Option<String> {
    gh_ok(&["api", &format!("repos/{nwo}/stacks")])
}

/// open な PR の一覧(最大 100 件)。`fields` は `--json` に渡す欄(例: `number,headRefName,baseRefName`)。
pub fn open_prs(nwo: &str, fields: &str) -> Option<String> {
    gh_ok(&[
        "pr", "list", "-R", nwo, "--state", "open", "--limit", "100", "--json", fields,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_binary_is_none_and_never_panics() {
        // この試験環境に `gh` が無くても、有っても、起動の失敗が `None` になるだけで panic しない。
        let _ = gh(&["--version"]);
        assert_eq!(
            gh_in(Some(Path::new("/nonexistent-dir-for-test")), &["--version"]),
            None,
            "cwd が無ければ起動できない"
        );
    }
}
