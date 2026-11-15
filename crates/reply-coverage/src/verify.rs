//! 照会: この下書きは、この会話について、検査に合格しているか。下書きの作成口の関所(hook-router)と、
//! 裁定ページの生成が、同じ関数を呼ぶ。LLM は呼ばない(記録を引くだけ)。

use crate::store::{Store, StoredThread};
use crate::text;

/// 保存した会話の有効な新しさ(24 時間)。これより古い全文では、相手の新しい返信を見落としうる。
pub const MAX_THREAD_AGE_SECS: u64 = 24 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// 会話の全文が保存されていない(読んでいない)。
    NoThread,
    /// 保存した全文が古い。
    StaleThread { age_secs: u64 },
    /// この下書き(とこの全文)の検査の記録が無い。
    Unchecked,
    /// 検査したが不合格だった。
    Failed {
        unanswered: Vec<String>,
        apology_missing: bool,
    },
}

impl Failure {
    /// エージェントへの拒否の文面。次の一手を書く。
    pub fn message(&self, thread_id: &str) -> String {
        match self {
            Failure::NoThread => format!(
                "会話 {thread_id} の全文が保存されていません。返信の下書きの前に、会話を最初から読んでください\
                 (Gmail は get_thread を呼ぶと自動で保存されます。ブラウザの媒体は browser_snapshot の filename で\
                 保存し、reply-coverage import で取り込みます)。"
            ),
            Failure::StaleThread { age_secs } => format!(
                "会話 {thread_id} の保存済みの全文は {} 時間前のものです(24 時間まで有効)。\
                 相手の新しい返信を見落とさないよう、会話を取り直してください。",
                age_secs / 3600
            ),
            Failure::Unchecked => format!(
                "この下書きは、会話 {thread_id} の相手の依頼への応答を検査していません。最終稿(ja-polish の後)を\
                 ファイルに書き、`reply-coverage check --thread {thread_id} --draft <ファイル>` を実行して、\
                 合格した本文のまま作成してください。"
            ),
            Failure::Failed {
                unanswered,
                apology_missing,
            } => {
                let mut s = format!(
                    "この下書きは、会話 {thread_id} の相手の依頼に答え漏れがあり、検査で不合格でした。"
                );
                for q in unanswered {
                    s.push_str(&format!("\n- 未回答: 「{q}」"));
                }
                if *apology_missing {
                    s.push_str("\n- 前の返信が落としていた依頼に答えるのに、軽い詫びがありません。");
                }
                s.push_str("\n直してから、もう一度 `reply-coverage check` を実行してください。");
                s
            }
        }
    }
}

/// 照会する。`thread` は、返信先のメッセージの id から引いた保存済みの会話(関所)か、id で引いた会話(CLI)。
pub fn verify(
    store: &Store,
    thread: Option<&StoredThread>,
    draft: &str,
    now: u64,
) -> Result<(), Failure> {
    let thread = thread.ok_or(Failure::NoThread)?;
    let age = now.saturating_sub(thread.fetched_ts);
    if age > MAX_THREAD_AGE_SECS {
        return Err(Failure::StaleThread { age_secs: age });
    }
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
            apology_missing: r.apology_missing,
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

    #[test]
    fn messages_name_the_next_step() {
        assert!(Failure::NoThread.message("gmail:t").contains("get_thread"));
        assert!(Failure::Unchecked
            .message("gmail:t")
            .contains("reply-coverage check"));
        let m = Failure::Failed {
            unanswered: vec!["日程の候補".to_string()],
            apology_missing: true,
        }
        .message("gmail:t");
        assert!(m.contains("日程の候補") && m.contains("詫び"));
    }
}
