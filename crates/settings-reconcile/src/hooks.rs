//! `.hooks.<event>` への hook 登録(Claude `settings.json` / Codex `hooks.json` /
//! Copilot `settings.json`)。
//!
//! 旧 bash(`registerHooks` と `scripts/register-{codex,copilot}-hooks`)は
//! 「command の完全一致が既にあれば何もしない」で存在判定していたため、
//! matcher や timeout を宣言側で変えても既存エントリが更新されない既知の罠が
//! あった(docs/claude/issue-index.md、#414)。ここでは宣言を正とする reconcile
//! にして、command が一致するハンドラの matcher / `if` / timeout を宣言に
//! 合わせて直す。それ以外(他ツールのエントリ、ユーザーが足した未知のキー)は
//! 触らない。`retire`(旧 command の完全一致削除)の意味は旧実装のまま。

use crate::json::J;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Number;
use std::collections::HashSet;

/// Claude / Codex は `{matcher?, hooks:[{type, command, ...}]}` の入れ子、
/// Copilot は event 配列に `{type, bash, timeoutSec}` を直接並べる。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    Nested,
    Flat,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Retire {
    pub event: String,
    pub command: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Register {
    pub event: String,
    /// 空文字または省略ならフィールド自体を出力しない(Flat では無視)。
    #[serde(default)]
    pub matcher: Option<String>,
    pub command: String,
    /// 省略ならフィールド自体を出力しない(Flat では `timeoutSec`)。
    #[serde(default)]
    pub timeout: Option<Number>,
    /// ハンドラレベルの絞り込み(permission rule 構文、例 `Bash(git -C *)`)。
    /// 空文字または省略なら出力しない(Nested のみ)。
    #[serde(default, rename = "if")]
    pub if_rule: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct HooksSpec {
    #[serde(default)]
    pub retire: Vec<Retire>,
    #[serde(default)]
    pub register: Vec<Register>,
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.is_empty())
}

/// retire を先に、register を後に適用する(旧実装と同じ順序)。
pub fn apply(root: &mut J, flavor: Flavor, spec: &HooksSpec) -> Result<(), String> {
    for r in &spec.retire {
        match flavor {
            Flavor::Nested => retire_nested(root, &r.event, &r.command)?,
            Flavor::Flat => retire_flat(root, &r.event, &r.command)?,
        }
    }
    // (event, command) は宣言の一意キー。重複した宣言は後勝ちにせず、最初の
    // ものだけを採って警告する(同じキーを 2 度書くと互いに上書きし合う)。
    let mut seen = HashSet::new();
    for r in &spec.register {
        if !seen.insert((r.event.as_str(), r.command.as_str())) {
            eprintln!(
                "settings-reconcile: 重複した hook 宣言を無視します: {} {}",
                r.event, r.command
            );
            continue;
        }
        match flavor {
            Flavor::Nested => register_nested(root, r)?,
            Flavor::Flat => register_flat(root, r)?,
        }
    }
    Ok(())
}

fn hooks_obj(root: &mut J, create: bool) -> Result<Option<&mut IndexMap<String, J>>, String> {
    let top = root
        .as_obj_mut()
        .ok_or("トップレベルが JSON オブジェクトではない")?;
    if !top.contains_key("hooks") || top.get("hooks") == Some(&J::Null) {
        if !create {
            return Ok(None);
        }
        top.insert("hooks".into(), J::Obj(IndexMap::new()));
    }
    match top.get_mut("hooks") {
        Some(J::Obj(m)) => Ok(Some(m)),
        _ => Err(".hooks がオブジェクトではない".into()),
    }
}

fn str_field<'a>(v: &'a J, key: &str) -> Option<&'a str> {
    v.as_obj()?.get(key)?.as_str()
}

fn group_handlers(g: &J) -> &[J] {
    match g.as_obj().and_then(|m| m.get("hooks")) {
        Some(J::Arr(a)) => a,
        _ => &[],
    }
}

fn group_has(g: &J, cmd: &str) -> bool {
    group_handlers(g)
        .iter()
        .any(|h| str_field(h, "command") == Some(cmd))
}

fn retire_nested(root: &mut J, event: &str, cmd: &str) -> Result<(), String> {
    let Some(hooks) = hooks_obj(root, false)? else {
        return Ok(());
    };
    let Some(J::Arr(groups)) = hooks.get_mut(event) else {
        return Ok(());
    };
    if !groups.iter().any(|g| group_has(g, cmd)) {
        // 該当ゼロなら読むだけで書かない(定常状態では触らない)。
        return Ok(());
    }
    for g in groups.iter_mut() {
        if let Some(J::Arr(hs)) = g.as_obj_mut().and_then(|m| m.get_mut("hooks")) {
            hs.retain(|h| str_field(h, "command") != Some(cmd));
        }
    }
    // 空になった matcher グループを畳む(hooks キーを持たないグループは残す)。
    groups.retain(|g| match g.as_obj().and_then(|m| m.get("hooks")) {
        Some(J::Arr(a)) => !a.is_empty(),
        _ => true,
    });
    if groups.is_empty() {
        hooks.shift_remove(event);
    }
    Ok(())
}

fn retire_flat(root: &mut J, event: &str, cmd: &str) -> Result<(), String> {
    let Some(hooks) = hooks_obj(root, false)? else {
        return Ok(());
    };
    let Some(J::Arr(entries)) = hooks.get_mut(event) else {
        return Ok(());
    };
    if !entries.iter().any(|e| str_field(e, "bash") == Some(cmd)) {
        return Ok(());
    }
    entries.retain(|e| str_field(e, "bash") != Some(cmd));
    if entries.is_empty() {
        hooks.shift_remove(event);
    }
    Ok(())
}

fn event_list<'a>(
    hooks: &'a mut IndexMap<String, J>,
    event: &str,
) -> Result<&'a mut Vec<J>, String> {
    if matches!(hooks.get(event), None | Some(J::Null)) {
        hooks.insert(event.to_owned(), J::Arr(Vec::new()));
    }
    match hooks.get_mut(event) {
        Some(J::Arr(a)) => Ok(a),
        _ => Err(format!(".hooks.{event} が配列ではない")),
    }
}

/// 宣言の timeout / if を既存ハンドラへ反映する。未知のキーは残す。
fn apply_handler_fields(h: &mut IndexMap<String, J>, r: &Register) {
    h.insert("type".into(), J::str("command"));
    h.insert("command".into(), J::str(&r.command));
    match nonempty(&r.if_rule) {
        Some(v) => {
            h.insert("if".into(), J::str(v));
        }
        None => {
            h.shift_remove("if");
        }
    }
    match &r.timeout {
        Some(t) => {
            h.insert("timeout".into(), J::Num(t.clone()));
        }
        None => {
            h.shift_remove("timeout");
        }
    }
}

fn new_group(handler: J, matcher: Option<&str>) -> J {
    let mut g = IndexMap::new();
    if let Some(m) = matcher {
        g.insert("matcher".into(), J::str(m));
    }
    g.insert("hooks".into(), J::Arr(vec![handler]));
    J::Obj(g)
}

fn register_nested(root: &mut J, r: &Register) -> Result<(), String> {
    let hooks = hooks_obj(root, true)?.ok_or("hooks")?;
    let desired = nonempty(&r.matcher);

    // 先に存在確認だけして、未登録のときだけ event キーを作る。
    let exists = matches!(hooks.get(&r.event), Some(J::Arr(gs)) if gs.iter().any(|g| group_has(g, &r.command)));
    if !exists {
        let mut h = IndexMap::new();
        apply_handler_fields(&mut h, r);
        event_list(hooks, &r.event)?.push(new_group(J::Obj(h), desired));
        return Ok(());
    }
    let groups = event_list(hooks, &r.event)?;

    let mut occ = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        for (hi, h) in group_handlers(g).iter().enumerate() {
            if str_field(h, "command") == Some(r.command.as_str()) {
                occ.push((gi, hi));
            }
        }
    }
    let (gi, hi) = occ[0];

    // 同じ command の重複は 1 つに畳む(宣言の command は event 内で一意)。
    let mut touched = Vec::new();
    for &(dgi, dhi) in occ.iter().skip(1).rev() {
        if let Some(J::Arr(hs)) = groups[dgi].as_obj_mut().and_then(|m| m.get_mut("hooks")) {
            hs.remove(dhi);
            touched.push(dgi);
        }
    }
    let mut idx = 0;
    groups.retain(|g| {
        let keep = !(touched.contains(&idx) && group_handlers(g).is_empty());
        idx += 1;
        keep
    });

    let single = group_handlers(&groups[gi]).len() == 1;
    let current = str_field(&groups[gi], "matcher").filter(|m| !m.is_empty());
    if single || current == desired {
        let g = groups[gi]
            .as_obj_mut()
            .ok_or("hook グループがオブジェクトではない")?;
        if single {
            match desired {
                Some(m) if g.contains_key("matcher") => {
                    g.insert("matcher".into(), J::str(m));
                }
                Some(m) => {
                    g.shift_insert(0, "matcher".into(), J::str(m));
                }
                None => {
                    g.shift_remove("matcher");
                }
            }
        }
        if let Some(J::Arr(hs)) = g.get_mut("hooks") {
            if let Some(h) = hs[hi].as_obj_mut() {
                apply_handler_fields(h, r);
            }
        }
        return Ok(());
    }

    // matcher が違うのに他ツールのハンドラと同じグループに居る: グループの
    // matcher を変えると相乗りしている他のハンドラの発火条件まで変わるので、
    // このハンドラだけ新しいグループへ移す。
    let moved = match groups[gi].as_obj_mut().and_then(|m| m.get_mut("hooks")) {
        Some(J::Arr(hs)) => hs.remove(hi),
        _ => return Ok(()),
    };
    let mut h = match moved {
        J::Obj(h) => h,
        _ => IndexMap::new(),
    };
    apply_handler_fields(&mut h, r);
    groups.push(new_group(J::Obj(h), desired));
    Ok(())
}

fn flat_entry_fields(e: &mut IndexMap<String, J>, r: &Register) {
    e.insert("type".into(), J::str("command"));
    e.insert("bash".into(), J::str(&r.command));
    match &r.timeout {
        Some(t) => {
            e.insert("timeoutSec".into(), J::Num(t.clone()));
        }
        None => {
            e.shift_remove("timeoutSec");
        }
    }
}

fn register_flat(root: &mut J, r: &Register) -> Result<(), String> {
    let hooks = hooks_obj(root, true)?.ok_or("hooks")?;
    let exists = matches!(hooks.get(&r.event), Some(J::Arr(es)) if es.iter().any(|e| str_field(e, "bash") == Some(r.command.as_str())));
    let entries = event_list(hooks, &r.event)?;
    if !exists {
        let mut e = IndexMap::new();
        flat_entry_fields(&mut e, r);
        entries.push(J::Obj(e));
        return Ok(());
    }
    let mut first = true;
    entries.retain(|e| {
        if str_field(e, "bash") != Some(r.command.as_str()) {
            return true;
        }
        let keep = first;
        first = false;
        keep
    });
    for e in entries.iter_mut() {
        if str_field(e, "bash") == Some(r.command.as_str()) {
            if let Some(m) = e.as_obj_mut() {
                flat_entry_fields(m, r);
            }
        }
    }
    Ok(())
}
