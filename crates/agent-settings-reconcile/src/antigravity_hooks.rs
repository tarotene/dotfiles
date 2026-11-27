//! Antigravity CLI(`agy`)の `~/.gemini/config/hooks.json` の reconcile。
//!
//! このファイルは `{<hook 名>: {<イベント>: [...]}}` の形で、agy の TUI(`/hooks`)と herdr の連携
//! (`herdr integration install antigravity-cli`)も、自分の名前のキーを足す
//! (agy 1.5.17 の changelog と同梱の `agy-customizations/docs/hooks.md`、取得 2026-11-27)。
//! ファイル全体は配れないので、**自分の名前のキー 1 つだけ**を所有する: 宣言した中身で置き換え、
//! 宣言が空なら消す。ほかのキーは触らない。所有の単位がキーなので、前回の記録(所有の状態ファイル)は
//! 要らない。宣言から外した hook は、キーごと置き換わって消える。
//!
//! 文書の形に合わない宣言は、ファイルを書く前に止める(agy は読めない hook を黙って捨てるので、
//! 配ったつもりで効かない状態を作らないため):
//!
//! - イベントは `SessionStart`・`PreToolUse`・`PostToolUse`・`PreInvocation`・`PostInvocation`・`Stop`。
//! - `PreToolUse`・`PostToolUse` は `{matcher, hooks: [ハンドラ]}` の組(matcher は道具名の正規表現)。
//!   ほかのイベントは、ハンドラを直接並べる(matcher は無視される)。
//! - ハンドラは `command`(空でない文字列)を持ち、`type` は `command` だけ、`timeout` は正の整数(秒)。

use crate::json::J;
use serde::Deserialize;

/// 文書の「Supported Event Types」の 6 つ。
const EVENTS: [&str; 6] = [
    "SessionStart",
    "PreToolUse",
    "PostToolUse",
    "PreInvocation",
    "PostInvocation",
    "Stop",
];

/// matcher とハンドラの組で書くイベント(文書の「Grouped」)。
const GROUPED: [&str; 2] = ["PreToolUse", "PostToolUse"];

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct HooksSpec {
    /// 所有するキー(hook 名)。
    pub name: String,
    /// そのキーの中身(イベント → 配列)。空のオブジェクトは「所有するキーを消す」。
    pub hooks: J,
}

fn check_handler(event: &str, handler: &J) -> Result<(), String> {
    let obj = handler
        .as_obj()
        .ok_or_else(|| format!("{event}: ハンドラはオブジェクト"))?;
    match obj.get("command").and_then(J::as_str) {
        Some(c) if !c.trim().is_empty() => {}
        _ => return Err(format!("{event}: ハンドラに空でない `command` が要る")),
    }
    if let Some(t) = obj.get("type") {
        if t.as_str() != Some("command") {
            return Err(format!("{event}: `type` は `command` だけ(文書の記述)"));
        }
    }
    if let Some(t) = obj.get("timeout") {
        match t {
            J::Num(n) if n.as_u64().is_some_and(|n| n > 0) => {}
            _ => return Err(format!("{event}: `timeout` は正の整数(秒)")),
        }
    }
    for key in obj.keys() {
        if !["type", "command", "timeout"].contains(&key.as_str()) {
            return Err(format!("{event}: ハンドラの未知のキー `{key}`"));
        }
    }
    Ok(())
}

/// 宣言の中身を、文書の形と照らして確かめる。
pub fn validate(hooks: &J) -> Result<(), String> {
    let events = hooks
        .as_obj()
        .ok_or("hooks: イベントのオブジェクトが必要")?;
    for (event, entries) in events {
        if !EVENTS.contains(&event.as_str()) {
            return Err(format!("未知のイベント `{event}`({})", EVENTS.join("・")));
        }
        let J::Arr(entries) = entries else {
            return Err(format!("{event}: 配列が必要"));
        };
        for entry in entries {
            if GROUPED.contains(&event.as_str()) {
                let obj = entry
                    .as_obj()
                    .ok_or_else(|| format!("{event}: `{{matcher, hooks}}` の組が必要"))?;
                if obj.get("matcher").and_then(J::as_str).is_none() {
                    return Err(format!("{event}: `matcher`(文字列)が要る"));
                }
                let Some(J::Arr(handlers)) = obj.get("hooks") else {
                    return Err(format!("{event}: `hooks`(ハンドラの配列)が要る"));
                };
                if obj.len() != 2 {
                    return Err(format!("{event}: 組のキーは `matcher` と `hooks` だけ"));
                }
                for h in handlers {
                    check_handler(event, h)?;
                }
            } else {
                check_handler(event, entry)?;
            }
        }
    }
    Ok(())
}

/// `root`(hooks.json の中身)の `spec.name` のキーを、宣言で置き換える。宣言が空なら、そのキーを消す。
/// `root` が空(ファイルが無い)なら、オブジェクトにする。ほかのキーは触らない。
pub fn apply(root: &mut J, spec: &HooksSpec) -> Result<(), String> {
    if spec.name.trim().is_empty() {
        return Err("name が空".to_owned());
    }
    validate(&spec.hooks)?;
    if matches!(root, J::Null) {
        *root = J::Obj(Default::default());
    }
    let map = root
        .as_obj_mut()
        .ok_or("hooks.json の最上位がオブジェクトでない")?;
    let empty = spec.hooks.as_obj().is_some_and(|m| m.is_empty());
    if empty {
        map.shift_remove(&spec.name);
    } else {
        map.insert(spec.name.clone(), spec.hooks.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn spec(text: &str) -> HooksSpec {
        serde_json::from_str(text).unwrap()
    }

    fn run(before: &str, spec_text: &str) -> Result<J, String> {
        let mut root = parse(before).unwrap();
        apply(&mut root, &spec(spec_text))?;
        Ok(root)
    }

    const OWN: &str = r#"{"name":"dotfiles","hooks":{
        "SessionStart":[{"type":"command","command":"echo a","timeout":10}],
        "PreToolUse":[{"matcher":"run_command","hooks":[{"command":"echo b"}]}]}}"#;

    #[test]
    fn an_empty_file_gets_the_owned_key() {
        let after = run("null", OWN).unwrap();
        assert!(after.as_obj().unwrap().contains_key("dotfiles"));
    }

    #[test]
    fn other_keys_are_left_alone_and_in_order() {
        let before =
            r#"{"herdr-agent-state":{"Stop":[{"command":"h"}]},"mine":{"Stop":[{"command":"m"}]}}"#;
        let after = run(before, OWN).unwrap();
        let keys: Vec<_> = after.as_obj().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["herdr-agent-state", "mine", "dotfiles"]);
        assert_eq!(
            after.as_obj().unwrap()["herdr-agent-state"],
            parse(r#"{"Stop":[{"command":"h"}]}"#).unwrap()
        );
    }

    #[test]
    fn the_owned_key_is_replaced_not_merged() {
        let before = r#"{"dotfiles":{"Stop":[{"command":"stale"}]}}"#;
        let after = run(before, OWN).unwrap();
        let own = &after.as_obj().unwrap()["dotfiles"];
        assert!(own.as_obj().unwrap().get("Stop").is_none());
        assert!(own.as_obj().unwrap().get("SessionStart").is_some());
    }

    #[test]
    fn an_empty_declaration_removes_only_the_owned_key() {
        let before =
            r#"{"dotfiles":{"Stop":[{"command":"x"}]},"other":{"Stop":[{"command":"o"}]}}"#;
        let after = run(before, r#"{"name":"dotfiles","hooks":{}}"#).unwrap();
        let keys: Vec<_> = after.as_obj().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["other"]);
    }

    #[test]
    fn applying_twice_changes_nothing() {
        let once = run("{}", OWN).unwrap();
        let mut twice = once.clone();
        apply(&mut twice, &spec(OWN)).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_shape_agy_would_silently_drop_is_rejected() {
        for bad in [
            // 知らないイベント(Claude の名前)
            r#"{"name":"d","hooks":{"UserPromptSubmit":[{"command":"x"}]}}"#,
            // 組が要るイベントに、ハンドラを直接並べた
            r#"{"name":"d","hooks":{"PreToolUse":[{"command":"x"}]}}"#,
            // matcher の無い組
            r#"{"name":"d","hooks":{"PostToolUse":[{"hooks":[{"command":"x"}]}]}}"#,
            // 組を使えないイベントに、組を置いた
            r#"{"name":"d","hooks":{"Stop":[{"matcher":"x","hooks":[{"command":"x"}]}]}}"#,
            // 空の command
            r#"{"name":"d","hooks":{"Stop":[{"command":" "}]}}"#,
            // type が command でない
            r#"{"name":"d","hooks":{"Stop":[{"type":"http","command":"x"}]}}"#,
            // timeout が正の整数でない
            r#"{"name":"d","hooks":{"Stop":[{"command":"x","timeout":0}]}}"#,
            // 未知のハンドラのキー(Claude の `if`)
            r#"{"name":"d","hooks":{"Stop":[{"command":"x","if":"Bash(gh *)"}]}}"#,
        ] {
            assert!(run("{}", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_root_that_is_not_an_object_is_an_error() {
        assert!(run("[]", OWN).is_err());
    }
}
