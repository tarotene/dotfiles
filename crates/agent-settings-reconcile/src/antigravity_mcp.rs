//! Antigravity CLI(`agy`)の `~/.gemini/config/mcp_config.json` の MCP サーバーの reconcile。
//!
//! ファイルは `{"mcpServers": {<名前>: {...}}}` の形で、`agy mcp add`・`remove`・`enable`・`disable` も
//! 書く(agy 1.5.17 の `agy mcp --help` と同梱の `agy-customizations/docs/mcp_servers.md`、取得 2026-11-27)。
//! Claude の `claude-mcp-servers`(宣言を user scope の完全な望ましい状態として、宣言外を消す)と違い、
//! **所有の記録に載せた名前だけ**を管理する: 前回書いたが今回の宣言に無いサーバーを消し、`agy mcp add` で
//! 本人が足したサーバーは残す(agy の `mcp_config.json` は、本人が実行時に足すことを前提にした面なので)。
//! 宣言したサーバーは、宣言したキーを既存の値の上に書く(`agy mcp disable` が足す `disabled` など、
//! 宣言していないキーは残る)。
//!
//! 許すキーは文書の `command`・`args`・`env`・`cwd`・`url`・`headers`・`disabled`・`variables`。ほかのキー
//! (Claude の `type`・`timeout` など)は agy が黙って捨てる(`migrate-agent-settings` の変換の規則)ので、
//! 宣言に残っていれば、ファイルを書く前に止める。

use crate::hooks::Managed;
use crate::json::J;
use serde::Deserialize;

const ALLOWED: [&str; 8] = [
    "command",
    "args",
    "env",
    "cwd",
    "url",
    "headers",
    "disabled",
    "variables",
];

/// 所有の記録の種別。記録の要素は `(種別, 名前)`。
const KIND: &str = "mcp-server";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct McpSpec {
    /// サーバー名 → 設定。
    pub servers: J,
    /// 所有の記録のファイル。
    #[serde(default)]
    pub state: Option<String>,
}

fn servers(spec: &McpSpec) -> Result<&indexmap::IndexMap<String, J>, String> {
    spec.servers
        .as_obj()
        .ok_or_else(|| "servers: サーバー名 → 設定のオブジェクトが必要".to_owned())
}

/// 宣言を、文書の形と照らして確かめる。
pub fn validate(spec: &McpSpec) -> Result<(), String> {
    for (name, server) in servers(spec)? {
        if name.trim().is_empty() {
            return Err("サーバー名が空".to_owned());
        }
        let obj = server
            .as_obj()
            .ok_or_else(|| format!("{name}: 設定はオブジェクト"))?;
        for key in obj.keys() {
            if !ALLOWED.contains(&key.as_str()) {
                return Err(format!(
                    "{name}: agy の mcp_config.json に無いキー `{key}`(許すのは {})",
                    ALLOWED.join("・")
                ));
            }
        }
        let has = |k: &str| {
            obj.get(k)
                .and_then(J::as_str)
                .is_some_and(|s| !s.is_empty())
        };
        match (has("command"), has("url")) {
            (true, false) | (false, true) => {}
            _ => {
                return Err(format!(
                    "{name}: `command`(stdio)か `url`(リモート)のどちらか 1 つが要る"
                ))
            }
        }
    }
    Ok(())
}

/// 今回の宣言を、所有の記録の形にする。
pub fn declared(spec: &McpSpec) -> Managed {
    servers(spec)
        .map(|m| m.keys().map(|n| (KIND.to_owned(), n.clone())).collect())
        .unwrap_or_default()
}

/// `root`(mcp_config.json の中身)の `mcpServers` を、宣言に合わせる。
pub fn apply(root: &mut J, spec: &McpSpec, previous: Option<&Managed>) -> Result<(), String> {
    validate(spec)?;
    let wanted = servers(spec)?;
    let top = root
        .as_obj_mut()
        .ok_or("mcp_config.json の最上位がオブジェクトでない")?;
    let stale: Vec<String> = previous
        .map(|p| {
            p.iter()
                .filter(|(k, n)| k == KIND && !wanted.contains_key(n))
                .map(|(_, n)| n.clone())
                .collect()
        })
        .unwrap_or_default();
    if wanted.is_empty() && stale.is_empty() {
        return Ok(()); // 何も宣言せず、消すものも無い。ファイルに `mcpServers` を作らない。
    }
    let entry = top
        .entry("mcpServers".to_owned())
        .or_insert_with(|| J::Obj(Default::default()));
    if matches!(entry, J::Null) {
        *entry = J::Obj(Default::default());
    }
    let map = entry
        .as_obj_mut()
        .ok_or(".mcpServers がオブジェクトではない")?;
    for name in &stale {
        if map.shift_remove(name).is_some() {
            eprintln!("agent-settings-reconcile: 宣言から外れた MCP サーバーを agy の設定から消します: {name}");
        }
    }
    for (name, server) in wanted {
        let J::Obj(fields) = server else { continue };
        match map.get_mut(name) {
            Some(J::Obj(existing)) => {
                for (k, v) in fields {
                    existing.insert(k.clone(), v.clone());
                }
            }
            _ => {
                map.insert(name.clone(), server.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn spec(servers: &str) -> McpSpec {
        serde_json::from_str(&format!(r#"{{"servers":{servers}}}"#)).unwrap()
    }

    fn run(before: &str, s: &McpSpec, prev: Option<&Managed>) -> Result<J, String> {
        let mut root = parse(before).unwrap();
        apply(&mut root, s, prev)?;
        Ok(root)
    }

    const ONE: &str = r#"{"playwright":{"command":"/x/launcher","args":["--a"],"env":{"K":"v"}}}"#;

    #[test]
    fn an_empty_file_gets_the_declared_servers() {
        let after = run("{}", &spec(ONE), None).unwrap();
        assert_eq!(
            after.as_obj().unwrap()["mcpServers"].as_obj().unwrap()["playwright"],
            parse(r#"{"command":"/x/launcher","args":["--a"],"env":{"K":"v"}}"#).unwrap()
        );
    }

    #[test]
    fn servers_the_user_added_are_kept_and_declared_keys_win() {
        let before = r#"{"mcpServers":{
            "mine":{"command":"m"},
            "playwright":{"command":"old","disabled":true}}}"#;
        let after = run(before, &spec(ONE), None).unwrap();
        let m = after.as_obj().unwrap()["mcpServers"].as_obj().unwrap();
        assert!(m.contains_key("mine"));
        let p = m["playwright"].as_obj().unwrap();
        assert_eq!(p["command"].as_str(), Some("/x/launcher"));
        // 宣言していないキー(`agy mcp disable` が足したもの)は残る。
        assert!(p.contains_key("disabled"));
    }

    #[test]
    fn a_server_dropped_from_the_declaration_is_removed_only_if_it_was_ours() {
        let before = r#"{"mcpServers":{"gone":{"command":"g"},"mine":{"command":"m"}}}"#;
        let prev: Managed = [(KIND.to_owned(), "gone".to_owned())].into_iter().collect();
        let after = run(before, &spec("{}"), Some(&prev)).unwrap();
        let keys: Vec<_> = after.as_obj().unwrap()["mcpServers"]
            .as_obj()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["mine"]);
    }

    #[test]
    fn nothing_declared_and_nothing_to_remove_leaves_the_file_alone() {
        let after = run("{}", &spec("{}"), None).unwrap();
        assert_eq!(after, parse("{}").unwrap());
    }

    #[test]
    fn applying_twice_changes_nothing() {
        let once = run("{}", &spec(ONE), None).unwrap();
        let mut twice = once.clone();
        apply(&mut twice, &spec(ONE), None).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_key_agy_would_drop_or_a_malformed_server_is_rejected() {
        for bad in [
            r#"{"a":{"command":"x","type":"stdio"}}"#,
            r#"{"a":{"command":"x","timeout":3}}"#,
            r#"{"a":{}}"#,
            r#"{"a":{"command":"x","url":"https://h/mcp"}}"#,
            r#"{"a":{"command":""}}"#,
            r#"{"a":"x"}"#,
            r#"{" ":{"command":"x"}}"#,
        ] {
            assert!(run("{}", &spec(bad), None).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_remote_server_needs_only_a_url() {
        assert!(run(
            "{}",
            &spec(r#"{"r":{"url":"https://h/mcp","headers":{"A":"b"}}}"#),
            None
        )
        .is_ok());
    }

    #[test]
    fn declared_lists_the_names() {
        let d = declared(&spec(ONE));
        assert!(d.contains(&(KIND.to_owned(), "playwright".to_owned())));
    }
}
