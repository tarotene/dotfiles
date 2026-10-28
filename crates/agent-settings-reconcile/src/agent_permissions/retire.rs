//! Invert recorded changes without overwriting subsequent owner edits.
use super::{digest, read, state_path, transaction};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

const RETIRED: &[u8] = b"{\"retired\":true,\"version\":1}\n";

fn value(item: &Item) -> Value {
    if let Some(t) = item.as_table_like() {
        return Value::Object(t.iter().map(|(k, v)| (k.into(), value(v))).collect());
    }
    let Some(v) = item.as_value() else {
        return Value::Null;
    };
    if let Some(s) = v.as_str() {
        json!(s)
    } else if let Some(b) = v.as_bool() {
        json!(b)
    } else if let Some(n) = v.as_integer() {
        json!(n)
    } else if let Some(a) = v.as_array() {
        Value::Array(a.iter().map(|v| value(&Item::Value(v.clone()))).collect())
    } else {
        json!(v.to_string())
    }
}
fn parse(path: &Path, bytes: &[u8]) -> Result<Value, String> {
    if path.extension().is_some_and(|e| e == "toml") {
        let doc: DocumentMut = std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e| format!("{e}"))?;
        Ok(value(&Item::Table(doc.as_table().clone())))
    } else {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}
fn inverse(
    current: &mut Value,
    before: Option<&Value>,
    after: Option<&Value>,
    path: &mut Vec<String>,
) {
    if path == &["autoMode", "environment"]
        || (path == &["permissions", "defaultMode"] && current == "plan")
        || path.first().is_some_and(|p| p == "mcp_servers")
    {
        return;
    }
    if before == after {
        return;
    }
    if before.is_some_and(Value::is_object) || after.is_some_and(Value::is_object) {
        let Some(obj) = current.as_object_mut() else {
            return;
        };
        let mut keys: Vec<_> = before
            .into_iter()
            .chain(after)
            .filter_map(Value::as_object)
            .flat_map(|o| o.keys().cloned())
            .collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let b = before.and_then(|v| v.get(&key));
            let a = after.and_then(|v| v.get(&key));
            path.push(key.clone());
            if path == &["autoMode", "environment"]
                || (path == &["permissions", "defaultMode"]
                    && obj.get(&key).is_some_and(|v| v == "plan"))
                || path.first().is_some_and(|p| p == "mcp_servers")
            {
                path.pop();
                continue;
            }
            if obj.get(&key) == a && a.is_none() {
                if let Some(b) = b {
                    obj.insert(key.clone(), b.clone());
                }
            } else if a.is_some_and(Value::is_array) || b.is_some_and(Value::is_array) {
                if let Some(current) = obj.get_mut(&key).and_then(Value::as_array_mut) {
                    let empty = vec![];
                    let old = b.and_then(Value::as_array).unwrap_or(&empty);
                    let new = a.and_then(Value::as_array).unwrap_or(&empty);
                    current.retain(|v| {
                        !new.contains(v)
                            || old.contains(v)
                            || v.as_str().is_some_and(|s| s.starts_with("mcp__"))
                    });
                    for v in old {
                        if !new.contains(v) && !current.contains(v) {
                            current.push(v.clone());
                        }
                    }
                }
            } else if b.is_some_and(Value::is_object) || a.is_some_and(Value::is_object) {
                if let Some(c) = obj.get_mut(&key) {
                    inverse(c, b, a, path);
                }
                if b.is_none()
                    && obj
                        .get(&key)
                        .and_then(Value::as_object)
                        .is_some_and(|o| o.is_empty())
                {
                    obj.remove(&key);
                }
            } else if obj.get(&key) == a {
                if let Some(b) = b {
                    obj.insert(key, b.clone());
                } else {
                    obj.remove(&key);
                }
            }
            path.pop();
        }
    } else if Some(&*current) == after {
        *current = before.cloned().unwrap_or(Value::Null);
    }
}
fn patch_toml(table: &mut dyn toml_edit::TableLike, desired: &Value) -> Result<(), String> {
    let object = desired.as_object().ok_or("TOML object required")?;
    let keys: Vec<_> = table.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        if !object.contains_key(&key) {
            table.remove(&key);
        }
    }
    for (key, v) in object {
        if value(table.get(key).unwrap_or(&Item::None)) == *v {
            continue;
        }
        if v.is_object() {
            if !table.contains_key(key) {
                table.insert(key, Item::Table(toml_edit::Table::new()));
            }
            patch_toml(
                table
                    .get_mut(key)
                    .and_then(Item::as_table_like_mut)
                    .ok_or("TOML table required")?,
                v,
            )?;
        } else {
            table.insert(key, Item::Value(crate::codex_config::to_toml(v)?));
        }
    }
    Ok(())
}

pub fn run(home: &Path, dry_run: bool) -> Result<Value, String> {
    transaction::check_pending(home)?;
    let initial_state = match std::fs::read(state_path(home)) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if initial_state.as_deref() == Some(RETIRED) {
        return Ok(json!({"retired":true,"changes":[]}));
    }
    let mut journals: Vec<PathBuf> = match std::fs::read_dir(transaction::root(home)) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.to_string()),
    };
    journals.sort();
    journals.reverse();
    let mut restored_rules = BTreeMap::new();
    let mut retained = vec![];
    let mut snapshots = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut desired = BTreeMap::<PathBuf, Value>::new();
    // Ownership bytes, rather than journal recency, prove which migrations
    // remain active. A restored/cancelled transaction must never be inverted.
    let mut active_state = initial_state.clone();
    for journal in journals {
        if active_state.is_none() {
            break;
        }
        let journal: transaction::Journal =
            serde_json::from_slice(&read(&journal)?).map_err(|e| e.to_string())?;
        let Some(ownership) = journal
            .entries
            .iter()
            .find(|entry| entry.path == state_path(home) && entry.after == active_state)
        else {
            continue;
        };
        if !matches!(journal.version, 1 | 2) {
            return Err("unsupported permissions retirement journal".into());
        }
        active_state = ownership.before.clone();
        for entry in journal.entries {
            if entry.path == state_path(home) {
                continue;
            }
            if entry.path.extension().is_some_and(|s| s == "rules") && entry.after.is_none() {
                if let Some(before) = entry.before {
                    if !entry.path.exists() && entry.path.parent().is_some_and(Path::is_dir) {
                        restored_rules.insert(entry.path.clone(), Some(before));
                    } else {
                        retained.push(json!({"path":entry.path,"reason":"existing replacement or retired project directory; rules not restored"}));
                    }
                }
                continue;
            }
            if !matches!(
                entry.path.extension().and_then(|s| s.to_str()),
                Some("json" | "toml")
            ) {
                continue;
            }
            let Some(after) = entry.after.as_deref() else {
                continue;
            };
            if !snapshots.contains_key(&entry.path) {
                let bytes = match std::fs::read(&entry.path) {
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.to_string()),
                };
                desired.insert(entry.path.clone(), parse(&entry.path, &bytes)?);
                snapshots.insert(entry.path.clone(), bytes);
            }
            let b = entry
                .before
                .as_deref()
                .map(|b| parse(&entry.path, b))
                .transpose()?;
            let a = parse(&entry.path, after)?;
            let current = desired.get_mut(&entry.path).unwrap();
            let mcp: Vec<_> = current["permissions"]["allow"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v.as_str().is_some_and(|s| s.starts_with("mcp__")))
                .cloned()
                .collect();
            inverse(current, b.as_ref(), Some(&a), &mut vec![]);
            if b.as_ref().is_some_and(|before| before != current) {
                retained.push(json!({"path":entry.path,"reason":"owner changes, native MCP allowances, or protected Claude plan/environment retained"}));
            }
            if let Some(allows) = current
                .get_mut("permissions")
                .and_then(|p| p.get_mut("allow"))
                .and_then(Value::as_array_mut)
            {
                for tool in mcp {
                    if !allows.contains(&tool) {
                        allows.push(tool);
                    }
                }
            }
        }
    }
    if active_state.is_some() {
        retained.push(json!({"path":state_path(home),"reason":"active ownership chain has no matching earlier journal; unavailable before-images retained"}));
    } else if initial_state.is_none() {
        retained.push(json!({"path":state_path(home),"reason":"no active ownership record; historical journals do not authorize restoration"}));
    }
    // Without a before-image, remove only the explicitly named shared selector.
    let codex = home.join(".codex/config.toml");
    if !snapshots.contains_key(&codex) && codex.exists() {
        let bytes = read(&codex)?;
        let mut v = parse(&codex, &bytes)?;
        if v["default_permissions"] == "dotfiles" {
            v.as_object_mut().unwrap().remove("default_permissions");
            if let Some(p) = v.get_mut("permissions").and_then(Value::as_object_mut) {
                p.remove("dotfiles");
            }
            retained.push(json!({"path":codex,"reason":"no recorded before-image; only named dotfiles selector/profile removed; other settings retained"}));
            snapshots.insert(codex.clone(), bytes);
            desired.insert(codex, v);
        }
    }
    for relative in [".claude/settings.json", ".codex/config.toml"] {
        let path = home.join(relative);
        if path.exists() && !snapshots.contains_key(&path) {
            retained.push(json!({"path":path,"reason":"no recorded before-image; ambiguous native settings retained"}));
        }
    }
    let mut writes = restored_rules;
    let mut expected = BTreeMap::new();
    for (path, bytes) in &snapshots {
        let v = &desired[path];
        let after = if path.extension().is_some_and(|s| s == "toml") {
            let mut doc: DocumentMut = std::str::from_utf8(bytes)
                .map_err(|e| e.to_string())?
                .parse()
                .map_err(|e| format!("{e}"))?;
            patch_toml(doc.as_table_mut(), v)?;
            doc.to_string().into_bytes()
        } else {
            let mut b = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
            b.push(b'\n');
            b
        };
        expected.insert(path.clone(), digest(bytes));
        if parse(path, bytes)? != *v {
            writes.insert(path.clone(), Some(after));
        }
    }
    if let Some(bytes) = initial_state {
        expected.insert(state_path(home), digest(&bytes));
    }
    let changes: Vec<_> = writes.keys().cloned().collect();
    let differences:Vec<_>=writes.iter().map(|(path,after)|json!({"path":path,"before_digest":expected.get(path),"after_digest":after.as_ref().map(|b|digest(b))})).collect();
    if !dry_run {
        transaction::commit(home, writes, &expected, || Ok(RETIRED.to_vec()))?;
    }
    // Rules that reclaim queued before the retirement are not restored by it: they wait in the
    // queue for `pending --restore-rules` or `--drop` (#1158).
    let queued = super::reclaim::candidates(home).map_or(0, |c| c.len());
    Ok(
        json!({"dry_run":dry_run,"changes":changes,"differences":differences,"retained":retained,"queued_rules":queued}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn inverse_preserves_user_edits_plan_and_environment() {
        let before = json!({"sandbox":{"enabled":false},"permissions":{"defaultMode":"plan"},"autoMode":{"environment":["owner"]},"model":"old"});
        let after = json!({"sandbox":{"enabled":true},"permissions":{"defaultMode":"plan"},"autoMode":{"environment":["shared"],"classifyAllShell":true},"model":"old"});
        let mut current = after.clone();
        current["model"] = json!("user");
        current["autoMode"]["environment"] = json!(["user"]);
        inverse(&mut current, Some(&before), Some(&after), &mut vec![]);
        assert_eq!(current["sandbox"]["enabled"], false);
        assert_eq!(current["permissions"]["defaultMode"], "plan");
        assert_eq!(current["autoMode"]["environment"], json!(["user"]));
        assert_eq!(current["model"], "user");
        assert!(current["autoMode"].get("classifyAllShell").is_none());
    }
    #[test]
    fn no_record_dry_run_and_retry_are_conservative() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(home.join(".codex")).unwrap();
        let file = home.join(".codex/config.toml");
        let text="model = 'owner'\ndefault_permissions = 'dotfiles'\n[permissions.dotfiles]\nextends = ':workspace'\n[permissions.owner]\nextends = ':read-only'\n";
        fs::write(&file, text).unwrap();
        run(home, true).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
        assert!(!state_path(home).exists());
        run(home, false).unwrap();
        let result = fs::read_to_string(&file).unwrap();
        assert!(result.contains("owner"));
        assert!(!result.contains("dotfiles"));
        // New native policy written after retirement is never retired on retry.
        fs::write(&file, text).unwrap();
        run(home, false).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
    }
    #[test]
    fn recorded_change_restores_only_equal_leaves() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, "{\"owned\":false,\"user\":0}").unwrap();
        let before = fs::read(&file).unwrap();
        transaction::commit(
            home,
            BTreeMap::from([(file.clone(), Some(b"{\"owned\":true,\"user\":1}".to_vec()))]),
            &BTreeMap::from([(file.clone(), digest(&before))]),
            || Ok(b"{}".to_vec()),
        )
        .unwrap();
        fs::write(&file, "{\"owned\":true,\"user\":2}").unwrap();
        run(home, false).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert_eq!(v, json!({"owned":false,"user":2}));
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    #[test]
    fn pending_conflict_keeps_backup_and_rejects_retirement() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        std::fs::write(&file, b"{}").unwrap();
        let result = transaction::commit(
            home,
            BTreeMap::from([(file.clone(), Some(b"{\"a\":1}".to_vec()))]),
            &BTreeMap::from([(file.clone(), digest(b"{}"))]),
            || Err("interrupted".into()),
        );
        assert!(result.is_err());
        assert!(run(home, false).unwrap_err().contains("incomplete"));
        assert!(transaction::root(home).join("pending").exists());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "{\"a\":1}");
    }
}

#[cfg(test)]
mod retirement_edges {
    use super::*;
    use std::fs;
    #[test]
    fn array_inverse_preserves_owner_additions_and_native_mcp() {
        let before = json!({"permissions":{"allow":["old"]}});
        let after = json!({"permissions":{"allow":["shared","mcp__native__tool"]}});
        let mut current = json!({"permissions":{"allow":["shared","owner","mcp__native__tool"]}});
        inverse(&mut current, Some(&before), Some(&after), &mut vec![]);
        assert_eq!(
            current["permissions"]["allow"],
            json!(["owner", "mcp__native__tool", "old"])
        );
    }
    #[test]
    fn deleted_rules_restored_but_missing_project_and_replacements_retained() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let rules = home.join("default.rules");
        fs::write(&rules, "prefix_rule(pattern=['git'])\n").unwrap();
        let old = fs::read(&rules).unwrap();
        transaction::commit(
            home,
            BTreeMap::from([(rules.clone(), None)]),
            &BTreeMap::from([(rules.clone(), digest(&old))]),
            || Ok(b"{}".to_vec()),
        )
        .unwrap();
        let preview = run(home, true).unwrap();
        assert!(preview["changes"]
            .as_array()
            .unwrap()
            .contains(&json!(rules)));
        assert!(!rules.exists());
        run(home, false).unwrap();
        assert_eq!(fs::read(&rules).unwrap(), old);
        // Replacements are never overwritten, even with the same old journal.
        fs::remove_file(state_path(home)).unwrap();
        fs::write(&rules, "owner replacement").unwrap();
        run(home, false).unwrap();
        assert_eq!(fs::read_to_string(&rules).unwrap(), "owner replacement");
        fs::remove_file(state_path(home)).unwrap();
        let sub = home.join("retired-project");
        fs::create_dir(&sub).unwrap();
        let path = sub.join("a.rules");
        fs::write(&path, "old").unwrap();
        transaction::commit(
            home,
            BTreeMap::from([(path.clone(), None)]),
            &BTreeMap::from([(path.clone(), digest(b"old"))]),
            || Ok(b"{}".to_vec()),
        )
        .unwrap();
        fs::remove_dir(&sub).unwrap();
        run(home, false).unwrap();
        assert!(!sub.exists());
    }
    #[test]
    fn unknown_journal_version_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(transaction::root(home)).unwrap();
        fs::write(state_path(home), b"{}").unwrap();
        fs::write(
            transaction::root(home).join("1-1.json"),
            json!({"version":999,"entries":[{"path":state_path(home),"before":null,"after":"7b7d"}]}).to_string(),
        )
        .unwrap();
        assert!(run(home, false).unwrap_err().contains("unsupported"));
        assert_eq!(fs::read(state_path(home)).unwrap(), b"{}");
    }
}

#[cfg(test)]
mod active_ownership_tests {
    use super::*;
    use std::fs;
    #[test]
    fn cancelled_migration_is_not_inverted_even_if_user_recreates_its_values() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, b"{\"enabled\":false}").unwrap();
        let journal = transaction::commit(
            home,
            BTreeMap::from([(file.clone(), Some(b"{\"enabled\":true}".to_vec()))]),
            &BTreeMap::from([(file.clone(), digest(b"{\"enabled\":false}"))]),
            || Ok(b"{\"ownership\":1}".to_vec()),
        )
        .unwrap();
        transaction::restore(home, &journal).unwrap();
        fs::write(&file, b"{\"enabled\":true}").unwrap();
        let report = run(home, false).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"{\"enabled\":true}");
        assert!(report["retained"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["reason"]
                .as_str()
                .unwrap()
                .contains("no active ownership")));
    }
    #[test]
    fn active_chain_skips_later_cancelled_journal_and_restores_original() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let file = home.join("settings.json");
        fs::write(&file, b"{\"level\":0}").unwrap();
        transaction::commit(
            home,
            BTreeMap::from([(file.clone(), Some(b"{\"level\":1}".to_vec()))]),
            &BTreeMap::from([(file.clone(), digest(b"{\"level\":0}"))]),
            || Ok(b"{\"ownership\":1}".to_vec()),
        )
        .unwrap();
        let cancelled = transaction::commit(
            home,
            BTreeMap::from([(file.clone(), Some(b"{\"level\":2}".to_vec()))]),
            &BTreeMap::from([(file.clone(), digest(b"{\"level\":1}"))]),
            || Ok(b"{\"ownership\":2}".to_vec()),
        )
        .unwrap();
        transaction::restore(home, &cancelled).unwrap();
        run(home, false).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(),
            json!({"level":0})
        );
    }
}
