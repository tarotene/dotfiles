//! routines-write-guard — deny a cron-bearing `RemoteTrigger` create/update
//! whose body doesn't carry the structure that `claude-routines`'
//! `routines-plan.sh` always produces (ADR-0000-routines-declaration-in-repo
//! D7).
//!
//! `RemoteTrigger` は Bash ツールではないため、`rulesets-write-guard` が
//! 使う「`BYPASS=` env var 代入をコマンド文字列の字頭から読む」方式
//! (`hook_io::shell::split` 前提)は使えない — 1 回のツール呼び出しに
//! コマンド文字列という概念自体が無い。代わりに、正規の経路
//! (`claude-routines` スキルの `build-body`/`classify` が出す body)が
//! 必ず満たす構造(1. `name` が `<owner>/<repo>:<routine-name>` の名前
//! 空間キーであること、2. prompt 最終行が `routine-spec: <sha256 hex>`
//! であること)を判定条件にする。bypass は無い — 意図的な手動書き込みは
//! Web UI 経由で行う想定。
//!
//! この guard が対象にするのは cron を伴う create/update だけ(`body` に
//! 非空の `cron_expression` を含むもの)。run-once/webhook trigger の
//! create、read 系アクション(list/get/run/list_runs/get_run_log)は
//! 対象外。クラウドの meta connector(`Claude_Code_Remote` MCP)経由の
//! 書き込みはこの hook からは見えない — そちらは auditor の unmanaged
//! 検出で拾う(decision-colocation の Codex/Copilot adapter 不要判断
//! (ADR-396 D8)と同じ理由: 対象ツールが Claude Code 固有のため)。

use hook_io::{HookInput, PermissionDecision};
use serde_json::Value;

/// "<owner>/<repo>:<routine-name>" の形かどうか。owner/repo は非空、
/// routine-name は小文字英数字とハイフンのみ。
fn is_namespaced_name(name: &str) -> bool {
    let Some((repo_part, routine_name)) = name.split_once(':') else {
        return false;
    };
    if routine_name.is_empty()
        || !routine_name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return false;
    }
    matches!(repo_part.split('/').collect::<Vec<_>>().as_slice(), [owner, repo] if !owner.is_empty() && !repo.is_empty())
}

/// prompt の最終行が `routine-spec: <64桁の小文字16進数>` かどうか。
fn ends_with_routine_spec(content: &str) -> bool {
    let Some(last_line) = content.trim_end().rsplit('\n').next() else {
        return false;
    };
    let Some(hash) = last_line.strip_prefix("routine-spec: ") else {
        return false;
    };
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn prompt_content(body: &Value) -> Option<&str> {
    body.get("job_config")?
        .get("ccr")?
        .get("events")?
        .get(0)?
        .get("data")?
        .get("message")?
        .get("content")?
        .as_str()
}

/// PreToolUse: deny するなら理由付きの判定を返す。判定しない(deny しない)
/// なら `None`。
pub fn check(input: &HookInput) -> Option<PermissionDecision> {
    if input.tool_name != "RemoteTrigger" {
        return None;
    }
    let action = input.tool_input.get("action")?.as_str()?;
    if action != "create" && action != "update" {
        return None;
    }
    let body = input.tool_input.get("body")?;
    let cron = body.get("cron_expression")?.as_str()?;
    if cron.is_empty() {
        return None;
    }

    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    if !is_namespaced_name(name) {
        return Some(PermissionDecision::deny(
            "cron routine の直接 create/update は deny。name が <owner>/<repo>:<name> の \
             名前空間キーではありません — claude-routines スキルの build-body/classify を \
             使ってください(docs/claude/claude-routines.md、routines-write-guard)",
        ));
    }

    let content = prompt_content(body).unwrap_or("");
    if !ends_with_routine_spec(content) {
        return Some(PermissionDecision::deny(
            "cron routine の直接 create/update は deny。prompt 末尾に routine-spec 注記が \
             ありません — claude-routines スキルの build-body/classify を使ってください \
             (docs/claude/claude-routines.md、routines-write-guard)",
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(action: &str, body: Value) -> HookInput {
        let v = json!({
            "hook_event_name": "PreToolUse",
            "session_id": "sess-1",
            "tool_name": "RemoteTrigger",
            "tool_input": {"action": action, "body": body},
        });
        HookInput::parse(&v.to_string()).unwrap()
    }

    fn cron_body(name: &str, content: &str) -> Value {
        json!({
            "name": name,
            "cron_expression": "0 3 * * *",
            "job_config": {"ccr": {"events": [{"data": {"message": {"content": content}}}]}}
        })
    }

    const GOOD_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn allows_wellformed_skill_output() {
        let body = cron_body(
            "owner/repo:nightly-check",
            &format!("do the thing\n\nroutine-spec: {GOOD_HASH}"),
        );
        assert!(check(&input("create", body.clone())).is_none());
        assert!(check(&input("update", body)).is_none());
    }

    #[test]
    fn denies_missing_namespace() {
        let body = cron_body(
            "nightly-check",
            &format!("do the thing\n\nroutine-spec: {GOOD_HASH}"),
        );
        assert!(check(&input("create", body)).is_some());
    }

    #[test]
    fn denies_missing_annotation() {
        let body = cron_body("owner/repo:nightly-check", "do the thing, no annotation");
        assert!(check(&input("update", body)).is_some());
    }

    #[test]
    fn denies_malformed_hash() {
        let body = cron_body(
            "owner/repo:nightly-check",
            "do the thing\n\nroutine-spec: not-actually-hex",
        );
        assert!(check(&input("create", body)).is_some());
    }

    #[test]
    fn allows_run_once_without_cron_expression() {
        let body = json!({
            "name": "adhoc probe",
            "run_once_at": "2027-01-01T00:00:00Z",
            "job_config": {"ccr": {"events": [{"data": {"message": {"content": "no annotation"}}}]}}
        });
        assert!(check(&input("create", body)).is_none());
    }

    #[test]
    fn ignores_read_actions_and_other_tools() {
        assert!(check(&input("list", json!({}))).is_none());
        assert!(check(&input("get", json!({}))).is_none());
        let i = HookInput::parse(
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi"},
            })
            .to_string(),
        )
        .unwrap();
        assert!(check(&i).is_none());
    }

    #[test]
    fn no_body_or_no_action_is_not_judged() {
        let i = HookInput::parse(
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "RemoteTrigger",
                "tool_input": {"action": "create"},
            })
            .to_string(),
        )
        .unwrap();
        assert!(check(&i).is_none());
    }
}
