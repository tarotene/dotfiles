//! 1 回の検査: 検出を呼び、照合して、記録を残す。CLI の `check` と `bench` が使う。

use crate::backend::{Backend, Request};
use crate::detect::{self, Detection, Status, Verdict};
use crate::store::{Record, Store, StoredThread};
use crate::text;
use std::path::Path;
use std::time::Duration;

/// 既定のモデル。`bench` の測定で決める(docs/agents/reply-coverage.md)。環境変数 `REPLY_COVERAGE_MODEL` で上書きできる。
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// 1 回の依頼の持ち時間。
pub const TIMEOUT: Duration = Duration::from_secs(180);

pub fn model_from_env() -> String {
    process_env::var_nonempty("REPLY_COVERAGE_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// 検出を呼び、照合して判定する(記録は書かない)。
pub fn detect(
    backend: &dyn Backend,
    cwd: &Path,
    model: &str,
    thread_text: &str,
    draft: &str,
    me_hint: Option<&str>,
) -> Result<Verdict, String> {
    let user = detect::user_prompt(thread_text, draft, me_hint);
    let req = Request {
        model,
        system: detect::SYSTEM,
        user: &user,
        schema: detect::SCHEMA,
        cwd,
        timeout: TIMEOUT,
    };
    let json = backend.complete(&req).map_err(|e| e.to_string())?;
    let det: Detection =
        serde_json::from_str(&json).map_err(|e| format!("検出の JSON を読めません: {e}"))?;
    Ok(detect::judge(&det, thread_text, draft))
}

/// 検査して、記録を追記する。
pub fn run_check(
    store: &Store,
    backend: &dyn Backend,
    thread: &StoredThread,
    draft: &str,
    model: &str,
    me_hint: Option<&str>,
    now: u64,
) -> Result<(Verdict, Record), String> {
    let cwd = store
        .cwd()
        .map_err(|e| format!("作業ディレクトリを作れません: {e}"))?;
    let verdict = detect(backend, &cwd, model, &thread.text, draft, me_hint)?;
    let record = Record {
        ts_unix: now,
        thread: thread.id.clone(),
        thread_sha: text::sha(&thread.text),
        draft_sha: text::sha(draft),
        verdict: if verdict.pass { "pass" } else { "fail" }.to_string(),
        model: model.to_string(),
        asks: verdict.asks.len(),
        unanswered: verdict
            .unanswered()
            .iter()
            .map(|a| a.quote.clone())
            .collect(),
        apology_missing: verdict.apology_missing(),
    };
    store
        .append_record(&record)
        .map_err(|e| format!("記録を書けません: {e}"))?;
    Ok((verdict, record))
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Answered => "答える",
        Status::Declined => "断る",
        Status::Deferred => "保留",
        Status::AnsweredBefore => "先に答え済み",
        Status::Retracted => "取り下げ済み",
        Status::Unanswered => "★未回答",
    }
}

/// 人とエージェントが読む報告。依頼の一覧と処分を出す。
pub fn report(thread_id: &str, v: &Verdict) -> String {
    let mut s = format!("会話 {thread_id}: 相手の依頼 {} 件\n", v.asks.len());
    for a in &v.asks {
        let note = if a.downgraded {
            "(根拠が照合に外れたため未回答に戻した)"
        } else {
            ""
        };
        s.push_str(&format!(
            "- [{}] {}{note}\n    「{}」\n",
            status_label(a.status),
            a.kind,
            a.quote
        ));
    }
    if v.dropped > 0 {
        s.push_str(&format!(
            "(引用が会話の全文に無く、捨てた依頼 {} 件)\n",
            v.dropped
        ));
    }
    if v.apology_missing() {
        s.push_str(
            "★前の返信が落としていた依頼に、今回初めて答えています。冒頭で、ずれた・遅れたことに軽く触れて詫びてください。\n",
        );
    }
    s.push_str(if v.pass {
        "判定: 合格\n"
    } else {
        "判定: 不合格(未回答の依頼に答える、または断る・保留する一文を足して、もう一度 check してください)\n"
    });
    s
}
