//! Sequential session lifecycle orchestration. State is owned by each component.
use feedback_trial as trial;
use issue_inbox::*;
pub use issue_inbox::{read_hook_stdin, self_path, self_path_from_argv0};
use session_retrospective as retro;
use std::io::Write;
/// Stop hook 本体。0 = 素通り、2 = ゲート(stderr に指示)。
pub fn stop_hook(self_path: &str, input: &str) -> Code {
    let h = HookJson::parse(input);
    // バックグラウンド処理の完了を待つターンは、作業の終わりではない(#854)。振り返りも
    // 起票の指示も出さず、振り返りの上限 3 回のカウンタも消費しない。
    if waiting_on_background(&h.get("transcript_path", None).unwrap_or_default()) {
        return 0;
    }
    let canonical = std::path::Path::new(self_path)
        .file_name()
        .is_some_and(|n| n == "session-wrapup");
    let trial_path = if canonical {
        format!("{}/feedback-trial", dirname(self_path))
    } else {
        self_path.to_owned()
    };
    match trial::hook(&trial_path, input, false) {
        Ok(Some(message)) => {
            eprintln!("{message}");
            return 2;
        }
        Ok(None) => {}
        Err(error) => eprintln!("trial: {error}; pending state retained, retry next input/resume"),
    }
    // レトロは stop_hook_active で抜けない(複数往復が要る。上限はセッション単位の
    // カウンタ)。PR を作っていないセッションでは台帳を 1 つ読むだけで何も起きない。
    {
        let sid = h.session_id();
        let proj = h.project().unwrap_or_default();
        let transcript = h.get("transcript_path", None).unwrap_or_default();
        if let Some(msg) = retro::stop_check(
            &if canonical {
                format!("{}/session-retrospective", dirname(self_path))
            } else {
                self_path.to_owned()
            },
            &sid,
            &proj,
            &transcript,
        ) {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return 2;
        }
    }
    issue_inbox::stop_check(
        &if canonical {
            format!("{}/issue-inbox", dirname(self_path))
        } else {
            self_path.to_owned()
        },
        input,
        &h,
    )
}
// ---------------------------------------------------------------------------
// SessionStart

#[derive(serde::Serialize)]
struct SessionStartOut<'a> {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: SessionStartInner<'a>,
}

#[derive(serde::Serialize)]
struct SessionStartInner<'a> {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'a str,
    #[serde(rename = "additionalContext")]
    additional_context: &'a str,
}

/// SessionStart hook 本体。注入 JSON を返す(何も注入しないときは `None`)。
/// `self_path` は session-start 自身のパスで、指示文には同じディレクトリの
/// `wrapup-stop-gate` を示す。
pub fn session_start(self_path: &str, input: &str) -> Result<Option<String>, Code> {
    let h = HookJson::parse(input);
    let canonical = std::path::Path::new(self_path)
        .file_name()
        .is_some_and(|n| n == "session-wrapup");
    let gate = format!(
        "{}/{}",
        dirname(self_path),
        if canonical {
            "issue-inbox"
        } else {
            "wrapup-stop-gate"
        }
    );
    let trial_path = if canonical {
        format!("{}/feedback-trial", dirname(self_path))
    } else {
        gate.clone()
    };
    let project = h.project()?;
    if project.is_empty() {
        return Ok(None);
    }
    let _ = migrate_legacy_inbox(&project);
    let inbox = inbox_for(&project);
    let sid = h.session_id();
    // bash 版は `bash "$gate" --stamp-feedback-session "$session_id" || true`
    // だったので、空の session_id は usage エラーで何もしない。
    if !sid.is_empty() {
        stamp_feedback_session(&sid);
    }
    let pending = pending_count(&project);
    let mut ctx = format!(
        "<hook-directive source=\"wrapup-session-start\" event=\"SessionStart\">\n\
         wrap-up inbox for this project: {inbox}\n\
         When something outside the current task's scope is worth recording (a sign of a\n\
         bug, debt, an improvement idea), append it right then as one line per finding:\n\
         \x20 '{gate}' --add '{inbox}' '{{\"ts\": \"<ISO8601>\", \"title\": \"<title>\", \"detail\": \"<what and why>\"}}'\n\
         A finding becomes a note in the user's vault (inbox/, one note per title; the same title\n\
         adds a row instead of a duplicate) or, on a machine without a vault, a line in the jsonl above.\n\
         It is NOT filed as a GitHub Issue: the user adjudicates findings later with /backlog-cleanup,\n\
         which fixes them by PR and files an Issue only for the ones the user picks. Do not file an\n\
         Issue for a finding yourself, and do not edit the inbox directly (always go through --add)."
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThere are {pending} unadjudicated finding(s) for this project (including leftovers from past sessions)."
        ));
    }
    ctx.push_str("\n</hook-directive>");
    match trial::hook(&trial_path, input, true) {
        Ok(Some(message)) => {
            ctx.push('\n');
            ctx.push_str(&message);
        }
        Ok(None) => {}
        Err(error) => eprintln!("trial: {error}; pending state retained, retry next input/resume"),
    }
    let out = SessionStartOut {
        hook_specific_output: SessionStartInner {
            hook_event_name: "SessionStart",
            additional_context: &ctx,
        },
    };
    Ok(Some(
        serde_json::to_string_pretty(&out).expect("serialize session-start output"),
    ))
}

pub fn gate_main(self_path: &str, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--trial") => trial::cli(&args[1..]),
        Some("--retro-add" | "--retro-close" | "--retro-procedure") => retro::cli(self_path, args),
        Some(
            "--inbox-path"
            | "--migrate"
            | "--stamp-feedback-session"
            | "--add"
            | "--check-dup"
            | "--list-open"
            | "--set-status"
            | "--mark-filed"
            | "--selftest",
        ) => issue_inbox::gate_main(self_path, args),
        // 起票の手順書と実行中の印は無くなった(気づきは vault に入り、裁定は backlog-cleanup)。
        // 未知のフラグは Stop hook として動いてしまうので、無言で通さず、はっきり断る。
        Some(flag @ ("--procedure" | "--chores-begin" | "--chores-end")) => {
            eprintln!(
                "session-wrapup: {flag} was removed (ADR-1136): findings are no longer filed at Stop; use --list-open / --set-status"
            );
            1
        }
        _ => stop_hook(self_path, &read_hook_stdin()),
    }
}

const WAIT_STALE_SECS: u64 = 3600;
/// この Stop が、バックグラウンド処理の完了を待つ途中のターンか(#854)。Stop の入力に
/// バックグラウンドの状態は無いので、transcript の開始と完了通知から導く。待ちのターンは
/// 作業の終わりではなく、振り返りの対象は「セッション全体」なので、途中で出すとやり直しになる。
/// transcript が読めないときは `false`(従来どおり判定する)。
pub fn waiting_on_background(transcript_path: &str) -> bool {
    if transcript_path.is_empty() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(transcript_path) else {
        return false;
    };
    !agent_session::transcript::pending_background_tasks(
        &text,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        WAIT_STALE_SECS,
    )
    .is_empty()
}
