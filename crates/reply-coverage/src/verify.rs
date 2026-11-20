//! 照会: この下書きは、この会話について、検査に合格しているか。下書きの作成口の関所(hook-router)と、
//! 裁定ページの生成が、同じ関数を呼ぶ。LLM は呼ばない(記録を引くだけ)。

use crate::store::{Store, StoredThread};
use crate::text;

/// 保存した会話の有効な新しさ(24 時間)。これより古い全文では、相手の新しい返信を見落としうる。
pub const MAX_THREAD_AGE_SECS: u64 = 24 * 3600;

/// 求人・スカウトなどのメッセージを、通知メールで知らせる媒体の差出人の目印(小文字で、差出人の文字列に含まれる語)。
/// 通知から返信を組むと、Gmail のスレッドには、本人が媒体で送った文が無い(F70)。
pub const NOTIFIER_MARKERS: &[&str] = &["linkedin", "lapras", "findy"];

/// Gmail の会話(`gmail:` の id)のうち、相手のメッセージの差出人が、媒体の通知メールのものか。返すのは、当たった目印。
/// 保存した全文の見出し(`## them <時刻> <差出人>`)の差出人を見る。
pub fn media_notification(thread: &StoredThread) -> Option<&'static str> {
    if !thread.id.starts_with("gmail:") {
        return None;
    }
    thread
        .text
        .lines()
        .filter(|l| l.starts_with("## them"))
        .find_map(|l| {
            let l = l.to_lowercase();
            NOTIFIER_MARKERS.iter().copied().find(|m| l.contains(m))
        })
}

/// 24 時間以内に取り込んだ、Gmail 以外(ブラウザで読んだ媒体の会話)の保存があるか。
fn has_recent_browser_import(store: &Store, now: u64) -> bool {
    store.all_threads().iter().any(|t| {
        !t.id.starts_with("gmail:") && now.saturating_sub(t.fetched_ts) <= MAX_THREAD_AGE_SECS
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// 会話の全文が保存されていない(読んでいない)。
    NoThread,
    /// 保存した全文が古い。
    StaleThread { age_secs: u64 },
    /// 媒体(LinkedIn など)の通知メールの会話で、ブラウザで開いた会話の取り込みが無い(F70)。
    NotificationOnly { marker: &'static str },
    /// この下書き(とこの全文)の検査の記録が無い。
    Unchecked,
    /// 検査したが不合格だった。
    Failed { unanswered: Vec<String> },
}

impl Failure {
    /// エージェントへの拒否の文面。次の一手を書く。
    pub fn message(&self, thread_id: &str) -> String {
        match self {
            Failure::NoThread => format!(
                "会話 {thread_id} の全文が保存されていません。返信の下書きの前に、会話を最初から読んでください\
                 (Gmail は get_thread を呼ぶと自動で保存されます。結果が大きくてファイルに保存されたときは、\
                 `reply-coverage import-gmail --file <そのファイル>` で取り込みます。ブラウザの媒体は browser_snapshot の\
                 filename で保存し、reply-coverage import で取り込みます)。"
            ),
            Failure::StaleThread { age_secs } => format!(
                "会話 {thread_id} の保存済みの全文は {} 時間前のものです(24 時間まで有効)。\
                 相手の新しい返信を見落とさないよう、会話を取り直してください。",
                age_secs / 3600
            ),
            Failure::NotificationOnly { marker } => format!(
                "会話 {thread_id} は、{marker} などの媒体の通知メールです。通知は、同じ件名でも別のスレッドに分かれ、本人が媒体で\
                 送った文は Gmail に残りません(2026-11-19、通知のスレッドだけで組んだ下書きが検査を通ったが、同じ会話で本人が\
                 面談を見送り、相手が承知していた)。ブラウザでその会話を開き、先頭まで読み込んで、browser_snapshot の filename で\
                 保存し、`reply-coverage import` で取り込んでから、その会話に `reply-coverage check` をかけてください\
                 (config/agents/reference/browser-operations.md「返信を下書きする前に、会話の全文を保存する」)。"
            ),
            Failure::Unchecked => format!(
                "この下書きは、会話 {thread_id} の相手の依頼への応答を検査していません。最終稿(ja-polish の後)を\
                 ファイルに書き、`reply-coverage check --thread {thread_id} --draft <ファイル>` を実行して、\
                 合格した本文のまま作成してください。"
            ),
            Failure::Failed { unanswered } => {
                let mut s = format!(
                    "この下書きは、会話 {thread_id} の相手の依頼に答え漏れがあり、検査で不合格でした。"
                );
                for q in unanswered {
                    s.push_str(&format!("\n- 未回答: 「{q}」"));
                }
                s.push_str("\n直してから、もう一度 `reply-coverage check` を実行してください。");
                s
            }
        }
    }
}

/// 返信案を作る・見せる前の、会話の読み込みの確認(下書きの検査より前の段)。保存した全文があり、24 時間以内で、
/// 媒体の通知メールの会話なら、ブラウザで読んだ会話の取り込みもあること。`verify` の前半と、`ready` が同じ関数を呼ぶ。
pub fn thread_ready(store: &Store, thread: Option<&StoredThread>, now: u64) -> Result<(), Failure> {
    let thread = thread.ok_or(Failure::NoThread)?;
    let age = now.saturating_sub(thread.fetched_ts);
    if age > MAX_THREAD_AGE_SECS {
        return Err(Failure::StaleThread { age_secs: age });
    }
    if let Some(marker) = media_notification(thread) {
        if !has_recent_browser_import(store, now) {
            return Err(Failure::NotificationOnly { marker });
        }
    }
    Ok(())
}

/// 照会する。`thread` は、返信先のメッセージの id から引いた保存済みの会話(関所)か、id で引いた会話(CLI)。
pub fn verify(
    store: &Store,
    thread: Option<&StoredThread>,
    draft: &str,
    now: u64,
) -> Result<(), Failure> {
    thread_ready(store, thread, now)?;
    let thread = thread.ok_or(Failure::NoThread)?;
    let (draft_sha, thread_sha) = (text::sha(draft), text::sha(&thread.text));
    let latest =
        store.records().into_iter().rev().find(|r| {
            r.thread == thread.id && r.draft_sha == draft_sha && r.thread_sha == thread_sha
        });
    match latest {
        None => Err(Failure::Unchecked),
        Some(r) if r.verdict == "pass" => Ok(()),
        Some(r) => Err(Failure::Failed {
            unanswered: r.unanswered,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Record;

    fn thread(ts: u64, text: &str) -> StoredThread {
        StoredThread {
            id: "gmail:t".to_string(),
            fetched_ts: ts,
            format: "normalized".to_string(),
            message_ids: vec![],
            text: text.to_string(),
        }
    }

    fn rec(t: &StoredThread, draft: &str, verdict: &str, ts: u64) -> Record {
        Record {
            ts_unix: ts,
            thread: t.id.clone(),
            thread_sha: text::sha(&t.text),
            draft_sha: text::sha(draft),
            verdict: verdict.to_string(),
            model: "m".to_string(),
            asks: 1,
            unanswered: vec!["日程の候補".to_string()],
            apology_missing: false,
        }
    }

    #[test]
    fn no_thread_stale_unchecked_pass_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let t = thread(1000, "全文");
        assert_eq!(verify(&s, None, "d", 1001), Err(Failure::NoThread));
        assert_eq!(
            verify(&s, Some(&t), "d", 1000 + MAX_THREAD_AGE_SECS + 1),
            Err(Failure::StaleThread {
                age_secs: MAX_THREAD_AGE_SECS + 1
            })
        );
        assert_eq!(verify(&s, Some(&t), "d", 1001), Err(Failure::Unchecked));

        s.append_record(&rec(&t, "d", "fail", 1002)).unwrap();
        assert!(matches!(
            verify(&s, Some(&t), "d", 1003),
            Err(Failure::Failed { .. })
        ));

        // 同じ下書きの、より新しい合格の記録が、古い不合格に勝つ。
        s.append_record(&rec(&t, "d", "pass", 1004)).unwrap();
        assert_eq!(verify(&s, Some(&t), "d", 1005), Ok(()));
    }

    #[test]
    fn a_pass_does_not_carry_over_to_an_edited_draft_or_a_refetched_thread() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let t = thread(1000, "全文");
        s.append_record(&rec(&t, "d", "pass", 1001)).unwrap();
        assert_eq!(verify(&s, Some(&t), "d", 1002), Ok(()));
        // 下書きを 1 字でも直したら、合格は引き継がれない。
        assert_eq!(verify(&s, Some(&t), "d!", 1002), Err(Failure::Unchecked));
        // 会話を取り直して中身が変わったら(相手の新しい返信)、合格は引き継がれない。
        let t2 = thread(1500, "全文 + 相手の新しい返信");
        assert_eq!(verify(&s, Some(&t2), "d", 1501), Err(Failure::Unchecked));
    }

    #[test]
    fn whitespace_only_edits_keep_the_pass() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let t = thread(1000, "全文");
        s.append_record(&rec(&t, "a b", "pass", 1001)).unwrap();
        assert_eq!(verify(&s, Some(&t), "a\n\nb", 1002), Ok(()));
    }

    fn thread_from(id: &str, ts: u64, text: &str) -> StoredThread {
        StoredThread {
            id: id.to_string(),
            fetched_ts: ts,
            format: "normalized".to_string(),
            message_ids: vec![],
            text: text.to_string(),
        }
    }

    #[test]
    fn a_media_notification_thread_needs_a_recent_browser_import() {
        // F70: 通知メールの Gmail のスレッドだけでは、本人が媒体で送った文が無い。ブラウザで読んだ会話の取り込みを求める。
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        let t = thread_from(
            "gmail:t9",
            1000,
            "# thread: gmail:t9\n## them 2026-11-19T10:00:00Z Someone via LinkedIn <messages-noreply@linkedin.com>\n新しいメッセージ",
        );
        assert_eq!(media_notification(&t), Some("linkedin"));
        assert_eq!(
            thread_ready(&s, Some(&t), 1001),
            Err(Failure::NotificationOnly { marker: "linkedin" })
        );
        // verify も、記録の前に同じ理由で止まる。
        assert_eq!(
            verify(&s, Some(&t), "d", 1001),
            Err(Failure::NotificationOnly { marker: "linkedin" })
        );
        // 古いブラウザの取り込みは数えない。
        s.save_thread(&thread_from("linkedin:old", 1, "## them x\nhi"))
            .unwrap();
        assert!(thread_ready(&s, Some(&t), 1000 + MAX_THREAD_AGE_SECS).is_err());
        // 24 時間以内にブラウザで読んだ会話を取り込めば、通る。
        s.save_thread(&thread_from("linkedin:conv", 990, "## them x\nhi"))
            .unwrap();
        assert_eq!(thread_ready(&s, Some(&t), 1001), Ok(()));
    }

    #[test]
    fn ordinary_gmail_threads_and_browser_threads_are_not_media_notifications() {
        let plain = thread_from(
            "gmail:t1",
            1000,
            "# thread: gmail:t1\n## them 2026-11-01T10:00:00Z recruiter@example.invalid\nお電話できますか。",
        );
        assert_eq!(media_notification(&plain), None);
        // 本人(me)の差出人に媒体の語があっても、相手の通知とは数えない。
        let mine = thread_from(
            "gmail:t2",
            1000,
            "# thread: gmail:t2\n## me 2026-11-01T10:00:00Z me@linkedin.example.invalid\nはい。",
        );
        assert_eq!(media_notification(&mine), None);
        // ブラウザの媒体の会話(gmail: でない id)は対象外(通知メールでない)。
        let browser = thread_from("linkedin:c", 1000, "## them 2026-11-01 LinkedIn\nhi");
        assert_eq!(media_notification(&browser), None);
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path());
        assert_eq!(thread_ready(&s, Some(&plain), 1001), Ok(()));
        assert_eq!(thread_ready(&s, None, 1001), Err(Failure::NoThread));
    }

    #[test]
    fn messages_name_the_next_step() {
        let m = Failure::NotificationOnly { marker: "linkedin" }.message("gmail:t");
        assert!(m.contains("reply-coverage import") && m.contains("通知メール"));
        assert!(Failure::NoThread.message("gmail:t").contains("get_thread"));
        assert!(Failure::Unchecked
            .message("gmail:t")
            .contains("reply-coverage check"));
        let m = Failure::Failed {
            unanswered: vec!["日程の候補".to_string()],
        }
        .message("gmail:t");
        assert!(m.contains("日程の候補") && m.contains("reply-coverage check"));
    }
}
