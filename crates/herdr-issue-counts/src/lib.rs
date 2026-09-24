//! herdr-issue-counts のコアロジック(純粋関数のみ)。
//!
//! herdr のサイドバー workspace 行に、その workspace のリポジトリの open Issue
//! 数(PR を除く)を `$issues` トークンとして出す。プロセス起動(herdr / git /
//! gh)と exit code は main.rs が担い、ここはパースと組み立てだけを持つ。
//! 設計: docs/claude/herdr-sidebar-metadata.md「`$issues`」節。
use std::collections::BTreeMap;

use serde::Deserialize;

/// サイドバーに出すトークン名(config/herdr/config.toml の `$issues`)。
pub const TOKEN: &str = "issues";
/// workspace.report_metadata の source。他の reporter とトークン名を
/// 共有しないので、source を分けるのは消去(`--clear-token`)の範囲を
/// 自分の値に限るため。
pub const SOURCE: &str = "issue-counts";
/// 15 分 = タイマー間隔(5 分)の 3 周期。タイマーが止まれば herdr 自身が
/// 値を消すので、ローカルキャッシュは持たない。
pub const TTL_MS: u64 = 15 * 60 * 1000;
/// nf-oct-issue_opened(FiraCode Nerd Font に収録、モノクロ字形なので fg が効く)。
pub const ICON: char = '\u{f41b}';

/// Issue 数を出す対象の workspace(herdr が worktree 情報を付けたものだけ)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub workspace_id: String,
    pub repo_root: String,
}

#[derive(Deserialize)]
struct ListEnvelope {
    result: ListResult,
}

#[derive(Deserialize)]
struct ListResult {
    workspaces: Vec<RawWorkspace>,
}

#[derive(Deserialize)]
struct RawWorkspace {
    workspace_id: String,
    worktree: Option<RawWorktree>,
}

#[derive(Deserialize)]
struct RawWorktree {
    repo_root: String,
}

/// `herdr workspace list` の出力(`{"result":{"workspaces":[...]}}`)から、
/// `worktree.repo_root` を持つ workspace だけを返す。worktree を持たない
/// workspace(git 外で開いたもの)はリポジトリを判定できないので対象外。
pub fn parse_workspace_list(json: &str) -> Result<Vec<Workspace>, String> {
    let env: ListEnvelope = serde_json::from_str(json)
        .map_err(|e| format!("herdr workspace list の出力を解釈できない: {e}"))?;
    Ok(env
        .result
        .workspaces
        .into_iter()
        .filter_map(|w| {
            w.worktree.map(|t| Workspace {
                workspace_id: w.workspace_id,
                repo_root: t.repo_root,
            })
        })
        .collect())
}

/// GitHub のリポジトリ識別子。GraphQL クエリへ文字列として埋め込むので、
/// 構築は `parse_github_url` 経由に限り、使える文字を GitHub の名前規則の
/// 範囲(英数字と `-` `_` `.`)に閉じる。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Repo {
    owner: String,
    name: String,
}

impl Repo {
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// remote URL から owner/repo を取り出す。https(`https://github.com/o/r.git`)と
/// scp 形式 ssh(`git@github.com:o/r.git`)、`ssh://git@github.com/o/r` を受ける。
/// github.com 以外は `None`。
///
/// config/claude/hooks/issue-index.sh の `owner_repo()` と同じ判定だが、
/// あちらの正規表現はリポジトリ名の `.` を許さない(`foo.github.io` が
/// 対象外になる)。ここでは GitHub の名前規則どおり `.` を許す。
pub fn parse_github_url(url: &str) -> Option<Repo> {
    let idx = url.find("github.com")?;
    let before = &url[..idx];
    // `notgithub.com` のようなホスト名の部分一致を弾く。
    if !(before.is_empty() || before.ends_with('/') || before.ends_with('@')) {
        return None;
    }
    let rest = url[idx + "github.com".len()..].trim_start_matches([':', '/']);
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() || !valid_segment(owner) || !valid_segment(name) {
        return None;
    }
    Some(Repo {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// `git remote -v` の出力から GitHub リポジトリを選ぶ。`origin` が GitHub なら
/// それを優先し、無ければ最初に現れた GitHub remote を使う。
pub fn repo_from_remotes(remote_v: &str) -> Option<Repo> {
    let mut first = None;
    for line in remote_v.lines() {
        let mut fields = line.split_whitespace();
        let (Some(remote), Some(url)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Some(repo) = parse_github_url(url) else {
            continue;
        };
        if remote == "origin" {
            return Some(repo);
        }
        first.get_or_insert(repo);
    }
    first
}

/// GraphQL のエイリアス名(`r0`, `r1`, ...)。`build_query` と
/// `parse_counts` が同じ規則を使う。
pub fn alias(i: usize) -> String {
    format!("r{i}")
}

/// 全リポジトリの open Issue 数を 1 リクエストで取るクエリ。
/// `issues(states: OPEN)` は PR を含まない(REST の `open_issues_count` は含む)。
pub fn build_query(repos: &[Repo]) -> String {
    let body: Vec<String> = repos
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "{}: repository(owner: \"{}\", name: \"{}\") {{ issues(states: OPEN) {{ totalCount }} }}",
                alias(i),
                r.owner,
                r.name
            )
        })
        .collect();
    format!("{{ {} }}", body.join(" "))
}

/// GraphQL 応答を alias → 件数(または取れなかった理由)に分解する。
///
/// `gh api graphql` は一部のリポジトリだけ解決できない(削除済み・権限なし)と
/// 非 0 で終わるが、stdout には `data`(失敗した alias は `null`)と `errors`
/// が両方入る。そのため成否は `data` の有無で判定し、alias 単位で扱う。
/// `data` 自体が無い応答だけを全体の失敗とする。
pub fn parse_counts(json: &str) -> Result<BTreeMap<String, Result<u64, String>>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("GraphQL 応答が JSON でない: {e}"))?;
    let data = v.get("data").and_then(|d| d.as_object()).ok_or_else(|| {
        let msg = v
            .pointer("/errors/0/message")
            .and_then(|m| m.as_str())
            .unwrap_or("data が無い");
        format!("GraphQL 応答に data が無い: {msg}")
    })?;

    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    if let Some(errors) = v.get("errors").and_then(|e| e.as_array()) {
        for e in errors {
            if let Some(a) = e.pointer("/path/0").and_then(|p| p.as_str()) {
                let msg = e
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("理由不明");
                reasons.insert(a.to_string(), msg.to_string());
            }
        }
    }

    Ok(data
        .iter()
        .map(|(a, repo)| {
            let count = repo
                .pointer("/issues/totalCount")
                .and_then(|n| n.as_u64())
                .ok_or_else(|| {
                    reasons
                        .get(a)
                        .cloned()
                        .unwrap_or_else(|| "totalCount が無い".to_string())
                });
            (a.clone(), count)
        })
        .collect())
}

/// サイドバーに出す値。0 も出す(「取得済みで 0 件」と「未取得」を区別する)。
pub fn format_token(count: u64) -> String {
    format!("{ICON} {count}")
}

/// 1 回の実行の結末。exit code は「配信の成否」だけを表す
/// (crates/detect-drift の `exit_code` と同じ考え方、#442)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 想定内の不在(herdr 未起動・gh 未認証・GitHub リポの workspace 無し)。
    /// 恒久的にこの状態のマシンで unit を failed に張り付かせない。
    Skipped,
    /// 全 workspace へ報告できた(一部リポジトリの解決失敗は含む)。
    Reported,
    /// GraphQL・ネットワーク・report-metadata の失敗。一過性なので次の
    /// 成功で failed は消える。
    Failed,
}

pub fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::Skipped | Outcome::Reported => 0,
        Outcome::Failed => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(o: &str, n: &str) -> Repo {
        Repo {
            owner: o.into(),
            name: n.into(),
        }
    }

    #[test]
    fn parse_workspace_list_keeps_only_worktree_workspaces() {
        let json = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[
            {"workspace_id":"w1","label":"dotfiles","worktree":{"repo_root":"/r/dotfiles","checkout_path":"/r/dotfiles"}},
            {"workspace_id":"w2","label":"scratch"},
            {"workspace_id":"w3","worktree":{"repo_root":"/r/dotfiles","checkout_path":"/w/x"}}
        ]}}"#;
        assert_eq!(
            parse_workspace_list(json).unwrap(),
            vec![
                Workspace {
                    workspace_id: "w1".into(),
                    repo_root: "/r/dotfiles".into()
                },
                Workspace {
                    workspace_id: "w3".into(),
                    repo_root: "/r/dotfiles".into()
                },
            ]
        );
    }

    #[test]
    fn parse_workspace_list_rejects_error_envelope() {
        let json =
            r#"{"id":"cli:workspace:list","error":{"code":"server_not_running","message":"x"}}"#;
        assert!(parse_workspace_list(json).is_err());
    }

    #[test]
    fn parse_github_url_accepts_https_ssh_and_suffixes() {
        for url in [
            "https://github.com/tarotene/dotfiles.git",
            "https://github.com/tarotene/dotfiles",
            "https://github.com/tarotene/dotfiles/",
            "git@github.com:tarotene/dotfiles.git",
            "ssh://git@github.com/tarotene/dotfiles.git",
        ] {
            assert_eq!(
                parse_github_url(url),
                Some(repo("tarotene", "dotfiles")),
                "{url}"
            );
        }
    }

    #[test]
    fn parse_github_url_allows_dots_in_repo_name() {
        assert_eq!(
            parse_github_url("https://github.com/o/o.github.io.git"),
            Some(repo("o", "o.github.io"))
        );
    }

    #[test]
    fn parse_github_url_rejects_other_hosts_and_bad_names() {
        for url in [
            "https://gitlab.com/o/r.git",
            "https://notgithub.com/o/r.git",
            "https://github.com/o",
            "https://github.com/o/r/extra",
            "https://github.com/o/r\"x",
            "https://github.com/../r",
        ] {
            assert_eq!(parse_github_url(url), None, "{url}");
        }
    }

    #[test]
    fn repo_from_remotes_prefers_origin() {
        let text = "fork\thttps://github.com/me/r.git (fetch)\n\
                    fork\thttps://github.com/me/r.git (push)\n\
                    origin\thttps://github.com/up/r.git (fetch)\n";
        assert_eq!(repo_from_remotes(text), Some(repo("up", "r")));
    }

    #[test]
    fn repo_from_remotes_falls_back_to_first_github_remote() {
        let text = "origin\thttps://gitlab.com/x/y.git (fetch)\n\
                    mirror\tgit@github.com:a/b.git (fetch)\n";
        assert_eq!(repo_from_remotes(text), Some(repo("a", "b")));
        assert_eq!(repo_from_remotes(""), None);
    }

    #[test]
    fn build_query_aliases_each_repo_in_order() {
        let q = build_query(&[repo("a", "b"), repo("c", "d.e")]);
        assert_eq!(
            q,
            "{ r0: repository(owner: \"a\", name: \"b\") { issues(states: OPEN) { totalCount } } \
             r1: repository(owner: \"c\", name: \"d.e\") { issues(states: OPEN) { totalCount } } }"
        );
    }

    #[test]
    fn parse_counts_splits_partial_errors_per_alias() {
        // gh api graphql が 1 件だけ解決できなかったときの実際の応答形
        // (2026-09-24 に存在しないリポジトリで実測)。
        let json = r#"{"data":{"r0":{"issues":{"totalCount":27}},"r1":null},"errors":[{"type":"NOT_FOUND","path":["r1"],"message":"Could not resolve to a Repository with the name 'o/nope'."}]}"#;
        let got = parse_counts(json).unwrap();
        assert_eq!(got["r0"], Ok(27));
        assert_eq!(
            got["r1"],
            Err("Could not resolve to a Repository with the name 'o/nope'.".into())
        );
    }

    #[test]
    fn parse_counts_fails_without_data() {
        let json = r#"{"errors":[{"message":"Bad credentials"}]}"#;
        let err = parse_counts(json).unwrap_err();
        assert!(err.contains("Bad credentials"), "{err}");
        assert!(parse_counts("not json").is_err());
    }

    #[test]
    fn format_token_shows_zero() {
        assert_eq!(format_token(0), "\u{f41b} 0");
        assert_eq!(format_token(27), "\u{f41b} 27");
    }

    #[test]
    fn exit_code_reflects_delivery_only() {
        assert_eq!(exit_code(Outcome::Skipped), 0);
        assert_eq!(exit_code(Outcome::Reported), 0);
        assert_eq!(exit_code(Outcome::Failed), 1);
    }
}
