//! 依存グラフの不変条件: 機能 crate は、他の機能 crate に依存しない。
//!
//! 機能 crate(hook・CLI)が別の機能 crate の関数を呼ぶと、その機能の変更が呼び出し側の挙動を黙って変える。
//! 共有の仕事は、`docs/agents/agent-libraries.md` のライブラリが持つ。依存してよいのは次の 3 つだけ。
//!
//! - **ライブラリ**([`LIBRARIES`])への依存。名前で列挙する(依存される目的の crate)。
//! - **集約役**([`AGGREGATORS`])からの依存。複数の機能を 1 つの入口にまとめる crate で、名前で列挙する。
//! - **既知の例外**([`EXCEPTIONS`])。理由つきで、1 件ずつ列挙する。解消したら、例外のほうが落ちる
//!   (鮮度の検査)。新しい違反を足すには、例外に書くか、共有の仕事をライブラリへ移す。
//!
//! 対象は `[dependencies]` と `[build-dependencies]` の `path` の依存(workspace 内)。`[dev-dependencies]` は
//! 試験の道具なので対象外。

use crate::manifest::Violation;
use std::collections::BTreeSet;
use toml::{Table, Value};

/// 他の crate が使うために在る crate(共有のライブラリ。`docs/agents/agent-libraries.md` の責務表と、
/// 同じ目的の crate)。
pub const LIBRARIES: &[&str] = &[
    // 責務表の 10 ライブラリ
    "agent-session",
    "civil-time",
    "command-inspection",
    "git-repository",
    "github-command",
    "herdr-client",
    "jq-json-compat",
    "process-control",
    "process-env",
    "vault-note",
    // 同じ目的の共有 crate
    "agent-observations",
    "backup-runtime",
    "hub-resolve",
    "model-catalog",
    // 他リポジトリの CI が 1 crate ずつ sparse-checkout する、依存ゼロの crate(`manifest::ZERO_DEPENDENCY_CRATES`)
    "pr-contract-check",
    "pr-title-check",
];

/// 複数の機能を 1 つの入口にまとめる crate。依存は自由。
pub const AGGREGATORS: &[&str] = &[
    "hook-router",
    "session-wrapup",
    // 旧入口の互換 bin(`session-wrapup` と `issue-inbox` へ転送する薄い bin)。互換入口の削除(#1493)で消える。
    "wrapup-stop-gate",
];

/// 既知の例外 `(依存する crate, 依存される crate, 理由)`。解消したら消す(鮮度の検査が落ちる)。
pub const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "cmd-hash-log",
        "issue-inbox",
        "兆候の語彙(`codify::Vocabulary`)を読む。語彙の読みをライブラリへ移せば消える",
    ),
    (
        "codex-plan",
        "codex-launcher",
        "Codex の起動(`Launch`・`command`)。起動の組み立てを共有のライブラリへ移せば消える",
    ),
    (
        "codex-plan-gate",
        "new-tool-guard",
        "計画の `既存手段:` 行の台帳への記録(`hook::record_plan_lines`・`skipped`)",
    ),
    (
        "detect-drift",
        "update-own-tools",
        "自前の道具の台帳(`Registry`・`parse_registry`)の読み",
    ),
    (
        "gate-failopen-advisory",
        "ja-polish",
        "ja-polish の記録(`records::Entry`・`transient_rate`)と設定の場所の読み(課金の検討の知らせ、ADR-1387 の D14)。記録の読みを共有のライブラリへ移せば消える",
    ),
    (
        "github-app-snapshot",
        "github-audit",
        "jq 互換の読み(`jq::j_get`・`parse_ordered`)。`jq-json-compat` へ移せば消える",
    ),
    (
        "pr-gate",
        "new-tool-guard",
        "新しい道具・単位の判定(`is_new_tool_unit`)と台帳(G_prior)",
    ),
    (
        "pr-gate",
        "worktree-lease",
        "他のセッションの worktree の貸し出しの確認(`held_by_other`)",
    ),
    (
        "promotion-detect",
        "due-remind",
        "期限の通知の理由の読み(`find_shown_reason`)",
    ),
    (
        "promotion-detect",
        "issue-inbox",
        "兆候の語彙と、暦日の変換(`vault::jst_date` など。`civil-time` へ移せる)",
    ),
    (
        "promotion-detect",
        "record-layer",
        "記録の層の宣言の検査(`check_files`・`drift`)",
    ),
    (
        "session-retrospective",
        "cmd-hash-log",
        "コマンドの痕跡の置き場の解決(`resolve_log_path`)",
    ),
    (
        "session-retrospective",
        "issue-inbox",
        "気づきの追加・兆候の語彙・状態の置き場",
    ),
    (
        "session-retrospective",
        "verdict-escalate",
        "判定の記録の置き場の読み(`ledger_dir`・`read_session_records`)",
    ),
    (
        "stack-base-guard",
        "pr-gate",
        "stack の連鎖の判定(`gates::stack`)。`github-command` へ移せば消える",
    ),
];

/// workspace 内の依存の辺 `(依存する crate, 依存される crate)`。`[dependencies]` と `[build-dependencies]`
/// の `path` の依存だけ(`[target.<cfg>.…]` も)。`members` は `(crate 名, manifest の文字列)`。
pub fn edges(members: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, text) in members {
        let Ok(manifest) = text.parse::<Table>() else {
            continue;
        };
        let mut tables: Vec<&Table> = Vec::new();
        for key in ["dependencies", "build-dependencies"] {
            if let Some(Value::Table(t)) = manifest.get(key) {
                tables.push(t);
            }
        }
        if let Some(Value::Table(targets)) = manifest.get("target") {
            for target in targets.values() {
                for key in ["dependencies", "build-dependencies"] {
                    if let Some(Value::Table(t)) = target.get(key) {
                        tables.push(t);
                    }
                }
            }
        }
        for table in tables {
            for spec in table.values() {
                let Some(path) = spec.get("path").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(dep) = path.strip_prefix("../") {
                    out.push((name.clone(), dep.trim_end_matches('/').to_string()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn violation(subject: &str, detail: impl Into<String>) -> Violation {
    Violation {
        subject: subject.to_string(),
        detail: detail.into(),
    }
}

/// 辺の一覧を検査する。`members` は workspace のメンバー名(列挙した名前が実在するかの検査に使う)。
pub fn check(edges: &[(String, String)], members: &[String]) -> Vec<Violation> {
    let mut out = Vec::new();
    let member_set: BTreeSet<&str> = members.iter().map(String::as_str).collect();
    for name in LIBRARIES.iter().chain(AGGREGATORS) {
        if !member_set.contains(name) {
            out.push(violation(
                name,
                "列挙した crate が workspace に無い(列挙から外す)",
            ));
        }
    }
    for (from, to) in edges {
        let allowed = LIBRARIES.contains(&to.as_str())
            || AGGREGATORS.contains(&from.as_str())
            || EXCEPTIONS.iter().any(|(f, t, _)| f == from && t == to);
        if !allowed {
            out.push(violation(
                from,
                format!(
                    "機能 crate `{to}` に依存している(機能 crate は、他の機能 crate に依存しない)。共有の仕事を `docs/agents/agent-libraries.md` のライブラリへ移すか、`deps::EXCEPTIONS` に理由つきで足す"
                ),
            ));
        }
    }
    for (from, to, _) in EXCEPTIONS {
        if !edges.iter().any(|(f, t)| f == from && t == to) {
            out.push(violation(
                from,
                format!("例外 `{from} → {to}` は、もう存在しない(`deps::EXCEPTIONS` から消す)"),
            ));
        }
        if LIBRARIES.contains(to) || AGGREGATORS.contains(from) {
            out.push(violation(
                from,
                format!("例外 `{from} → {to}` は不要(ライブラリへの依存か、集約役からの依存で、元から許されている)"),
            ));
        }
    }
    out
}

/// 実際のツリーを検査する。
pub fn check_tree(root: &std::path::Path) -> Result<Vec<Violation>, String> {
    let members = crate::manifest::read_members(root)?;
    let names: Vec<String> = members.iter().map(|(n, _)| n.clone()).collect();
    Ok(check(&edges(&members), &names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, deps: &[&str]) -> (String, String) {
        let mut text = format!("[package]\nname = \"{name}\"\n[dependencies]\n");
        for d in deps {
            text.push_str(&format!(
                "{d} = {{ path = \"../{d}\", version = \"0.1.0\" }}\n"
            ));
        }
        (name.to_string(), text)
    }

    fn names(m: &[(String, String)]) -> Vec<String> {
        m.iter().map(|(n, _)| n.clone()).collect()
    }

    #[test]
    fn edges_read_path_dependencies_only_from_the_non_dev_tables() {
        let text = r#"
[dependencies]
serde = { workspace = true }
a = { path = "../a", version = "0.1.0" }
[dev-dependencies]
d = { path = "../d" }
[build-dependencies]
b = { path = "../b" }
[target.'cfg(unix)'.dependencies]
t = { path = "../t" }
"#;
        let e = edges(&[("x".to_string(), text.to_string())]);
        let to: Vec<&str> = e.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(to, ["a", "b", "t"]);
    }

    #[test]
    fn a_feature_crate_depending_on_another_feature_crate_is_a_violation() {
        let mut m = vec![
            member("feature-a", &["feature-b"]),
            member("feature-b", &[]),
        ];
        // 列挙した crate が実在しないという別の違反を避けるため、列挙の名前を足す。
        for n in LIBRARIES.iter().chain(AGGREGATORS) {
            m.push(member(n, &[]));
        }
        // 例外の鮮度の違反は、この試験の関心の外なので、辺を足して満たす。
        let mut e = edges(&m);
        for (f, t, _) in EXCEPTIONS {
            e.push((f.to_string(), t.to_string()));
        }
        let v = check(&e, &names(&m));
        let own: Vec<&Violation> = v.iter().filter(|x| x.subject == "feature-a").collect();
        assert_eq!(own.len(), 1, "{v:?}");
        assert!(own[0].detail.contains("feature-b"), "{own:?}");
    }

    #[test]
    fn libraries_aggregators_and_listed_exceptions_are_allowed() {
        let mut m = vec![
            member("feature-a", &["process-env"]),
            member("hook-router", &["feature-a"]),
        ];
        for n in LIBRARIES.iter().chain(AGGREGATORS) {
            if !m.iter().any(|(x, _)| x == n) {
                m.push(member(n, &[]));
            }
        }
        let mut e = edges(&m);
        for (f, t, _) in EXCEPTIONS {
            e.push((f.to_string(), t.to_string()));
        }
        let v = check(&e, &names(&m));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn a_resolved_exception_is_itself_a_violation() {
        let mut m = Vec::new();
        for n in LIBRARIES.iter().chain(AGGREGATORS) {
            m.push(member(n, &[]));
        }
        // 例外の辺が 1 つも無い
        let v = check(&edges(&m), &names(&m));
        assert_eq!(v.len(), EXCEPTIONS.len(), "{v:?}");
        assert!(
            v.iter().all(|x| x.detail.contains("もう存在しない")),
            "{v:?}"
        );
    }

    #[test]
    fn an_unlisted_library_name_is_a_violation() {
        let v = check(&[], &["only-this".to_string()]);
        assert!(
            v.iter().any(|x| x.detail.contains("workspace に無い")),
            "{v:?}"
        );
    }
}
