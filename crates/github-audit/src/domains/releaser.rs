//! releaser ドメイン(grill-me セッション調べ、ADR-436 + Amendment
//! 2026-09-30、#613、#615、#633、#673)。
//!
//! releaser GitHub App(release-plz / release-please 用)の secret 配線と、
//! `github-app-snapshot` の snapshot があれば install 有無を見る。個人
//! アカウントには account-level の Actions secret が無い(repo/environment/org
//! のみ、org secret も Free プランでは private repo から読めない)ので、App を
//! 1 個に集約しても `RELEASER_APP_*` の repo ごとのコピーは還元できず、そこも
//! drift しうる。この domain 自身は secret を要求しない(ADR-436 D4)。

use crate::jq;
use crate::model::{Finding, RepoGql, Verdict};
use regex::Regex;
use std::sync::OnceLock;

/// releaser の workflow が読んでよい名前(#613)。これ以外の `RELEASE*` の
/// 資格情報(`vars.RELEASE_APP_ID`、`RELEASER_APP_*` 統一前の
/// `RELEASE_APP_*` 等)は `releaser-workflow-refs-nonstandard`。統一前の
/// `RELEASE_PLZ_APP_*` / `RELEASE_PLEASE_APP_*` は judge_releaser が
/// `releaser-secret-name-legacy` として別に報告するのでここでは許す。
/// #615: actions/create-github-app-token が `app-id` を非推奨にして
/// `client-id` に移ったので正準の ID は `RELEASER_APP_CLIENT_ID`。
/// `secrets.RELEASER_APP_ID` は読む名前としては許し、repo secret 側を
/// `releaser-app-id-deprecated`(未移行)/ `releaser-app-id-leftover`
/// (移行済みで残骸、advisory、#633)で報告する。
pub const RELEASER_CANONICAL_REFS: [&str; 7] = [
    "secrets.RELEASER_APP_ID",
    "secrets.RELEASER_APP_PRIVATE_KEY",
    "secrets.RELEASE_PLZ_APP_ID",
    "secrets.RELEASE_PLZ_APP_PRIVATE_KEY",
    "secrets.RELEASE_PLEASE_APP_ID",
    "secrets.RELEASE_PLEASE_APP_PRIVATE_KEY",
    "secrets.RELEASER_APP_CLIENT_ID",
];

/// release 系 workflow の慣用ファイル名(claim の判定と #673 の run 取得)。
pub const RELEASE_WORKFLOW_FILES: [&str; 2] = ["release-plz.yml", "release-please.yml"];

fn ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:app-id|client-id|private-key):[ \t]*\$\{\{[ \t]*((?:secrets|vars)\.[A-Za-z0-9_]+)[ \t]*\}\}",
        )
        .expect("valid regex")
    })
}

/// bash: `releaser_workflow_refs`。`actions/create-github-app-token` を使う
/// workflow が `app-id`/`client-id`/`private-key` に渡す `secrets.X` /
/// `vars.Y` のうち、名前に RELEASE を含むもの(大小無視、一意、バイト順)。
pub fn releaser_workflow_refs(gql: &RepoGql) -> Vec<String> {
    let mut out = Vec::new();
    for e in gql.workflow_entries() {
        let Some(text) = e.object.as_ref().and_then(|o| o.text.as_deref()) else {
            continue;
        };
        if !text.contains("actions/create-github-app-token") {
            continue;
        }
        for cap in ref_re().captures_iter(text) {
            let r = &cap[1];
            if r.to_ascii_lowercase().contains("release") {
                out.push(r.to_string());
            }
        }
    }
    jq::unique_strings(out)
}

/// ADR-436 D2 + #613: claim = 慣用のファイル名、または RELEASE* の資格情報を
/// 実際に読む workflow(release-pr.yml / release.yml という名前もありうる —
/// claim の対象はファイル名ではない)。
pub fn has_releaser_workflow(gql: &RepoGql, refs: &[String]) -> bool {
    !refs.is_empty()
        || RELEASE_WORKFLOW_FILES
            .iter()
            .any(|f| gql.has_workflow_file(f))
}

/// judge_releaser の入力(bash 版の位置引数 $1..$6)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReleaserInput {
    /// release workflow の claim があるか。
    pub has_workflow: bool,
    /// repo secret の名前一覧。
    pub secrets: Vec<String>,
    /// app-snapshot.json に releaser App があったか(無ければ install 判定を飛ばす)。
    pub snapshot_present: bool,
    /// snapshot 上この repo に install 済みか。
    pub installed: bool,
    /// `releaser_workflow_refs` の結果。
    pub refs: Vec<String>,
    /// release workflow の最新の完了 run の conclusion(無ければ空)。
    pub release_conclusion: String,
}

/// `printf '<head>\n%s\n' "${codes[@]}"`: printf は引数が余るたびに書式を
/// 使い回すので、codes が 2 つ以上だと head が codes ごとに繰り返される
/// (bash 版の挙動をそのまま再現する)。
fn printf_reuse(head: &str, codes: &[String]) -> Vec<String> {
    if codes.is_empty() {
        return vec![head.to_string()];
    }
    codes
        .iter()
        .flat_map(|c| [head.to_string(), c.clone()])
        .filter(|s| !s.is_empty())
        .collect()
}

/// bash: `judge_releaser`。
pub fn judge_releaser(input: &ReleaserInput) -> Finding {
    let has = |name: &str| input.secrets.iter().any(|s| s == name);
    let mut codes: Vec<String> = Vec::new();
    if input.snapshot_present && !input.installed {
        codes.push("releaser-app-not-installed".into());
    }
    if input.has_workflow {
        if input
            .refs
            .iter()
            .any(|r| !RELEASER_CANONICAL_REFS.contains(&r.as_str()))
        {
            codes.push("releaser-workflow-refs-nonstandard".into());
        }
        // #673: release workflow の最新の完了 run が失敗していれば、secret が
        // 揃っていても ok にしない(CI の成功に失敗が隠れる問題の検出)。
        if input.release_conclusion == "failure" {
            codes.push("releaser-release-failing".into());
        }
    }

    if !input.has_workflow {
        // #506/#588 の姉妹判定: release workflow が無いのに snapshot 上は
        // install 済み(過剰付与)なら info 級の advisory。
        return if input.snapshot_present && input.installed {
            Finding::plain(
                Verdict::Advisory,
                vec!["releaser-app-installed-unclaimed".into()],
            )
        } else {
            Finding::not_applicable()
        };
    }

    let has_id = has("RELEASER_APP_CLIENT_ID");
    let has_key = has("RELEASER_APP_PRIVATE_KEY");
    if has_id && has_key {
        // #633: 移行済みでも旧 RELEASER_APP_ID が残っていれば advisory。移行
        // PR の merge 前は workflow がまだ旧名を読むので drifted にはしないが、
        // 削除し忘れを ok に埋もれさせない。
        let leftover = has("RELEASER_APP_ID");
        return if !codes.is_empty() {
            let mut m = codes;
            if leftover {
                m.push("releaser-app-id-leftover".into());
            }
            Finding::plain(Verdict::Drifted, m)
        } else if leftover {
            Finding::plain(Verdict::Advisory, vec!["releaser-app-id-leftover".into()])
        } else {
            Finding::plain(Verdict::Ok, Vec::new())
        };
    }

    // #615: 数値 ID の RELEASER_APP_ID(非推奨の `app-id` 入力用)だけが
    // 残っていて Client ID が無い repo は「まだ移行していないだけ」。
    if has("RELEASER_APP_ID") && has_key {
        return Finding::plain(
            Verdict::Drifted,
            printf_reuse("releaser-app-id-deprecated", &codes),
        );
    }

    // 統合前の旧名(rust 系 RELEASE_PLZ_APP_*、astro 系 RELEASE_PLEASE_APP_*)。
    // 片方の組が丸ごと揃っている場合だけ「未移行」と区別し、それ以外は
    // 一律 missing。
    let has_legacy_id = has("RELEASE_PLZ_APP_ID") || has("RELEASE_PLEASE_APP_ID");
    let has_legacy_key =
        has("RELEASE_PLZ_APP_PRIVATE_KEY") || has("RELEASE_PLEASE_APP_PRIVATE_KEY");
    if has_legacy_id && has_legacy_key {
        return Finding::plain(
            Verdict::Drifted,
            printf_reuse("releaser-secret-name-legacy", &codes),
        );
    }

    Finding::plain(
        Verdict::Drifted,
        printf_reuse("releaser-app-secrets-missing", &codes),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_reuse_repeats_head() {
        assert_eq!(printf_reuse("h", &[]), vec!["h"]);
        assert_eq!(
            printf_reuse("h", &["a".into(), "b".into()]),
            vec!["h", "a", "h", "b"]
        );
    }
}
