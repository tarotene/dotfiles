//! 書き直しの本体と、「どの段落が未通過か」の判定。
//!
//! 判定([`pending`])は LLM を呼ばず、記録だけで決まる。投稿の関所と文書ファイルの関所は
//! これだけを呼ぶ。書き直し([`polish`])は同じ判定で対象を決め、モデルに依頼し、戻ってきた
//! 段落を照合してから差し替える。

use crate::blocks::{self, Block, MIN_JP_CHARS};
use crate::config::Config;
use crate::llm::{self, Backend, FailureKind, Item, LlmError, Request};
use crate::mask::{self, Masked};
use crate::records::Entry;
use std::collections::hash_map::RandomState;
use std::collections::HashSet;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// 前後の文脈として見せる長さの上限(字)。
const CONTEXT_CHARS: usize = 400;

/// 未通過の日本語の段落。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// 段落の番号(0 始まり、[`blocks::split`] の並び)。
    pub index: usize,
    /// 段落の本文。
    pub text: String,
}

/// 書き直しの対象になる段落の番号。次のすべてを満たす:
/// - フェンスや front matter でない
/// - 伏せ字を除いて、日本語の字が [`MIN_JP_CHARS`] 以上ある
/// - 記録に無い(通していない)
/// - `baseline` があれば、基点の文書に同じ段落が無い(追加・変更された段落である)
fn pending_indexes(
    blocks: &[Block],
    baseline: Option<&str>,
    records: &HashSet<String>,
) -> Vec<usize> {
    let base: Option<HashSet<String>> = baseline.map(|b| {
        blocks::split(b)
            .iter()
            .map(|x| blocks::hash(&x.text))
            .collect()
    });
    blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.verbatim)
        .filter(|(_, b)| blocks::jp_count(&mask::mask(&b.text).text) >= MIN_JP_CHARS)
        .filter(|(_, b)| {
            let h = blocks::hash(&b.text);
            !records.contains(&h) && !base.as_ref().is_some_and(|s| s.contains(&h))
        })
        .map(|(i, _)| i)
        .collect()
}

/// 未通過の日本語の段落の一覧。空なら、この文書は通っている。
pub fn pending(text: &str, baseline: Option<&str>, records: &HashSet<String>) -> Vec<Pending> {
    let blocks = blocks::split(text);
    pending_indexes(&blocks, baseline, records)
        .into_iter()
        .map(|index| Pending {
            index,
            text: blocks[index].text.clone(),
        })
        .collect()
}

/// 段落ごとの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// 書き直した(本文が変わった)。
    Polished,
    /// 見て、直す必要が無かった。
    Unchanged,
    /// 通せなかったので、元の本文のまま残した。理由つき。
    Skipped(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Para {
    pub before: String,
    pub after: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolishResult {
    /// 書き直した後の文書全体。
    pub text: String,
    pub paras: Vec<Para>,
    /// 記録に足す行。
    pub entries: Vec<Entry>,
}

impl PolishResult {
    pub fn skipped(&self) -> Vec<&str> {
        self.paras
            .iter()
            .filter_map(|p| match &p.status {
                Status::Skipped(r) => Some(r.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn count(&self, f: impl Fn(&Status) -> bool) -> usize {
        self.paras.iter().filter(|p| f(&p.status)).count()
    }
}

/// 隣の段落を、文体を合わせるための参考として見せる文字列にする。
fn context(blocks: &[Block], index: usize, forward: bool) -> String {
    let j = if forward {
        index.checked_add(1)
    } else {
        index.checked_sub(1)
    };
    let Some(b) = j.and_then(|j| blocks.get(j)) else {
        return String::new();
    };
    if b.verbatim {
        return "(コードブロック)".to_string();
    }
    mask::redact(&b.text).chars().take(CONTEXT_CHARS).collect()
}

/// 戻ってきた段落(伏せ字つき)を照合する。通れば前後の改行を落とした本文、外れれば理由。
fn verify(input: &str, output: &str) -> Result<String, String> {
    let out = output.trim_matches(['\n', '\r']);
    if out.trim().is_empty() {
        return Err("応答の段落が空です".to_string());
    }
    if out.contains("<para") || out.contains("</para") || out.contains("<text>") {
        return Err("応答に依頼の書式が混ざっています".to_string());
    }
    let (mut a, mut b) = (mask::token_ids(input), mask::token_ids(out));
    a.sort_unstable();
    b.sort_unstable();
    if a != b {
        return Err("伏せ字(コード・URL・参照)が欠けたか増えました".to_string());
    }
    if mask::numbers(input) != mask::numbers(out) {
        return Err("数値の集合が変わりました".to_string());
    }
    if mask::structure(input) != mask::structure(out) {
        return Err("行頭の記法(見出し・箇条書き・引用・表)が変わりました".to_string());
    }
    let (n_in, n_out) = (blocks::jp_count(input), blocks::jp_count(out));
    // 翻訳調を削ると半分近くまで縮むことがある。4 割を切るか 2 倍を超えたら、省いたか足したとみなす。
    if n_out * 10 < n_in * 4 || n_out > n_in * 2 {
        return Err("分量が大きく変わりました".to_string());
    }
    Ok(out.to_string())
}

/// 通った段落: 照合に通った本文(伏せ字つき)と、それを答えたモデル(記録の `model` になる)。
#[derive(Debug, Clone)]
struct Verified {
    text: String,
    model: String,
}

/// 通せなかった段落の理由。`transient` は、混雑・時間切れのような一時的な原因か
/// (記録の [`Entry::transient`] になり、課金の検討を知らせる判定の入力になる)。
#[derive(Debug, Clone)]
struct Skip {
    reason: String,
    transient: bool,
}

impl Skip {
    /// 照合の外れ・応答の欠けなど、待っても直らない理由。
    fn permanent(reason: impl Into<String>) -> Self {
        Skip {
            reason: reason.into(),
            transient: false,
        }
    }
}

impl From<LlmError> for Skip {
    fn from(e: LlmError) -> Self {
        Skip {
            transient: e.is_transient(),
            reason: e.reason,
        }
    }
}

/// 再依頼の前の待ち。`base` を `attempt` 回目(1 始まり)ごとに 3 倍し、±25% のゆらぎを付ける
/// (Google, "Troubleshooting" の、指数バックオフにジッタを足す推奨。同じ瞬間に再依頼が
/// そろわないようにする)。
fn backoff(base: Duration, attempt: u32) -> Duration {
    let grown = base.saturating_mul(3u32.saturating_pow(attempt.saturating_sub(1)));
    let r = RandomState::new().build_hasher().finish() % 501;
    grown.mul_f64(0.75 + r as f64 / 1000.0)
}

/// 1 つの束の段落ごとの結果(束の中の並びのまま)。
type ChunkOutput = Vec<Result<Verified, Skip>>;

/// 1 つの束(複数の段落を 1 回の依頼にしたもの)を送り、答えたモデルと本文を返す。
///
/// 1. 主のモデルに送る。混雑(`Busy`)なら、間を置いて `cfg.retries` 回まで再依頼する。
/// 2. 時間切れ(`Timeout`)は同じモデルへ再依頼しない。待ち直しても応答が返らないモデル
///    (無料枠の混雑)に、`timeout` を何度も使い切らないため。
/// 3. 主のモデルで通らず、予備のモデル(`cfg.fallback_model`)があれば、1 回だけ送る。
///    予備には思考量の指定を渡さない。
/// 4. 恒久的な失敗(`Permanent`: キーが無い、など)は、再依頼も予備もなしでそこで止める。
fn call(
    backend: &dyn Backend,
    cfg: &Config,
    system: &str,
    user: &str,
) -> Result<(String, String), Skip> {
    let primary = Request {
        model: &cfg.model,
        thinking_level: cfg.thinking_level.as_deref(),
        system,
        user,
        timeout: cfg.timeout,
    };
    let mut last = LlmError::permanent("依頼していません");
    for attempt in 0..=cfg.retries {
        if attempt > 0 {
            std::thread::sleep(backoff(cfg.retry_base, attempt));
        }
        match backend.complete(&primary) {
            Ok(resp) => return Ok((resp, cfg.model.clone())),
            Err(e) if e.kind == FailureKind::Permanent => return Err(Skip::from(e)),
            Err(e) => {
                let timed_out = e.kind == FailureKind::Timeout;
                last = e;
                if timed_out {
                    break;
                }
            }
        }
    }
    if let Some(fb) = cfg.fallback_model.as_deref().filter(|f| *f != cfg.model) {
        let fallback = Request {
            model: fb,
            thinking_level: None,
            ..primary
        };
        match backend.complete(&fallback) {
            Ok(resp) => return Ok((resp, fb.to_string())),
            Err(e) => {
                last = LlmError::new(
                    last.kind,
                    format!("{}(予備の {fb} も失敗: {})", last.reason, e.reason),
                );
            }
        }
    }
    Err(Skip::from(last))
}

/// 1 つの束の段落を書き直す。`ks` は束に入る段落の番号(`items`・`masked` の添字)で、戻り値は `ks` と
/// 同じ並び。照合で外れた段落だけ、もう 1 回依頼する。依頼そのものが失敗したら、束の段落すべてを素通しにする。
fn process_chunk(
    ks: &[usize],
    items: &[Item],
    masked: &[Masked],
    system: &str,
    cfg: &Config,
    backend: &dyn Backend,
) -> Vec<Result<Verified, Skip>> {
    let mut outputs: Vec<Result<Verified, Skip>> = ks
        .iter()
        .map(|_| Err(Skip::permanent("依頼していません")))
        .collect();
    let mut todo: Vec<usize> = (0..ks.len()).collect();
    for _attempt in 0..2 {
        if todo.is_empty() {
            break;
        }
        let sub: Vec<Item> = todo.iter().map(|&p| items[ks[p]].clone()).collect();
        match call(backend, cfg, system, &llm::user_message(&sub)) {
            Err(skip) => {
                for &p in &todo {
                    outputs[p] = Err(skip.clone());
                }
                break;
            }
            Ok((resp, model)) => {
                let parsed = llm::parse_response(&resp);
                let mut next = Vec::new();
                for (pos, &p) in todo.iter().enumerate() {
                    let verdict = match parsed.get(&(pos + 1)) {
                        None => Err("応答に該当する段落がありません".to_string()),
                        Some(out) => verify(&masked[ks[p]].text, out),
                    };
                    if verdict.is_err() {
                        next.push(p);
                    }
                    outputs[p] = verdict
                        .map(|text| Verified {
                            text,
                            model: model.clone(),
                        })
                        .map_err(Skip::permanent);
                }
                todo = next;
            }
        }
    }
    outputs
}

/// 文書の未通過の日本語の段落を書き直す。
///
/// - `baseline`: 基点の文書。あれば、そこに無い(追加・変更された)段落だけが対象。
/// - `records`: 通した段落の `sha` の集合。
/// - 対象の段落は `cfg.chunk_paras` 個ずつの束に分け、最大 `cfg.parallel` 本を同時に送る
///   (1 回の依頼が大きいほど応答が遅くなり、時間切れにもなりやすいため)。束の中の再依頼と予備の
///   モデルは [`call`]。照合で外れた段落だけ、もう 1 回依頼する。通せなかった段落は元の本文のまま
///   素通しにして理由を残す。
pub fn polish(
    text: &str,
    baseline: Option<&str>,
    records: &HashSet<String>,
    style: &str,
    cfg: &Config,
    backend: &dyn Backend,
) -> PolishResult {
    let mut blocks = blocks::split(text);
    let idxs = pending_indexes(&blocks, baseline, records);
    if idxs.is_empty() {
        return PolishResult {
            text: text.to_string(),
            paras: Vec::new(),
            entries: Vec::new(),
        };
    }

    let masked: Vec<Masked> = idxs.iter().map(|&i| mask::mask(&blocks[i].text)).collect();
    let items: Vec<Item> = idxs
        .iter()
        .zip(&masked)
        .map(|(&i, m)| Item {
            before: context(&blocks, i, false),
            text: m.text.clone(),
            after: context(&blocks, i, true),
        })
        .collect();
    let system = llm::system_prompt(style);

    let all: Vec<usize> = (0..idxs.len()).collect();
    let chunks: Vec<&[usize]> = all.chunks(cfg.chunk_paras.max(1)).collect();
    // 束ごとの結果の置き場(束の番号で置くので、どの順に終わっても段落の並びは変わらない)。
    let slots: Mutex<Vec<Option<ChunkOutput>>> = Mutex::new(chunks.iter().map(|_| None).collect());
    let next = AtomicUsize::new(0);
    let workers = cfg.parallel.clamp(1, chunks.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let c = next.fetch_add(1, Ordering::SeqCst);
                let Some(ks) = chunks.get(c) else { break };
                let out = process_chunk(ks, &items, &masked, &system, cfg, backend);
                slots.lock().expect("slots lock")[c] = Some(out);
            });
        }
    });
    let outputs: Vec<Result<Verified, Skip>> = slots
        .into_inner()
        .expect("slots lock")
        .into_iter()
        .flat_map(|o| o.expect("every chunk is processed"))
        .collect();

    let mut paras = Vec::new();
    let mut entries = Vec::new();
    for (k, &i) in idxs.iter().enumerate() {
        let original = blocks[i].text.clone();
        match &outputs[k] {
            Ok(v) => {
                let restored = mask::unmask(&v.text, &masked[k].slots);
                let changed = blocks::normalize(&restored) != blocks::normalize(&original);
                if changed {
                    blocks[i].text = restored.clone();
                }
                entries.push(Entry::polished(blocks::hash(&restored), &v.model));
                paras.push(Para {
                    before: original,
                    after: restored,
                    status: if changed {
                        Status::Polished
                    } else {
                        Status::Unchanged
                    },
                });
            }
            Err(skip) => {
                entries.push(
                    Entry::skipped(blocks::hash(&original), &cfg.model, &skip.reason)
                        .with_transient(skip.transient),
                );
                paras.push(Para {
                    before: original.clone(),
                    after: original,
                    status: Status::Skipped(skip.reason.clone()),
                });
            }
        }
    }

    PolishResult {
        text: blocks::join(&blocks),
        paras,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    /// 送られた依頼の記録(順序は並列だと決まらないので、中身で確かめる)。
    #[derive(Debug, Clone)]
    struct Call {
        model: String,
        thinking: Option<String>,
        user: String,
    }

    type Responder = Box<dyn Fn(&Request) -> Result<String, LlmError> + Send + Sync>;

    /// 依頼を記録し、決めた応答を返す。応答は、順に返す列(`new`)か、依頼から作る関数(`responding`)。
    struct Fake {
        replies: Mutex<Vec<Result<String, LlmError>>>,
        responder: Option<Responder>,
        calls: Mutex<Vec<Call>>,
    }

    impl Fake {
        fn new(replies: Vec<Result<String, LlmError>>) -> Self {
            Fake {
                replies: Mutex::new(replies),
                responder: None,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn responding(
            f: impl Fn(&Request) -> Result<String, LlmError> + Send + Sync + 'static,
        ) -> Self {
            Fake {
                replies: Mutex::new(Vec::new()),
                responder: Some(Box::new(f)),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Backend for Fake {
        fn complete(&self, req: &Request) -> Result<String, LlmError> {
            self.calls.lock().unwrap().push(Call {
                model: req.model.to_string(),
                thinking: req.thinking_level.map(str::to_string),
                user: req.user.to_string(),
            });
            if let Some(f) = &self.responder {
                return f(req);
            }
            let mut r = self.replies.lock().unwrap();
            if r.is_empty() {
                Err(LlmError::permanent("応答が尽きました"))
            } else {
                r.remove(0)
            }
        }
    }

    fn ok(s: &str) -> Result<String, LlmError> {
        Ok(s.to_string())
    }

    fn busy(s: &str) -> Result<String, LlmError> {
        Err(LlmError::new(FailureKind::Busy, s))
    }

    fn timeout() -> Result<String, LlmError> {
        Err(LlmError::new(
            FailureKind::Timeout,
            "45 秒以内に応答がありません",
        ))
    }

    /// 待ちを 0 にし、予備のモデルは使わない設定(個々のテストが必要な項目だけ足す)。
    fn cfg() -> Config {
        Config {
            model: "m".to_string(),
            fallback_model: None,
            retry_base: Duration::ZERO,
            ..Config::default()
        }
    }

    fn run_with(text: &str, baseline: Option<&str>, cfg: &Config, fake: &Fake) -> PolishResult {
        polish(text, baseline, &HashSet::new(), "# 規約", cfg, fake)
    }

    fn run(text: &str, baseline: Option<&str>, fake: &Fake) -> PolishResult {
        run_with(text, baseline, &cfg(), fake)
    }

    #[test]
    fn pending_skips_code_english_short_recorded_and_baseline() {
        let text = "日本語の段落です。\n\n```\n日本語のコード内の文字列です\n```\n\nEnglish only paragraph.\n\n検証\n\n基点にもある段落です。\n";
        let p = pending(text, None, &HashSet::new());
        assert_eq!(
            p.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            vec!["日本語の段落です。", "基点にもある段落です。"]
        );

        let base = "基点にもある段落です。\n";
        let p = pending(text, Some(base), &HashSet::new());
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text, "日本語の段落です。");

        let mut rec = HashSet::new();
        rec.insert(blocks::hash("日本語の段落です。"));
        assert!(pending("日本語の段落です。\n", None, &rec).is_empty());
    }

    #[test]
    fn heading_only_and_trailer_only_paragraphs_are_not_pending() {
        let text = "## 解決策を書く節\n\nCloses #12\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\n";
        assert!(pending(text, None, &HashSet::new()).is_empty());
    }

    #[test]
    fn polish_replaces_only_the_target_and_keeps_separators() {
        let text =
            "## 課題\n\n翻訳調の文章を直すことができる。`code` を残す。\n\n\nEnglish only.\n";
        // 伏せ字の番号は mask の実装が振る(この段落ではコードが 0 番)。
        let m = mask::mask("翻訳調の文章を直すことができる。`code` を残す。");
        assert_eq!(m.slots, vec!["`code`"]);
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">\n文章を直す。⟦P0⟧ を残す。\n</para>",
        )]);
        let r = run(text, None, &fake);
        assert_eq!(
            r.text,
            "## 課題\n\n文章を直す。`code` を残す。\n\n\nEnglish only.\n"
        );
        assert_eq!(r.paras.len(), 1);
        assert_eq!(r.paras[0].status, Status::Polished);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].state, "polished");
        assert_eq!(r.entries[0].model, "m");
        assert_eq!(
            r.entries[0].sha,
            blocks::hash("文章を直す。`code` を残す。")
        );
        // 見出しは依頼に載らない。
        let sent = fake.calls()[0].user.clone();
        assert!(
            !sent.contains("課題\n</text>"),
            "見出しは対象にしない: {sent}"
        );
        assert!(sent.contains("<before>"), "前の段落は参考として渡す");
    }

    #[test]
    fn unchanged_when_model_returns_same_text() {
        let text = "すでに自然な日本語の文です。\n";
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">すでに自然な日本語の文です。</para>",
        )]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.paras[0].status, Status::Unchanged);
        assert_eq!(r.entries[0].state, "polished");
    }

    #[test]
    fn permanent_failure_skips_every_target_without_retry_or_fallback() {
        let text = "一つ目の段落です。\n\n二つ目の段落です。\n";
        let fake = Fake::new(vec![Err(LlmError::permanent("API キーが未設定"))]);
        let c = Config {
            fallback_model: Some("fb".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.skipped(), vec!["API キーが未設定", "API キーが未設定"]);
        assert!(r
            .entries
            .iter()
            .all(|e| e.state == "skipped" && !e.transient));
        assert_eq!(
            fake.calls().len(),
            1,
            "恒久的な失敗では再依頼も予備もしない"
        );
    }

    #[test]
    fn busy_is_retried_on_the_same_model_until_it_answers() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![
            busy("high demand"),
            busy("high demand"),
            ok("<para n=\"1\">一つ目の段落だ。</para>"),
        ]);
        let c = Config {
            thinking_level: Some("low".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, "一つ目の段落だ。\n");
        let calls = fake.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls
            .iter()
            .all(|c| c.model == "m" && c.thinking.as_deref() == Some("low")));
        assert_eq!(r.entries[0].model, "m");
        assert!(!r.entries[0].transient);
    }

    #[test]
    fn exhausted_retries_fall_back_once_without_the_thinking_level() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![
            busy("high demand"),
            busy("high demand"),
            busy("high demand"),
            ok("<para n=\"1\">一つ目の段落だ。</para>"),
        ]);
        let c = Config {
            fallback_model: Some("fb".to_string()),
            thinking_level: Some("low".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, "一つ目の段落だ。\n");
        let calls = fake.calls();
        assert_eq!(calls.len(), 4, "主を 1 + 2 回、予備を 1 回");
        assert_eq!(
            calls.iter().map(|c| c.model.as_str()).collect::<Vec<_>>(),
            vec!["m", "m", "m", "fb"]
        );
        assert_eq!(calls[3].thinking, None, "予備には思考量を渡さない");
        assert_eq!(r.entries[0].model, "fb", "記録には答えたモデルを書く");
    }

    #[test]
    fn timeout_goes_straight_to_the_fallback_without_waiting_again() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![timeout(), ok("<para n=\"1\">一つ目の段落だ。</para>")]);
        let c = Config {
            fallback_model: Some("fb".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, "一つ目の段落だ。\n");
        let models: Vec<String> = fake.calls().into_iter().map(|c| c.model).collect();
        assert_eq!(
            models,
            vec!["m", "fb"],
            "時間切れは同じモデルを待ち直さない"
        );
    }

    #[test]
    fn everything_failing_is_skipped_and_marked_transient() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![
            busy("high demand"),
            busy("high demand"),
            busy("high demand"),
            busy("503"),
        ]);
        let c = Config {
            fallback_model: Some("fb".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, text);
        let reason = r.skipped()[0].to_string();
        assert!(
            reason.contains("high demand") && reason.contains("予備の fb も失敗"),
            "{reason}"
        );
        assert_eq!(r.entries[0].state, "skipped");
        assert_eq!(r.entries[0].model, "m");
        assert!(r.entries[0].transient, "混雑が原因なので、一時的の印が付く");
    }

    #[test]
    fn fallback_equal_to_the_primary_is_not_tried_again() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![timeout()]);
        let c = Config {
            fallback_model: Some("m".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(fake.calls().len(), 1);
        assert!(r.entries[0].transient);
    }

    #[test]
    fn failed_verification_retries_only_that_paragraph_once() {
        let text = "一つ目は 3 件の段落です。\n\n二つ目の段落です。\n";
        let fake = Fake::new(vec![
            // 1 回目: 一つ目は数値を変えた(照合で外れる)、二つ目は良い。
            ok("<para n=\"1\">一つ目は 4 件の段落です。</para>\n<para n=\"2\">二つ目の段落だ。</para>"),
            // 2 回目: 一つ目だけ再依頼されて、直った。
            ok("<para n=\"1\">一つ目は 3 件の段落だ。</para>"),
        ]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, "一つ目は 3 件の段落だ。\n\n二つ目の段落だ。\n");
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].user.contains("3 件"));
        assert!(
            !calls[1].user.contains("<para n=\"2\">"),
            "再依頼は 1 段落だけ"
        );
    }

    #[test]
    fn still_failing_after_retry_is_skipped_with_the_reason() {
        let text = "一つ目は 3 件の段落です。\n";
        let bad = "<para n=\"1\">一つ目は 4 件の段落です。</para>";
        let fake = Fake::new(vec![ok(bad), ok(bad)]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.skipped(), vec!["数値の集合が変わりました"]);
        assert!(!r.entries[0].transient, "照合の外れは一時的ではない");
    }

    /// 依頼の `<para n="K">…<text>本文</text>…` から、各段落の本文を取り出す。
    fn texts_in(user: &str) -> Vec<(usize, String)> {
        let re = Regex::new(r#"(?s)<para n="(\d+)">.*?<text>\n(.*?)\n</text>"#).unwrap();
        re.captures_iter(user)
            .map(|c| (c[1].parse().unwrap(), c[2].to_string()))
            .collect()
    }

    /// 各段落の「です」を「だ」に直して返す偽のモデル。
    fn dessu_to_da(req: &Request) -> Result<String, LlmError> {
        Ok(texts_in(req.user)
            .into_iter()
            .map(|(n, t)| format!("<para n=\"{n}\">{}</para>", t.replace("です", "だ")))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    const FIVE: &str = "一番目の段落です。\n\n二番目の段落です。\n\n三番目の段落です。\n\n四番目の段落です。\n\n五番目の段落です。\n";

    #[test]
    fn chunks_are_sent_in_parallel_and_the_paragraph_order_is_kept() {
        let fake = Fake::responding(dessu_to_da);
        let c = Config {
            chunk_paras: 2,
            parallel: 3,
            ..cfg()
        };
        let r = run_with(FIVE, None, &c, &fake);
        assert_eq!(
            r.text,
            "一番目の段落だ。\n\n二番目の段落だ。\n\n三番目の段落だ。\n\n四番目の段落だ。\n\n五番目の段落だ。\n"
        );
        let calls = fake.calls();
        assert_eq!(calls.len(), 3, "5 段落を 2 つずつに分けると 3 束");
        assert!(calls
            .iter()
            .all(|c| c.user.matches("<para n=").count() <= 2));
        let hashes: Vec<String> = r.entries.iter().map(|e| e.sha.clone()).collect();
        let expected: Vec<String> = [
            "一番目の段落だ。",
            "二番目の段落だ。",
            "三番目の段落だ。",
            "四番目の段落だ。",
            "五番目の段落だ。",
        ]
        .iter()
        .map(|t| blocks::hash(t))
        .collect();
        assert_eq!(hashes, expected, "記録も段落の順");
    }

    #[test]
    fn one_failed_chunk_does_not_spoil_the_others() {
        // 「三番目」が書き直しの対象(<text>)に入っている束だけが失敗する(恒久的な失敗)。
        // 前後の参考(<before>・<after>)に出るだけの束は、失敗にしない。
        let fake = Fake::responding(|req| {
            if texts_in(req.user).iter().any(|(_, t)| t.contains("三番目")) {
                return Err(LlmError::permanent("この束だけ失敗"));
            }
            dessu_to_da(req)
        });
        let c = Config {
            chunk_paras: 2,
            parallel: 2,
            ..cfg()
        };
        let r = run_with(FIVE, None, &c, &fake);
        let statuses: Vec<bool> = r
            .paras
            .iter()
            .map(|p| matches!(p.status, Status::Skipped(_)))
            .collect();
        // 束は [一, 二] [三, 四] [五]。三・四が素通し、残りは書き直す。
        assert_eq!(statuses, vec![false, false, true, true, false]);
        assert!(r
            .text
            .starts_with("一番目の段落だ。\n\n二番目の段落だ。\n\n三番目の段落です。"));
        assert!(r.text.ends_with("五番目の段落だ。\n"));
    }

    #[test]
    fn backoff_grows_by_three_with_a_quarter_of_jitter() {
        for _ in 0..50 {
            let d1 = backoff(Duration::from_secs(2), 1);
            assert!(d1 >= Duration::from_millis(1500) && d1 <= Duration::from_millis(2500));
            let d2 = backoff(Duration::from_secs(2), 2);
            assert!(d2 >= Duration::from_millis(4500) && d2 <= Duration::from_millis(7500));
        }
        assert_eq!(backoff(Duration::ZERO, 3), Duration::ZERO);
    }

    #[test]
    fn verify_rejects_lost_tokens_structure_and_size() {
        let input = "- 項目を ⟦P0⟧ に直す 2 件";
        assert!(verify(input, "- 項目を ⟦P0⟧ に直す 2 件").is_ok());
        assert!(verify(input, "- 項目を直す 2 件")
            .unwrap_err()
            .contains("伏せ字"));
        assert!(verify(input, "項目を ⟦P0⟧ に直す 2 件")
            .unwrap_err()
            .contains("行頭"));
        assert!(verify(input, "- 項目を ⟦P0⟧ に直す 3 件")
            .unwrap_err()
            .contains("数値"));
        assert!(verify(input, "- 直 ⟦P0⟧ 2").unwrap_err().contains("分量"));
        assert!(verify(input, "  ").unwrap_err().contains("空"));
        assert!(verify(input, "<para n=\"1\">x")
            .unwrap_err()
            .contains("書式"));
    }

    #[test]
    fn nothing_pending_means_no_request() {
        let fake = Fake::new(vec![]);
        let r = run("English only.\n", None, &fake);
        assert!(fake.calls().is_empty());
        assert!(r.paras.is_empty() && r.entries.is_empty());
    }
}
