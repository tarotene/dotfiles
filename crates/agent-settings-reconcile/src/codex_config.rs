//! `~/.codex/config.toml` の reconcile。
//!
//! このファイルは Codex 自身が実行時に書き戻す(hook trust の `hooks.state.*`、
//! project trust、モデル選択)ので、home-manager は read-only symlink にしない
//! (ADR-457 Amendment)。宣言したキーだけを常に宣言値へ合わせ、ほかのキー・
//! コメント・並び・空行には触らない(`toml_edit` が書式を保つ)。
//!
//! spec は「ドット区切りのキー → 値」のオブジェクト(JSON)。値は文字列・整数・
//! 真偽値と、それらだけの配列、または literal-key オブジェクト。キーの各区間は引用なしの名前に限る(ドットを含む
//! キー名は宣言できない — 今回の用途では要らない)。

use crate::json::write_atomic;
use serde_json::Value as Spec;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, Value};

/// `spec_json` は `{"tui.terminal_title": ["activity", ...], ...}`。
pub fn apply(path: &Path, spec_json: &str) -> Result<(), String> {
    let spec: Spec = serde_json::from_str(spec_json).map_err(|e| format!("spec-json: {e}"))?;
    let Spec::Object(spec) = spec else {
        return Err("spec-json: JSON オブジェクトが必要".into());
    };

    let before = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut doc: DocumentMut = before
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;

    for (dotted, want) in &spec {
        set_dotted(&mut doc, dotted, want)
            .map_err(|e| format!("{}: {dotted}: {e}", path.display()))?;
    }

    let after = doc.to_string();
    if after == before {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_atomic(path, &after)
}

fn set_dotted(doc: &mut DocumentMut, dotted: &str, want: &Spec) -> Result<(), String> {
    let segs: Vec<&str> = dotted.split('.').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Err("キーに空の区間がある".into());
    }
    let (leaf, parents) = segs.split_last().expect("split は 1 要素以上を返す");

    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for seg in parents {
        if !table.contains_key(seg) {
            // 途中の表は implicit にして、空の `[a]` 見出しを出さない。
            let mut t = Table::new();
            t.set_implicit(true);
            table.insert(seg, Item::Table(t));
        }
        table = table
            .get_mut(seg)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| format!("`{seg}` が表ではない"))?;
    }

    if let Spec::Object(children) = want {
        if !table.contains_key(leaf) {
            let mut t = Table::new();
            t.set_implicit(true);
            table.insert(leaf, Item::Table(t));
        }
        let target = table
            .get_mut(leaf)
            .and_then(Item::as_table_like_mut)
            .ok_or("existing value is not a table")?;
        return merge_object(target, children);
    }
    let new = to_toml(want)?;
    match table.get_mut(leaf) {
        Some(Item::Value(old)) => {
            if same(old, want) {
                return Ok(());
            }
            // 行末コメントなどの装飾は引き継ぐ。
            let mut new = new;
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        Some(_) => return Err("既存の値が単一の値ではない(表・表の配列)".into()),
        None => {
            table.insert(leaf, Item::Value(new));
        }
    }
    Ok(())
}

pub(crate) fn to_toml(v: &Spec) -> Result<Value, String> {
    match v {
        Spec::String(s) => Ok(Value::from(s.as_str())),
        Spec::Bool(b) => Ok(Value::from(*b)),
        Spec::Number(n) => n
            .as_i64()
            .map(Value::from)
            .ok_or_else(|| format!("整数だけ扱える: {n}")),
        Spec::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                if matches!(x, Spec::Array(_) | Spec::Object(_)) {
                    return Err("配列の要素は文字列・整数・真偽値に限る".into());
                }
                arr.push(to_toml(x)?);
            }
            Ok(Value::Array(arr))
        }
        Spec::Null | Spec::Object(_) => Err("null・オブジェクトは宣言できない".into()),
    }
}

/// 書式(空白・クォートの種類)に左右されず、値として同じかを見る。
fn same(have: &Value, want: &Spec) -> bool {
    match (have, want) {
        (Value::String(s), Spec::String(w)) => s.value() == w,
        (Value::Boolean(b), Spec::Bool(w)) => b.value() == w,
        (Value::Integer(i), Spec::Number(w)) => w.as_i64() == Some(*i.value()),
        (Value::Array(a), Spec::Array(w)) => {
            a.len() == w.len() && a.iter().zip(w).all(|(h, w)| same(h, w))
        }
        _ => false,
    }
}

// Object members are literal TOML keys (filesystem paths and server names may
// contain dots); only the outer spec keys use dotted-path notation.
fn merge_object(
    table: &mut dyn toml_edit::TableLike,
    values: &serde_json::Map<String, Spec>,
) -> Result<(), String> {
    for (key, want) in values {
        if let Spec::Object(children) = want {
            if !table.contains_key(key) {
                let mut t = Table::new();
                t.set_implicit(true);
                table.insert(key, Item::Table(t));
            }
            let child = table
                .get_mut(key)
                .and_then(Item::as_table_like_mut)
                .ok_or_else(|| format!("{key}: existing value is not a table"))?;
            merge_object(child, children)?;
        } else {
            if table
                .get(key)
                .and_then(Item::as_value)
                .is_some_and(|v| same(v, want))
            {
                continue;
            }
            let mut value = to_toml(want)?;
            if let Some(old) = table.get(key).and_then(Item::as_value) {
                *value.decor_mut() = old.decor().clone();
            }
            table.insert(key, Item::Value(value));
        }
    }
    Ok(())
}

/// Native permissions declaration, independent of the retired shared policy.
/// MCP names retain the existing custom-server mapping; absent servers are skipped.
/// Declared named profiles are exact; unrelated named profiles and runtime fields
/// survive. With a declared default selector, legacy sandbox selectors are removed.
/// Upstream rejects simultaneous selector overrides: openai/codex tree
/// 6aaac694803004a63c6f393d153c96d22521a6b9,
/// codex-rs/core/src/config/mod.rs:3482-3486 (retrieved 2026-10-27).
/// Destructive replacement has a private content-addressed before-image alongside
/// the config; the write reuses the migration CAS/readback primitive.
pub fn permissions(path: &Path, spec_json: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Declaration {
        settings: serde_json::Map<String, Spec>,
        mcp_allow: Vec<String>,
        /// 所有の記録のファイル。あれば、前回書いたが今回の宣言に無い MCP 許可を消す(#1161)。
        #[serde(default)]
        state: Option<String>,
    }
    let mut declaration: Declaration =
        serde_json::from_str(spec_json).map_err(|e| format!("spec-json: {e}"))?;
    let before = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut doc: DocumentMut = before.parse().map_err(|e| format!("{e}"))?;
    // 宣言から外した MCP 許可は、前回の記録にある分だけ消す。所有は記録だけで決める — 本人が
    // 手で足した許可や、承認方式を変えたツールは、記録に無い・`approve` でないので触らない。
    let state = declaration.state.as_deref().map(Path::new);
    let previous = state.and_then(crate::managed::load);
    if let Some(previous) = &previous {
        for (kind, entry) in previous {
            if kind == MCP_ALLOW && !declaration.mcp_allow.contains(entry) {
                retire_mcp_allowance(&mut doc, entry);
            }
        }
    }
    let mut servers = serde_json::Map::new();
    for entry in &declaration.mcp_allow {
        let (server, tool) = crate::agent_permissions::mcp_tool(entry)
            .ok_or_else(|| format!("invalid MCP tool: {entry}"))?;
        if server == "codex_apps" {
            eprintln!("agent-settings-reconcile: codex_apps tool {tool} needs explicit apps.<connector>.tools mapping; preserved existing app settings");
            continue;
        }
        if doc
            .get("mcp_servers")
            .and_then(Item::as_table_like)
            .and_then(|s| s.get(server))
            .and_then(Item::as_table_like)
            .is_none()
        {
            eprintln!(
                "agent-settings-reconcile: custom MCP server {server} absent; allowance skipped"
            );
            continue;
        }
        let server = servers
            .entry(server)
            .or_insert_with(|| serde_json::json!({"tools":{}}));
        server["tools"][tool] = serde_json::json!({"approval_mode":"approve"});
    }
    if !servers.is_empty() {
        declaration
            .settings
            .insert("mcp_servers".into(), Spec::Object(servers));
    }
    let mut replaced = false;
    if declaration.settings.contains_key("default_permissions") {
        // Named native profiles replace the legacy selector; do not leave a
        // second selector for the launcher or upstream override parser.
        replaced |= doc.remove("sandbox_mode").is_some();
        replaced |= doc.remove("sandbox_workspace_write").is_some();
    }
    if let Some(Spec::Object(profiles)) = declaration.settings.get("permissions") {
        if let Some(table) = doc.get_mut("permissions").and_then(Item::as_table_like_mut) {
            for (name, want) in profiles {
                if table.get(name).is_some_and(|have| !item_same(have, want)) {
                    // This one profile is declaration-owned: retained domain or
                    // filesystem entries would silently broaden its boundary.
                    table.remove(name);
                    replaced = true;
                }
            }
        }
    }
    for (key, want) in &declaration.settings {
        set_dotted(&mut doc, key, want)?;
    }
    write_permissions(path, &before, doc.to_string(), replaced)?;
    if let Some(state) = state {
        let declared: crate::hooks::Managed = declaration
            .mcp_allow
            .iter()
            .map(|e| (MCP_ALLOW.to_owned(), e.clone()))
            .collect();
        crate::managed::store_if_changed(state, previous.as_ref(), &declared)?;
    }
    Ok(())
}

/// 所有の記録の中で、MCP 許可を表す種別。
const MCP_ALLOW: &str = "mcp_allow";

/// `mcp__<server>__<tool>` に当たる `mcp_servers.<server>.tools.<tool>` を、`approval_mode =
/// "approve"` のときだけ消す。`tools` が空になったらそれも消す。サーバー自体は本人のものなので残す。
fn retire_mcp_allowance(doc: &mut DocumentMut, entry: &str) {
    let Some((server, tool)) = crate::agent_permissions::mcp_tool(entry) else {
        return;
    };
    if server == "codex_apps" {
        return;
    }
    let Some(tools) = doc
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .and_then(|servers| servers.get_mut(server))
        .and_then(Item::as_table_like_mut)
        .and_then(|server| server.get_mut("tools"))
        .and_then(Item::as_table_like_mut)
    else {
        return;
    };
    let approved = tools
        .get(tool)
        .and_then(Item::as_table_like)
        .and_then(|t| t.get("approval_mode"))
        .and_then(Item::as_str)
        == Some("approve");
    if approved {
        tools.remove(tool);
    }
    if tools.is_empty() {
        if let Some(server) = doc
            .get_mut("mcp_servers")
            .and_then(Item::as_table_like_mut)
            .and_then(|servers| servers.get_mut(server))
            .and_then(Item::as_table_like_mut)
        {
            server.remove("tools");
        }
    }
}

fn write_permissions(
    path: &Path,
    before: &str,
    after: String,
    replaced: bool,
) -> Result<(), String> {
    if after == before {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let observed = match std::fs::read(path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if observed.as_deref().unwrap_or_default() != before.as_bytes() {
        return Err(format!("concurrent modification: {}", path.display()));
    }
    crate::agent_permissions::transaction::reject_symlinks(path)?;
    if replaced && observed.is_some() {
        native_backup(path, before.as_bytes())?;
    }
    crate::agent_permissions::transaction::replace(path, &observed, &Some(after.into_bytes()))
}

fn item_same(have: &Item, want: &Spec) -> bool {
    if let Spec::Object(want) = want {
        return have.as_table_like().is_some_and(|table| {
            table.len() == want.len()
                && want
                    .iter()
                    .all(|(key, value)| table.get(key).is_some_and(|item| item_same(item, value)))
        });
    }
    have.as_value().is_some_and(|v| same(v, want))
}

fn native_backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let name = path
        .file_name()
        .ok_or("config has no filename")?
        .to_string_lossy();
    let backup = path.with_file_name(format!(
        "{name}.native-permissions-backup.{}",
        crate::agent_permissions::digest(bytes)
    ));
    crate::agent_permissions::transaction::reject_symlinks(&backup)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&backup)
    {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::read(&backup).map_err(|e| e.to_string())? != bytes {
                return Err("native permissions backup conflict".into());
            }
        }
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}
