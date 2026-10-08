//! repo-create-guard — `gh repo create` / `gh api -X POST user/repos`・
//! `gh api -X POST orgs/*/repos` を作成時点で deny し、`repo-charter` スキルの
//! 手順(命名インタビュー → README/CONTRIBUTING → GitHub メタデータ反映 →
//! governance 播種)に強制的に載せる PreToolUse(Bash) hook
//! (docs/claude/repo-create-guard.md、ADR-0013 Amendment 2026-09-29、
//! ADR-0024 Stage 4a #415)。
//!
//! bash 版 `config/claude/hooks/repo-create-guard.sh` の移植。コマンド解析は
//! `guard-core` にあり、ここには対象コマンドと deny 文言だけを置く。
//!
//! 動機: `gh repo create` 自体には作成直後に走るフック機構が無く、作成時の
//! 強制点は `repo-charter` スキルという散文的手順のみだった(ADR-0013 D2)。
//! ある private リポジトリの新規作成セッションで、`core` 型の charter 手順が
//! `apply-repo-settings.sh` の呼び出しを欠いたまま実行され、実際に settings
//! drift(squash-only 化・delete-branch-on-merge 等が未適用)が発生した
//! (2026-09-28 実例、実名は書かない — ADR-0034)。この抜けは散文的手順を
//! 読み飛ばせば常に起こりうるため、作成コマンドそのものを deny して手順に
//! 強制的に載せる。
//!
//! 軸: 検出のみ — `gh` 経由の生成は deny できるが、`curl` で直接
//! `api.github.com/user/repos` を叩く経路や Web UI からの作成はこの hook の
//! 対象外(ADR-543 D1 と同型の限界、フルスクラッチ自体は表現不可能にできない)。
//!
//! バイパス: `REPO_CREATE_GUARD_BYPASS=1`(rulesets-write-guard と同型)。
//! bash 版と同じく hook 入出力層(main)の責務で、`decide` 自体は判定
//! ロジックだけを持つ。MCP(`mcp__github*`)は現在未接続のため Bash のみ判定する
//! — attribution-guard の `decide_mcp` と同じ理由。

use guard_core::command::{first_deny, gh_command_at, is_gh};
use guard_core::gh::{api_method, api_path};
use guard_core::Range;

/// 対象コマンドの種別(bash の `TARGET_KIND`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// `gh repo create`
    Cli,
    /// `gh api …`(メソッド・パスの絞り込みは [`decide_api_tokens`])
    Api,
}

/// bash の `is_target_at`: `gh repo create` と `gh api …` だけを対象にする。
pub fn is_target_at(tokens: &[String], i: usize) -> Option<TargetKind> {
    if i + 1 >= tokens.len() || !is_gh(&tokens[i]) {
        return None;
    }
    if tokens[i + 1] == "api" {
        return Some(TargetKind::Api);
    }
    gh_command_at(tokens, i, &["repo", "create"]).then_some(TargetKind::Cli)
}

const DENY_HINT: &str = "repo-charter スキルの作成スクリプトを使ってください
(config/claude/skills/repo-charter/SKILL.md §8):

  ~/.claude/skills/repo-charter/scripts/create-repo.sh --owner <owner> --repo <repo> \\
    --description \"<目的 1 文>\" --class <naming-クラス> --topic <topic> \\
    --type <rust|typst|astro|core> --dest <ローカルの checkout>
  (--dry-run で、検査の結果と走らせるコマンドを先に確かめられます)

スクリプトは §1 の閉語彙チェック(naming-codename/naming-descriptive 等のクラス確定)
→ `gh repo create` → `gh repo edit --add-topic <クラス>` → 型別の governance 播種まで
一式で行います。スクリプト経由なら迂回用の環境変数は要りません。

REPO_CREATE_GUARD_BYPASS=1 は、本人が `!` 付きで実行するときのためのものです。
エージェントが自分で前置しても auto モードの分類器に拒否されます(#754)
(ADR-0013 Amendment 2026-09-29 参照)。";

fn deny_reason_repo_create() -> String {
    format!("素の `gh repo create` は使わないでください。\n\n{DENY_HINT}")
}

fn deny_reason_repo_create_api(path: &str) -> String {
    format!("`gh api` での repo 作成({path})は使わないでください。\n\n{DENY_HINT}")
}

/// `REPO_CREATE_API_RE='^/(user/repos|orgs/[^/]+/repos)$'`。
///
/// gh api での repo 作成エンドポイント: 認証ユーザー自身の `user/repos` と
/// 組織配下の `orgs/{org}/repos`。POST 以外(GET での一覧取得、PATCH での
/// settings 変更 — apply-repo-settings.sh 自身がこれを使う)は対象外。
pub fn is_repo_create_path(t: &str) -> bool {
    if t == "/user/repos" {
        return true;
    }
    t.strip_prefix("/orgs/")
        .and_then(|r| r.strip_suffix("/repos"))
        .is_some_and(|org| !org.is_empty() && !org.contains('/'))
}

/// bash の `decide_api_tokens`: `-X`/`--method` が明示的に POST で、かつ
/// パスが repo 作成エンドポイントに一致するときだけ deny する(GET/PATCH/
/// DELETE 等はここで通す)。
pub fn decide_api_tokens(tokens: &[String]) -> Option<String> {
    let method = api_method(tokens)?;
    if !method.eq_ignore_ascii_case("POST") {
        return None; // 明示的な POST 指定が無ければ対象外
    }
    let path = api_path(tokens, is_repo_create_path)?; // 対象エンドポイントでない → 通す
    Some(deny_reason_repo_create_api(&path))
}

fn decide_range(r: &Range<'_, TargetKind>) -> Option<String> {
    match r.kind {
        TargetKind::Api => decide_api_tokens(r.tokens),
        // is_target_at で既に `gh repo create` に絞られている範囲なので、
        // body の有無に関わらず常に deny する。
        TargetKind::Cli => Some(deny_reason_repo_create()),
    }
}

/// bash の `decide`: Bash ツールのコマンド文字列全体。1 件でも deny なら理由文。
pub fn decide(cmd: &str) -> Option<String> {
    first_deny(cmd, is_target_at, decide_range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_create_paths() {
        for p in ["/user/repos", "/orgs/acme/repos"] {
            assert!(is_repo_create_path(p), "{p}");
        }
        for p in [
            "/orgs//repos",
            "/orgs/a/b/repos",
            "/orgs/acme/repos/x",
            "/user/repos/x",
            "/repos/o/r",
            "/orgs/acme",
        ] {
            assert!(!is_repo_create_path(p), "{p}");
        }
    }

    #[test]
    fn method_is_case_insensitive_and_explicit() {
        assert!(decide("gh api -X post user/repos").is_some());
        assert!(decide("gh api --method=POST /orgs/acme/repos").is_some());
        assert!(decide("gh api user/repos -f name=x").is_none());
    }
}
