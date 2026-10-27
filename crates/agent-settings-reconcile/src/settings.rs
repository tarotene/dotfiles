//! `permissions` / `statusLine` / `mcpServers` の reconcile。

use crate::json::J;
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct PermissionsSpec {
    /// `permissions.allow` から外す旧ルール(完全一致)。
    #[serde(default)]
    pub retire_allow: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub retire_ask: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    /// `permissions.deny`。宣言どうし・撤回(完全一致)とも allow / ask と同じ扱い。
    #[serde(default)]
    pub retire_deny: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    /// 宣言があるときだけ `sandbox` を合わせる(Codex の `dotfiles` 権限 profile と
    /// 同じ層構造。docs/agents/agent-permissions.md の層別の対応表)。
    #[serde(default)]
    pub sandbox: Option<SandboxSpec>,
    /// `autoMode.classifyAllShell`。宣言があるときだけ書く。`autoMode.environment`
    /// など他のキーには触れない。
    #[serde(default)]
    pub auto_mode_classify_all_shell: Option<bool>,
}

/// `sandbox` のうち、宣言したキーだけを合わせる。真偽値は set-if-different、
/// 配列は「宣言との和 − 撤回」(本人が `/sandbox` や承認で足した値は残す)。
/// 宣言に無いキー(`allowUnsandboxedCommands` など)には触れない。
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct SandboxSpec {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub fail_if_unavailable: Option<bool>,
    #[serde(default)]
    pub auto_allow_bash_if_sandboxed: Option<bool>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub retire_allowed_domains: Vec<String>,
    #[serde(default)]
    pub deny_read: Vec<String>,
    #[serde(default)]
    pub retire_deny_read: Vec<String>,
    /// `sandbox.filesystem.allowWrite`。sandbox 内の道具が書く場所(エージェントが Bash で呼ぶ
    /// 自前の道具の状態ディレクトリなど)。宣言との和 − 撤回で、本人が足した値は残す。
    #[serde(default)]
    pub allow_write: Vec<String>,
    #[serde(default)]
    pub retire_allow_write: Vec<String>,
    #[serde(default)]
    pub excluded_commands: Vec<String>,
    #[serde(default)]
    pub retire_excluded_commands: Vec<String>,
}

/// #461: 中間 `*` を含む allow rule(`Bash(git -C * add *)` のような、`(` か
/// 空白の直後の `*` に空白を挟んで `)` 以外が続くもの)。旧 jq の
/// `(\(| )\*[[:space:]]+[^)]` と同じ判定。
fn has_mid_wildcard(rule: &str) -> bool {
    let cs: Vec<char> = rule.chars().collect();
    for i in 0..cs.len() {
        if !(cs[i] == '(' || cs[i] == ' ') || cs.get(i + 1) != Some(&'*') {
            continue;
        }
        let rest = &cs[i + 2..];
        let ws = rest.iter().take_while(|c| c.is_whitespace()).count();
        // `[[:space:]]+[^)]`: 空白が 2 文字以上あれば最後の 1 文字が `[^)]` を
        // 兼ねる。ちょうど 1 文字なら、その次に `)` 以外の文字が要る。
        if ws >= 2 || (ws == 1 && rest.get(1).is_some_and(|c| *c != ')')) {
            return true;
        }
    }
    false
}

fn string_array<'a>(
    perms: &'a mut IndexMap<String, J>,
    key: &str,
) -> Result<&'a mut Vec<J>, String> {
    if matches!(perms.get(key), None | Some(J::Null)) {
        perms.insert(key.to_owned(), J::Arr(Vec::new()));
    }
    match perms.get_mut(key) {
        Some(J::Arr(a)) => Ok(a),
        _ => Err(format!(".permissions.{key} が配列ではない")),
    }
}

/// `permissions.allow` / `permissions.ask` を要素単位で冪等に同期する。
/// retire を外し(allow は中間ワイルドカード入りも一律 strip)、宣言のうち
/// まだ無いものを末尾に足す。`defaultMode` など他のキーには触らない。
/// 旧 bash と同じく、無い配列は空配列として作る。
pub fn permissions(root: &mut J, spec: &PermissionsSpec) -> Result<(), String> {
    let top = root
        .as_obj_mut()
        .ok_or("トップレベルが JSON オブジェクトではない")?;
    if matches!(top.get("permissions"), None | Some(J::Null)) {
        top.insert("permissions".into(), J::Obj(IndexMap::new()));
    }
    let Some(J::Obj(perms)) = top.get_mut("permissions") else {
        return Err(".permissions がオブジェクトではない".into());
    };

    let allow = string_array(perms, "allow")?;
    allow.retain(|v| match v.as_str() {
        Some(s) => !spec.retire_allow.iter().any(|r| r == s) && !has_mid_wildcard(s),
        None => true,
    });
    for r in &spec.allow {
        if !allow.iter().any(|v| v.as_str() == Some(r)) {
            allow.push(J::str(r));
        }
    }
    // 宣言どうしの重複は旧 jq と同じく(kept に対してだけ照合するので)そのまま
    // 追加される…が、ここでは追加の都度照合する。nix 側の宣言は一意なので差は出ない。

    let ask = string_array(perms, "ask")?;
    ask.retain(|v| match v.as_str() {
        Some(s) => !spec.retire_ask.iter().any(|r| r == s),
        None => true,
    });
    for r in &spec.ask {
        if !ask.iter().any(|v| v.as_str() == Some(r)) {
            ask.push(J::str(r));
        }
    }

    // deny は宣言も撤回も空なら配列を作らない(allow / ask と違い、宣言の無い
    // 環境に空配列を足さない)。
    if !spec.deny.is_empty() || !spec.retire_deny.is_empty() {
        sync_strings(string_array(perms, "deny")?, &spec.deny, &spec.retire_deny);
    }

    if let Some(sandbox) = &spec.sandbox {
        apply_sandbox(top, sandbox)?;
    }
    if let Some(classify) = spec.auto_mode_classify_all_shell {
        let auto = object_entry(top, "autoMode")?;
        set_bool(auto, "classifyAllShell", classify);
    }
    Ok(())
}

/// 配列から撤回の完全一致を外し、宣言のうち無いものを末尾に足す。
fn sync_strings(arr: &mut Vec<J>, add: &[String], retire: &[String]) {
    arr.retain(|v| match v.as_str() {
        Some(s) => !retire.iter().any(|r| r == s),
        None => true,
    });
    for a in add {
        if !arr.iter().any(|v| v.as_str() == Some(a)) {
            arr.push(J::str(a));
        }
    }
}

/// `key` のオブジェクトを返す。無い・null なら空のオブジェクトを作る。
fn object_entry<'a>(
    parent: &'a mut IndexMap<String, J>,
    key: &str,
) -> Result<&'a mut IndexMap<String, J>, String> {
    if matches!(parent.get(key), None | Some(J::Null)) {
        parent.insert(key.to_owned(), J::Obj(IndexMap::new()));
    }
    match parent.get_mut(key) {
        Some(J::Obj(o)) => Ok(o),
        _ => Err(format!(".{key} がオブジェクトではない")),
    }
}

fn set_bool(obj: &mut IndexMap<String, J>, key: &str, value: bool) {
    if obj.get(key) != Some(&J::Bool(value)) {
        obj.insert(key.to_owned(), J::Bool(value));
    }
}

fn apply_sandbox(top: &mut IndexMap<String, J>, spec: &SandboxSpec) -> Result<(), String> {
    let sandbox = object_entry(top, "sandbox")?;
    for (key, value) in [
        ("enabled", spec.enabled),
        ("failIfUnavailable", spec.fail_if_unavailable),
        (
            "autoAllowBashIfSandboxed",
            spec.auto_allow_bash_if_sandboxed,
        ),
    ] {
        if let Some(v) = value {
            set_bool(sandbox, key, v);
        }
    }
    if !spec.excluded_commands.is_empty() || !spec.retire_excluded_commands.is_empty() {
        sync_strings(
            string_array(sandbox, "excludedCommands")?,
            &spec.excluded_commands,
            &spec.retire_excluded_commands,
        );
    }
    if !spec.allowed_domains.is_empty() || !spec.retire_allowed_domains.is_empty() {
        let network = object_entry(sandbox, "network")?;
        sync_strings(
            string_array(network, "allowedDomains")?,
            &spec.allowed_domains,
            &spec.retire_allowed_domains,
        );
    }
    if !spec.deny_read.is_empty()
        || !spec.retire_deny_read.is_empty()
        || !spec.allow_write.is_empty()
        || !spec.retire_allow_write.is_empty()
    {
        let filesystem = object_entry(sandbox, "filesystem")?;
        if !spec.deny_read.is_empty() || !spec.retire_deny_read.is_empty() {
            sync_strings(
                string_array(filesystem, "denyRead")?,
                &spec.deny_read,
                &spec.retire_deny_read,
            );
        }
        if !spec.allow_write.is_empty() || !spec.retire_allow_write.is_empty() {
            sync_strings(
                string_array(filesystem, "allowWrite")?,
                &spec.allow_write,
                &spec.retire_allow_write,
            );
        }
    }
    Ok(())
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct StatusLineSpec {
    /// 空なら宣言なし(撤回だけ行う)。
    #[serde(default)]
    pub desired: String,
    #[serde(default)]
    pub retired: Vec<String>,
}

/// `statusLine` を宣言に合わせる。retired を先に処理し、`.statusLine.command` が
/// retired のいずれかに完全一致したときだけキーを削除する(無条件 del にしない
/// のは、`/statusline` で本人が設定した値を「宣言なし」の switch で奪わない
/// ため)。desired が非空なら最後に command が違うときだけ set する。
pub fn statusline(root: &mut J, spec: &StatusLineSpec) -> Result<(), String> {
    let top = root
        .as_obj_mut()
        .ok_or("トップレベルが JSON オブジェクトではない")?;
    let mut current = top
        .get("statusLine")
        .and_then(|s| s.as_obj())
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_owned();
    if spec.retired.contains(&current) {
        // null 代入ではなくキーの削除: 「この機能を入れる前の形に戻す」が撤回の
        // 意味で、/statusline による後からの設定も素直に効く。
        top.shift_remove("statusLine");
        current.clear();
    }
    if !spec.desired.is_empty() && current != spec.desired {
        let mut sl = IndexMap::new();
        sl.insert("type".to_owned(), J::str("command"));
        sl.insert("command".to_owned(), J::str(&spec.desired));
        top.insert("statusLine".into(), J::Obj(sl));
    }
    Ok(())
}

fn deep_merge(base: &mut J, new: &J) {
    match (base, new) {
        (J::Obj(b), J::Obj(n)) => {
            for (k, v) in n {
                match b.get_mut(k) {
                    Some(bv) => deep_merge(bv, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, n) => *b = n.clone(),
    }
}

/// `.mcpServers` を宣言集合に reconcile する。宣言外のキーは削除(削除名を
/// 返す — 不可逆なので呼び出し側が stderr に残す)、宣言したキーは宣言値を
/// 既存値の上に deep merge する(Claude Code が認証時に書き足す `oauth` 等を
/// 剥がさないため)。`.mcpServers` が無く宣言も空なら何も作らない。
/// `.projects` 配下(project scope)には触れない。
pub fn mcp_servers(root: &mut J, new: &IndexMap<String, J>) -> Result<Vec<String>, String> {
    let top = root
        .as_obj_mut()
        .ok_or("トップレベルが JSON オブジェクトではない")?;
    let current: IndexMap<String, J> = match top.get("mcpServers") {
        None | Some(J::Null) => IndexMap::new(),
        Some(J::Obj(m)) => m.clone(),
        Some(_) => return Err(".mcpServers がオブジェクトではない".into()),
    };
    let removed: Vec<String> = current
        .keys()
        .filter(|k| !new.contains_key(*k))
        .cloned()
        .collect();
    let mut merged = J::Obj(
        current
            .into_iter()
            .filter(|(k, _)| new.contains_key(k))
            .collect(),
    );
    deep_merge(&mut merged, &J::Obj(new.clone()));
    let empty_before = matches!(top.get("mcpServers"), None | Some(J::Null));
    if empty_before && merged == J::Obj(IndexMap::new()) {
        return Ok(removed);
    }
    top.insert("mcpServers".into(), merged);
    Ok(removed)
}
