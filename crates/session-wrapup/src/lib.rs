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
    let pending = if non_empty_file(&inbox) {
        newline_count(&inbox)
    } else {
        0
    };
    let mut ctx = format!(
        "<hook-directive source=\"wrapup-session-start\" event=\"SessionStart\">\n\
         wrap-up inbox for this project: {inbox}\n\
         When something outside the current task's scope is worth an Issue (a sign of a\n\
         bug, debt, an improvement idea), append it right then as one line per finding:\n\
         \x20 '{gate}' --add '{inbox}' '{{\"ts\": \"<ISO8601>\", \"title\": \"<Issue title>\", \"detail\": \"<what and why>\"}}'\n\
         Do not edit the inbox directly (always go through --add). The Stop hook at the\n\
         end of the turn points to the filing procedure for appended items."
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThe inbox currently holds {pending} unprocessed item(s) (including leftovers from past sessions)."
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
            | "--procedure"
            | "--migrate"
            | "--stamp-feedback-session"
            | "--add"
            | "--check-dup"
            | "--chores-begin"
            | "--chores-end"
            | "--mark-filed"
            | "--selftest",
        ) => issue_inbox::gate_main(self_path, args),
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
