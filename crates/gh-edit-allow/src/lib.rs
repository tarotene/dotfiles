//! gh-edit-allow — このセッションが作成した PR/Issue への `gh` 書き込みを、
//! 検証付きでプログラム的に allow する hook(#392、docs/claude/gh-edit-allow.md)。
//!
//! 1 バイナリで 2 役を持ち、`hook_event_name` で分岐する:
//!
//! - PostToolUse(記録役): `gh pr create` / `gh issue create` が成功した
//!   ときに `tool_response.stdout` に出る URL を、session_id キーの台帳に積む。
//!   URL が出ていなければ作成は失敗しているので、何も積まない。
//! - PreToolUse(判定役): `gh pr edit` / `gh issue edit` の対象が台帳にあれば
//!   allow を返す。`gh issue create` は、同じリポジトリに対してこのセッションが
//!   既に作成した PR/Issue があれば allow を返す。それ以外は何も出さない
//!   (deny ではない — 通常の確認フローに落ちる)。
//!
//! ネットワークには出ない。台帳に無い番号は、編集対象として表現できない。

pub mod gh;

use gh::{GhCommand, Kind, Verb};
use hook_io::{shell, HookInput, PermissionDecision, SessionLedger};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 台帳の 1 レコード: `"<kind> <owner/repo> <number>"`(owner/repo は小文字)。
pub fn record_key(kind: Kind, nwo: &str, number: u64) -> String {
    format!("{} {} {}", kind.as_str(), nwo.to_ascii_lowercase(), number)
}

/// PostToolUse: 作成された PR/Issue の URL を台帳に積み、積んだレコードを返す。
pub fn record(input: &HookInput, ledger: &SessionLedger) -> Vec<String> {
    let Some(cmd) = input.bash_command() else {
        return Vec::new();
    };
    let stdout = match &input.tool_response {
        Value::Object(o) => o.get("stdout").and_then(Value::as_str).unwrap_or(""),
        Value::String(s) => s.as_str(),
        _ => "",
    };
    let mut added = Vec::new();
    for line in stdout.lines() {
        let Some((kind, nwo, n)) = gh::parse_url(line.trim()) else {
            continue;
        };
        let expected = match kind {
            Kind::Pr => "gh pr create",
            Kind::Issue => "gh issue create",
        };
        if !cmd.contains(expected) {
            continue;
        }
        let key = record_key(kind, &nwo, n);
        if !ledger.contains(&input.session_id, &key)
            && ledger.append(&input.session_id, &key).is_ok()
        {
            added.push(key);
        }
    }
    added
}

/// PreToolUse: allow するなら理由付きの判定を返す。判定しないなら `None`。
///
/// `origin_nwo` は `-R` が無いときの対象リポジトリ(cwd の `remote.origin.url`)。
pub fn check(
    input: &HookInput,
    ledger: &SessionLedger,
    origin_nwo: Option<&str>,
) -> Option<PermissionDecision> {
    let words = shell::split(input.bash_command()?)?;
    let c: GhCommand = gh::parse(&words)?;
    let default_nwo = match &c.repo {
        Some(r) => Some(gh::normalize_repo(r)?),
        None => origin_nwo.map(str::to_ascii_lowercase),
    };
    let records = ledger.records(&input.session_id);
    match c.verb {
        Verb::Edit => {
            if c.positionals.is_empty() || (c.kind == Kind::Pr && c.positionals.len() > 1) {
                return None;
            }
            let mut targets = Vec::new();
            for p in &c.positionals {
                let key = if let Some((kind, nwo, n)) = gh::parse_url(p) {
                    if kind != c.kind {
                        return None;
                    }
                    record_key(kind, &nwo, n)
                } else {
                    record_key(c.kind, default_nwo.as_deref()?, gh::parse_number(p)?)
                };
                if !records.contains(&key) {
                    return None;
                }
                targets.push(key);
            }
            Some(PermissionDecision::allow(format!(
                "gh {} edit: このセッションが作成した {}(gh-edit-allow)",
                c.kind.as_str(),
                targets.join(", ")
            )))
        }
        Verb::Create => {
            if c.kind != Kind::Issue || !c.positionals.is_empty() {
                return None;
            }
            let nwo = default_nwo?;
            let seen = records
                .iter()
                .any(|r| r.split(' ').nth(1).is_some_and(|x| x == nwo));
            seen.then(|| {
                PermissionDecision::allow(format!(
                    "gh issue create: このセッションが既に {nwo} に PR/Issue を作成している(gh-edit-allow)"
                ))
            })
        }
    }
}

/// `${GH_EDIT_ALLOW_DIR:-$HOME/.claude/gh-edit-allow}`。
fn base_dir() -> Option<PathBuf> {
    match std::env::var_os("GH_EDIT_ALLOW_DIR") {
        Some(d) if !d.is_empty() => Some(d.into()),
        _ => Some(Path::new(&std::env::var_os("HOME")?).join(".claude/gh-edit-allow")),
    }
}

/// 台帳の置き場所: `<base>/state/<session_id>.ledger`。
pub fn default_ledger() -> Option<SessionLedger> {
    Some(SessionLedger::new(base_dir()?.join("state"), "ledger"))
}

/// hook 全体を止める skip 機構: `SKIP_GH_EDIT_ALLOW=1` か `<base>/skip` の存在
/// (`stack-base-guard.sh` / `plan-fresh-gate.sh` と同じ形)。
pub fn skipped() -> bool {
    std::env::var_os("SKIP_GH_EDIT_ALLOW").is_some_and(|v| v == "1")
        || base_dir().is_some_and(|d| d.join("skip").exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(event: &str, cmd: &str, stdout: Option<&str>) -> HookInput {
        let mut v = json!({
            "hook_event_name": event,
            "session_id": "sess-1",
            "tool_name": "Bash",
            "tool_input": {"command": cmd},
        });
        if let Some(s) = stdout {
            v["tool_response"] = json!({"stdout": s, "stderr": "", "interrupted": false});
        }
        HookInput::parse(&v.to_string()).unwrap()
    }

    fn ledger() -> (tempfile::TempDir, SessionLedger) {
        let d = tempfile::tempdir().unwrap();
        let l = SessionLedger::new(d.path().join("state"), "ledger");
        (d, l)
    }

    #[test]
    fn records_created_pr_and_issue() {
        let (_d, l) = ledger();
        let i = input(
            "PostToolUse",
            "git push -u origin x && gh pr create --title t --body b",
            Some("remote: ...\nhttps://github.com/Octo/Hello/pull/12\n"),
        );
        assert_eq!(record(&i, &l), vec!["pr octo/hello 12"]);
        // 同じレコードは二重に積まない
        assert!(record(&i, &l).is_empty());
        let i = input(
            "PostToolUse",
            "gh issue create --title t --body b",
            Some("https://github.com/octo/hello/issues/13\n"),
        );
        assert_eq!(record(&i, &l), vec!["issue octo/hello 13"]);
    }

    #[test]
    fn does_not_record_without_create_or_url() {
        let (_d, l) = ledger();
        // create 失敗(URL なし)
        let i = input(
            "PostToolUse",
            "gh pr create --title t",
            Some("error: ...\n"),
        );
        assert!(record(&i, &l).is_empty());
        // create 以外のコマンドの出力に URL があっても積まない
        let i = input(
            "PostToolUse",
            "gh pr view 12 --json url -q .url",
            Some("https://github.com/o/r/pull/12\n"),
        );
        assert!(record(&i, &l).is_empty());
        // pr create の出力に issue URL が混じっていても種類が合わなければ積まない
        let i = input(
            "PostToolUse",
            "gh pr create --title t",
            Some("see https://github.com/o/r/issues/1\nhttps://github.com/o/r/issues/1\n"),
        );
        assert!(record(&i, &l).is_empty());
    }

    #[test]
    fn allows_only_own_targets() {
        let (_d, l) = ledger();
        l.append("sess-1", "pr octo/hello 12").unwrap();
        l.append("sess-1", "issue octo/hello 13").unwrap();
        let origin = Some("Octo/Hello");

        let ok = |cmd: &str| check(&input("PreToolUse", cmd, None), &l, origin);
        assert!(ok("gh pr edit 12 --title 'new'").is_some());
        assert!(ok("gh pr edit https://github.com/octo/hello/pull/12 --body x").is_some());
        assert!(ok("gh pr edit 12 -R octo/hello --add-label bug").is_some());
        assert!(ok("gh issue edit 13 --body 'x | y'").is_some());

        // 台帳に無い番号・別リポジトリ・種類違い・複合コマンドは判定しない
        assert!(ok("gh pr edit 11 --title x").is_none());
        assert!(ok("gh pr edit 12 -R other/repo --title x").is_none());
        assert!(ok("gh issue edit 12 --title x").is_none());
        assert!(ok("gh pr edit https://github.com/octo/hello/issues/13").is_none());
        assert!(ok("gh issue edit 13 14 --title x").is_none());
        assert!(ok("gh pr edit 12 --title x; gh pr edit 11 --title y").is_none());
        assert!(ok("gh pr edit --title x").is_none());
        assert!(ok("gh pr edit my-branch --title x").is_none());
        assert!(ok("gh pr merge 12").is_none());

        // 別セッションの台帳は見ない
        let other = HookInput {
            session_id: "sess-2".into(),
            ..input("PreToolUse", "gh pr edit 12 --title x", None)
        };
        assert!(check(&other, &l, origin).is_none());
        // origin 不明で -R も無い番号指定は解決できない
        assert!(check(&input("PreToolUse", "gh pr edit 12", None), &l, None).is_none());
    }

    #[test]
    fn issue_create_needs_prior_record_in_same_repo() {
        let (_d, l) = ledger();
        let c =
            |cmd: &str, origin: Option<&str>| check(&input("PreToolUse", cmd, None), &l, origin);
        assert!(c("gh issue create --title t --body b", Some("octo/hello")).is_none());
        l.append("sess-1", "pr octo/hello 12").unwrap();
        assert!(c("gh issue create --title t --body b", Some("octo/hello")).is_some());
        assert!(c("gh issue create -R octo/hello --title t", None).is_some());
        assert!(c(
            "gh issue create -R other/repo --title t",
            Some("octo/hello")
        )
        .is_none());
        assert!(c("gh issue create --title t", None).is_none());
        // pr create は permissions.allow 側の管轄
        assert!(c("gh pr create --title t", Some("octo/hello")).is_none());
    }
}
