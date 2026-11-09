//! Sequential session lifecycle orchestration. State is owned by each component.
use agent_session::{Agent, HookEvent, Reply};
use feedback_trial as trial;
use issue_inbox::*;
pub use issue_inbox::{read_hook_stdin, self_path, self_path_from_argv0};
use session_retrospective as retro;
use std::io::Write;

/// 片付け(`/my-cleanup`)を起動時に提案する、未裁定の件数のしきい値。
pub const CLEANUP_PENDING_THRESHOLD: usize = 10;
/// 片付けを提案する、最後の行(初出か再発)からの日数。気づきが失効する日数(`vault::LAPSE_AFTER_DAYS`)の
/// 半分で、「あと半分の日数以内に、黙って失効する気づきがある」ことを表す。時計は失効と同じ。
pub const CLEANUP_STALE_DAYS: i64 = vault::LAPSE_AFTER_DAYS / 2;

/// 未裁定の件数 `pending` と、最後の行からいちばん日数がたっている未裁定の経過日数
/// `oldest_age_days`(vault が無ければ `None`)から、片付けを提案するかを決める。
/// 前回いつ片付けたかの記録は持たない(新しい記録を足さず、既存の inbox の記録から導く)。
pub fn should_suggest_cleanup(pending: usize, oldest_age_days: Option<i64>) -> bool {
    pending >= CLEANUP_PENDING_THRESHOLD || oldest_age_days.is_some_and(|d| d >= CLEANUP_STALE_DAYS)
}

fn cleanup_suggestion(pending: usize, oldest_age_days: Option<i64>) -> String {
    let age = oldest_age_days.map_or(String::new(), |d| {
        format!(
            "; the oldest was last seen {d} day(s) ago, and open findings lapse after {} days",
            vault::LAPSE_AFTER_DAYS
        )
    });
    format!(
        "\nThe wrap-up inbox of this project has piled up ({pending} open{age}). Propose running /my-cleanup to the user in one short sentence, and run it only if the user wants it; do not start it on your own."
    )
}
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

/// SessionStart hook 本体。注入 JSON(末尾の改行込み)を返す(何も注入しないときは `None`)。
/// 描画は `Reply::render` が持つ。
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
         \x20 '{gate}' --add '{inbox}' '{{\"title\": \"<title>\", \"detail\": \"<what and why>\"}}'\n\
         (the timestamp \"ts\" is added for you; do not build it with $(date))\n\
         A finding becomes a note in the user's vault (inbox/, one note per title; the same title\n\
         adds a row instead of a duplicate) or, on a machine without a vault, a line in the jsonl above.\n\
         It is NOT filed as a GitHub Issue: the user adjudicates findings later with /my-cleanup,\n\
         which fixes them by PR and files an Issue only for the ones the user picks. Do not file an\n\
         Issue for a finding yourself, and do not edit the inbox directly (always go through --add)."
    );
    if pending > 0 {
        ctx.push_str(&format!(
            "\nThere are {pending} unadjudicated finding(s) for this project (including leftovers from past sessions)."
        ));
        let oldest = pending_oldest_age_days(&project, &vault::jst_today());
        if should_suggest_cleanup(pending, oldest) {
            ctx.push_str(&cleanup_suggestion(pending, oldest));
        }
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
    Ok(Some(
        Reply::context(ctx).render(Agent::Claude, HookEvent::SessionStart),
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
        // 起票の手順書と実行中の印は無くなった(気づきは vault に入り、裁定は my-cleanup)。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pending_count_threshold_is_ten() {
        assert!(!should_suggest_cleanup(9, None));
        assert!(should_suggest_cleanup(10, None));
        assert!(should_suggest_cleanup(143, None));
    }

    #[test]
    fn the_age_threshold_is_half_of_the_lapse_period() {
        assert_eq!(CLEANUP_STALE_DAYS, 7);
        assert!(!should_suggest_cleanup(1, Some(6)));
        assert!(should_suggest_cleanup(1, Some(7)));
        assert!(should_suggest_cleanup(1, Some(13)));
    }

    #[test]
    fn either_condition_is_enough_and_no_age_means_only_the_count_counts() {
        assert!(should_suggest_cleanup(10, Some(0)));
        assert!(should_suggest_cleanup(1, Some(7)));
        assert!(!should_suggest_cleanup(9, Some(6)));
        assert!(!should_suggest_cleanup(9, None));
    }

    #[test]
    fn the_suggestion_names_the_entry_and_forbids_starting_it_unasked() {
        let s = cleanup_suggestion(12, Some(8));
        assert!(s.contains("/my-cleanup"), "{s}");
        assert!(s.contains("12 open"), "{s}");
        assert!(s.contains("8 day(s) ago"), "{s}");
        assert!(s.contains("only if the user wants it"), "{s}");
        // vault が無く経過日数を持たないときは、日数の句を出さない。
        assert!(!cleanup_suggestion(10, None).contains("day(s)"));
    }
}
