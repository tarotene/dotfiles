//! Antigravity CLI(`agy`)の `~/.gemini/antigravity-cli/settings.json` の権限の reconcile。
//!
//! CLI は起動時と `/permissions`・承認のたびにこのファイルを自分の形式で書き直し、既定値と
//! 同じキーを消し、知らないキーは残す(Google, Antigravity docs "Settings"、
//! <https://antigravity.google/docs/settings.md>、取得 2026-11-26)。ファイル全体は配れないので、
//! Claude の `permissions`(`settings.rs`)と同じく、宣言したキーだけを合わせる:
//!
//! - `permissions.{allow,ask,deny}` は「宣言との和 − 前回の記録にあり今回の宣言に無いもの」。
//!   所有は所有の記録(`managed`)だけで決めるので、本人が `/permissions` や承認で足した規則は残る。
//! - `toolPermission`・`enableTerminalSandbox` は、宣言があるときだけ、値が違うときに書く。
//!   既定値と同じ値を宣言すると CLI が次の起動で消すので、nix 側は既定と違う値だけを渡す。
//!
//! 規則の書式 `action(target)` は CLI のスキーマ
//! (<https://antigravity.google/schemas/v1/settings.json> の `permissionRule`)に合わない文字列を
//! 黙って無視するので、ここで先に弾く(無視された規則は、配ったつもりで効かない)。

use crate::hooks::Managed;
use crate::json::J;
use indexmap::IndexMap;
use serde::Deserialize;

/// スキーマの `permissionRule` が許す action(`^(command|read_file|…)\(.*\)$`)。
const ACTIONS: [&str; 7] = [
    "command",
    "read_file",
    "write_file",
    "read_url",
    "execute_url",
    "mcp",
    "unsandboxed",
];

/// スキーマの `toolPermission` の列挙。
const TOOL_PERMISSIONS: [&str; 4] = [
    "request-review",
    "proceed-in-sandbox",
    "always-proceed",
    "strict",
];

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct AntigravitySpec {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub tool_permission: Option<String>,
    #[serde(default)]
    pub enable_terminal_sandbox: Option<bool>,
    /// 所有の記録のファイル。あれば、前回書いたが今回の宣言に無い規則を消し、今回の宣言を記録する。
    #[serde(default)]
    pub state: Option<String>,
}

/// 所有の記録の種別(`permissions` の配列名と同じ)。
const KINDS: [&str; 3] = ["allow", "ask", "deny"];

fn rules<'a>(spec: &'a AntigravitySpec, kind: &str) -> &'a [String] {
    match kind {
        "allow" => &spec.allow,
        "ask" => &spec.ask,
        _ => &spec.deny,
    }
}

/// 宣言した規則の集合(所有の記録に書く値)。
pub fn declared(spec: &AntigravitySpec) -> Managed {
    KINDS
        .iter()
        .flat_map(|k| {
            rules(spec, k)
                .iter()
                .map(move |r| ((*k).to_owned(), r.clone()))
        })
        .collect()
}

/// `action(target)` の形か。`action` は列挙のどれか、`(` の前に空白を置かず、`)` で終わる。
fn valid_rule(rule: &str) -> bool {
    ACTIONS.iter().any(|a| {
        rule.strip_prefix(a)
            .and_then(|r| r.strip_prefix('('))
            .is_some_and(|r| r.ends_with(')'))
    })
}

fn validate(spec: &AntigravitySpec) -> Result<(), String> {
    for kind in KINDS {
        for r in rules(spec, kind) {
            if !valid_rule(r) {
                return Err(format!(
                    "permissions.{kind} の規則が `action(target)` の形ではない(CLI は黙って無視する): {r}"
                ));
            }
        }
    }
    if let Some(t) = &spec.tool_permission {
        if !TOOL_PERMISSIONS.contains(&t.as_str()) {
            return Err(format!("toolPermission が列挙に無い: {t}"));
        }
    }
    Ok(())
}

pub fn apply(
    root: &mut J,
    spec: &AntigravitySpec,
    previous: Option<&Managed>,
) -> Result<(), String> {
    validate(spec)?;
    let top = root
        .as_obj_mut()
        .ok_or("トップレベルが JSON オブジェクトではない")?;

    for kind in KINDS {
        let declared = rules(spec, kind);
        let dropped = |s: &str| {
            previous.is_some_and(|p| {
                p.contains(&(kind.to_owned(), s.to_owned())) && !declared.iter().any(|d| d == s)
            })
        };
        // 宣言も前回の記録も無い種別には、配列を作らない(CLI は空の配列を書かない)。
        let has_record = previous.is_some_and(|p| p.iter().any(|(k, _)| k == kind));
        if declared.is_empty() && !has_record {
            continue;
        }
        let arr = rule_array(top, kind)?;
        arr.retain(|v| v.as_str().is_none_or(|s| !dropped(s)));
        for r in declared {
            if !arr.iter().any(|v| v.as_str() == Some(r)) {
                arr.push(J::str(r));
            }
        }
    }

    if let Some(t) = &spec.tool_permission {
        set_scalar(top, "toolPermission", J::str(t));
    }
    if let Some(b) = spec.enable_terminal_sandbox {
        set_scalar(top, "enableTerminalSandbox", J::Bool(b));
    }
    Ok(())
}

/// `permissions.<kind>`(文字列の配列)を返す。無い・null なら空で作る。
fn rule_array<'a>(top: &'a mut IndexMap<String, J>, kind: &str) -> Result<&'a mut Vec<J>, String> {
    if matches!(top.get("permissions"), None | Some(J::Null)) {
        top.insert("permissions".into(), J::Obj(IndexMap::new()));
    }
    let Some(J::Obj(perms)) = top.get_mut("permissions") else {
        return Err(".permissions がオブジェクトではない".into());
    };
    if matches!(perms.get(kind), None | Some(J::Null)) {
        perms.insert(kind.to_owned(), J::Arr(Vec::new()));
    }
    match perms.get_mut(kind) {
        Some(J::Arr(a)) => Ok(a),
        _ => Err(format!(".permissions.{kind} が配列ではない")),
    }
}

fn set_scalar(top: &mut IndexMap<String, J>, key: &str, value: J) {
    if top.get(key) != Some(&value) {
        top.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_shape_follows_the_schema_pattern() {
        for ok in [
            "command(git status)",
            "read_file(/home/u/.ssh)",
            "read_url(github.com)",
            "mcp(srv/*)",
            "command(*)",
            "unsandboxed(x)",
        ] {
            assert!(valid_rule(ok), "{ok}");
        }
        for bad in [
            "command (git status)",
            "Bash(git status)",
            "command(git status",
            "command",
            "run(x)",
            "",
        ] {
            assert!(!valid_rule(bad), "{bad}");
        }
    }

    #[test]
    fn declared_collects_every_kind() {
        let spec = AntigravitySpec {
            allow: vec!["command(a)".into()],
            ask: vec!["command(b)".into()],
            deny: vec!["read_file(/c)".into()],
            ..Default::default()
        };
        let d = declared(&spec);
        assert!(d.contains(&("allow".into(), "command(a)".into())));
        assert!(d.contains(&("ask".into(), "command(b)".into())));
        assert!(d.contains(&("deny".into(), "read_file(/c)".into())));
        assert_eq!(d.len(), 3);
    }
}
