//! renovate ドメイン(#3、#465、ADR-568 D4)。

use crate::jq;
use crate::model::{Finding, RepoGql};
use serde_json::Value;

/// `(.extends // []) | index($p) != null`。extends が文字列なら jq の
/// `index` は部分文字列検索になる(bash 版の挙動どおり)。
fn extends_has(config: &Value, needle: &str) -> bool {
    match config.get("extends") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(needle)),
        Some(Value::String(s)) => s.contains(needle),
        _ => false,
    }
}

/// bash: `judge_renovate`。`preset` は extends に要る浮動参照
/// (`Config::renovate_policy_preset`)。
pub fn judge_renovate(gql: &RepoGql, primary_language: &str, preset: &str) -> Finding {
    // Typst リポジトリは自前の manifest を持たない — linguist の
    // primaryLanguage が、全ソースを取らずに使える唯一の適用可否シグナル
    // (typst-repo-governance で播いた全リポジトリが偽 not-applicable だった)。
    let has_manifest = gql.cargo_toml.is_some()
        || gql.package_json.is_some()
        || gql.pyproject_toml.is_some()
        || gql.go_mod.is_some()
        || gql.flake_nix.is_some()
        || primary_language == "Typst";
    if !has_manifest || !gql.has_workflows() {
        return Finding::not_applicable();
    }
    let configs = [
        &gql.renovate_json,
        &gql.renovate_json5,
        &gql.gh_renovate_json,
        &gql.gh_renovate_json5,
        &gql.renovaterc,
    ];
    if configs.iter().all(|c| c.is_none()) {
        return Finding::ok_or_drifted(vec!["renovate-config-missing".into()]);
    }

    // #465: 設定はあるのに Renovate App が一度も動いた形跡が無い(Dependency
    // Dashboard Issue が無い)サイレント失敗を検出する。App のインストール
    // 範囲は gh の OAuth token では読めない(/user/installations は 403)ので
    // この代理指標を使う。Renovate の ensureDependencyDashboard() は schedule
    // を経ずに毎 run の末尾で無条件に呼ばれる(lib/workers/repository/index.ts、
    // https://github.com/renovatebot/renovate、取得 2026-09-25)。
    // `dependencyDashboard: false` を明示しているリポジトリは対象外。
    let config_text = jq::sh_trim(
        configs
            .iter()
            .find_map(|c| c.as_ref().and_then(|b| b.text.as_deref()))
            .unwrap_or(""),
    )
    .to_string();
    let parsed: Option<Value> = serde_json::from_str(&config_text).ok();
    let mut dashboard_disabled =
        parsed.as_ref().and_then(|v| v.get("dependencyDashboard")) == Some(&Value::Bool(false));
    // `:disableDependencyDashboard` プリセット省略形(https://docs.renovatebot.com/
    // presets-default/#disabledependencydashboard、2026-09-29 確認)は
    // `dependencyDashboard: false` を書かずに同じ効果を持つ。構造的検査の後に
    // 引用符付き文字列の一致で JSON5 も拾う(下の preset 検査と同じ形)。
    if !dashboard_disabled {
        dashboard_disabled = parsed
            .as_ref()
            .is_some_and(|v| extends_has(v, ":disableDependencyDashboard"));
    }
    if !dashboard_disabled {
        dashboard_disabled = config_text.contains("\":disableDependencyDashboard\"");
    }
    let dashboard_count = gql
        .renovate_dashboard
        .as_ref()
        .and_then(|d| d.total_count)
        .unwrap_or(0);

    // D4: 共有 automerge policy preset を extends すること。JSON5 等で
    // 構造的に読めなければ、参照を自身の JSON 文字列の引用符ごと固定文字列で
    // 探す — 素の部分文字列一致だと `#tag` 固定の参照(浮動参照はその接頭辞)
    // にも一致し、D4 のピン留め禁止を黙って破る。
    let preset_ok = parsed.as_ref().is_some_and(|v| extends_has(v, preset))
        || config_text.contains(&format!("\"{preset}\""));

    let mut missing = Vec::new();
    if !dashboard_disabled && dashboard_count == 0 {
        missing.push("renovate-dashboard-missing".into());
    }
    if !preset_ok {
        missing.push("renovate-policy-preset-missing".into());
    }
    Finding::ok_or_drifted(missing)
}
