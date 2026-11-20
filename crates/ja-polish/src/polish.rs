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
use crate::textlint::Fixer;
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
    /// 段落の番号(0 始まり)。基点があるときは [`blocks::split_with_items`]、なければ [`blocks::split`] の並び。
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
    // 文書の先頭の front matter が `ja-polish: skip` を宣言していれば、この文書は書き直しも関所も飛ばす。
    // 書き直してはいけない文書(第三者の問題文の逐語の写し、本人のメモ、生成物、追記専用の記録)の逃げ道を、
    // セッション全体の環境変数でなく、文書ごとの宣言に置く(F19・F38・F58)。ここは書き直し(`polish`)・
    // Stop の関所・`--check`・投稿の関所が共通で通る入口なので、宣言はすべての経路に効く。
    if skip_declared(blocks) {
        return Vec::new();
    }
    // 基点は、文書と同じ単位(箇条書き・番号付きは項目ごと)で分ける。
    let base: Option<HashSet<String>> = baseline.map(|b| {
        blocks::split_with_items(b)
            .iter()
            .map(|x| blocks::hash(&x.text))
            .collect()
    });
    blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.verbatim)
        .filter(|(_, b)| !blocks::is_table(&b.text))
        .filter(|(_, b)| is_japanese(&b.text))
        .filter(|(_, b)| {
            !is_recorded(b, records)
                && !base
                    .as_ref()
                    .is_some_and(|s| s.contains(&blocks::hash(&b.text)))
        })
        .map(|(i, _)| i)
        .collect()
}

/// 文書の先頭の front matter に、`ja-polish: skip` の宣言があるか。値の大文字・小文字と、引用符は問わない。
/// front matter の外(本文)に同じ行があっても、宣言とは読まない。
fn skip_declared(blocks: &[Block]) -> bool {
    let Some(first) = blocks.first() else {
        return false;
    };
    if !first.verbatim || first.text.trim_start().lines().next().map(str::trim_end) != Some("---") {
        return false;
    }
    first
        .text
        .trim_start()
        .lines()
        .skip(1)
        .take_while(|l| l.trim_end() != "---")
        .any(|l| {
            l.trim().strip_prefix("ja-polish:").is_some_and(|v| {
                v.trim()
                    .trim_matches(['"', '\''])
                    .eq_ignore_ascii_case("skip")
            })
        })
}

/// 伏せ字を除いて、日本語の字が [`MIN_JP_CHARS`] 以上ある段落か。
fn is_japanese(text: &str) -> bool {
    blocks::jp_count(&mask::mask(text).text) >= MIN_JP_CHARS
}

/// 記録にある段落か。箇条書き・番号付きの段落(基点なしで、項目に分けていないもの)は、段落全体の記録のほか、
/// 日本語の項目がすべて記録にあれば、記録にあるとみなす(基点つきで項目ごとに書き直した文書を、基点なしで
/// 見る関所が、同じ箇条書きをもう一度求めないため)。
fn is_recorded(b: &Block, records: &HashSet<String>) -> bool {
    if records.contains(&blocks::hash(&b.text)) {
        return true;
    }
    let items = blocks::split_items(vec![b.clone()]);
    items.len() > 1
        && items
            .iter()
            .all(|i| !is_japanese(&i.text) || records.contains(&blocks::hash(&i.text)))
}

/// 文書を段落に分ける。基点があるときは、箇条書き・番号付きを項目ごとの段落にする
/// (項目を足しただけで、承認済みの項目まで書き直しの対象になるのを避ける)。
fn split_for(text: &str, baseline: Option<&str>) -> Vec<Block> {
    if baseline.is_some() {
        blocks::split_with_items(text)
    } else {
        blocks::split(text)
    }
}

/// 未通過の日本語の段落の一覧。空なら、この文書は通っている。
pub fn pending(text: &str, baseline: Option<&str>, records: &HashSet<String>) -> Vec<Pending> {
    let blocks = split_for(text, baseline);
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
    // 文脈の欄(<before>・<after>)だけを写した応答や、囲みの外れた `</text>` が単独で残った応答も、依頼の書式の混入
    // (`</text>` が文書に残る事故があった。F22)。段落の本文が、これらの語を元から持つときは除く。
    if ["<before>", "</before>", "<after>", "</after>", "</text>"]
        .iter()
        .any(|t| out.contains(t) && !input.contains(t))
    {
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
    // 行頭にあった伏せ字(`No-Visual:` などのタグの接頭辞)が、文中へ動くと、PR 本文の骨組みが壊れる(F36)。
    // 個数は同じなので、伏せ字の照合では見えない。行頭だった伏せ字は、出力でも行頭に残す。
    let heads_out = mask::line_head_ids(out);
    if mask::line_head_ids(input)
        .iter()
        .any(|id| !heads_out.contains(id))
    {
        return Err("行頭にあった伏せ字(タグの接頭辞など)が行頭から動きました".to_string());
    }
    // 括弧の幅。リポジトリの文書の括弧の流儀(半角か全角か)を、書き直しで変えない(F7)。
    if paren_widths(input) != paren_widths(out) {
        return Err("括弧の幅(半角・全角)が変わりました".to_string());
    }
    // 元に無い、日本語に出ない簡体字(F40)と、記号の実体参照(`&gt;` など。F68)の混入。
    if let Some(c) = new_simplified_char(input, out) {
        return Err(format!("入力に無い簡体字「{c}」が混ざりました"));
    }
    if entity_refs(out) > entity_refs(input) {
        return Err("記号の実体参照(&gt; など)が増えました".to_string());
    }
    let (n_in, n_out) = (blocks::jp_count(input), blocks::jp_count(out));
    // 翻訳調を削ると半分近くまで縮むことがある。4 割を切るか 2 倍を超えたら、省いたか足したとみなす。
    if n_out * 10 < n_in * 4 || n_out > n_in * 2 {
        return Err("分量が大きく変わりました".to_string());
    }
    Ok(out.to_string())
}

/// 半角と全角の括弧の個数(`(` `)` `（` `）` の順)。伏せ字の中のコードや URL の括弧は数えない
/// (呼び出し側が、伏せ字つきの本文を渡す)。
fn paren_widths(s: &str) -> [usize; 4] {
    let mut n = [0; 4];
    for c in s.chars() {
        match c {
            '(' => n[0] += 1,
            ')' => n[1] += 1,
            '（' => n[2] += 1,
            '）' => n[3] += 1,
            _ => {}
        }
    }
    n
}

/// 日本語に出ない、頻出の簡体字(日本語の字体では別の字になるもの)。書き直しの出力に、入力に無いこれらの字が
/// 現れたら、簡体字の混入とみなす(2026-11-15 に「換」→「换」、2026-11-19 に「頼まず」→「頼せず」の型の誤りが
/// 通った)。字種を網羅する検査ではない(JIS X 0208 の符号化の表を持つ crate は、依存に無い)。
const SIMPLIFIED_ONLY: &str = "个们这说还对时发现么话为认让过书买样开关种车门见观线经给结约级统编项题问顺页类实动总务员单无听读记论设计识讲请谁谢访证评议调译试该语误谈质费资转轻较载输边达运进远连选适遗换";

/// 出力にあって入力に無い、簡体字の最初の 1 字。
fn new_simplified_char(input: &str, out: &str) -> Option<char> {
    out.chars()
        .find(|c| SIMPLIFIED_ONLY.contains(*c) && !input.contains(*c))
}

/// 記号の実体参照(`&gt;` `&lt;` `&amp;` `&quot;`)の個数。
fn entity_refs(s: &str) -> usize {
    ["&gt;", "&lt;", "&amp;", "&quot;"]
        .iter()
        .map(|e| s.matches(e).count())
        .sum()
}

/// 応答の先頭として理由に添える文字数。
const HEAD_CHARS: usize = 200;

/// 素通しの理由に、答えたモデルと(`head` があれば)応答の先頭を添える。
///
/// 予備のモデルに切り替わった場面で、応答の書式が崩れることがある(2026-11-13・11-14 に、全段落が
/// 「応答に依頼の書式が混ざっています」で素通しになったが、原因は未確認)。次の発生で、どのモデルが
/// どんな応答を返したかを、理由から読めるようにする。応答の本文は、コード・URL が伏せ字のままで、改行は `⏎` にする。
fn annotate(reason: String, model: &str, head: Option<&str>) -> String {
    match head {
        Some(h) => {
            let head: String = h
                .chars()
                .take(HEAD_CHARS)
                .map(|c| if c == '\n' || c == '\r' { '⏎' } else { c })
                .collect();
            format!("{reason}(モデル {model}。応答の先頭: 「{head}」)")
        }
        None => format!("{reason}(モデル {model})"),
    }
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
///    (無料枠の混雑)に、`timeout` を何度も使い切らないため。無料枠の 1 日の上限(`DailyLimit`)も同じ
///    で、そのモデルは次の再開まで使えず、間を置いても通らないので、再依頼せず予備のモデルへ移る(F6)。
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
                let no_retry = matches!(e.kind, FailureKind::Timeout | FailureKind::DailyLimit);
                last = e;
                if no_retry {
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

/// 1 つの束の段落を書き直す。`ks` は束に入る段落の番号(`items`・`base` の添字)で、戻り値は `ks` と
/// 同じ並び。`base` は、前段の textlint が直したあとの、モデルに送った本文(伏せ字つき)で、戻りの照合の
/// 基準になる。照合で外れた段落だけ、もう 1 回依頼する。依頼そのものが失敗したら、束の段落すべてを素通しにする。
fn process_chunk(
    ks: &[usize],
    items: &[Item],
    base: &[String],
    system: &str,
    cfg: &Config,
    backend: &dyn Backend,
) -> Vec<Result<Verified, Skip>> {
    let mut outputs: Vec<Result<Verified, Skip>> = ks
        .iter()
        .map(|_| Err(Skip::permanent("依頼していません")))
        .collect();
    let mut todo: Vec<usize> = (0..ks.len()).collect();
    for attempt in 0..2 {
        if todo.is_empty() {
            break;
        }
        // 2 回目は、前後の参考(<before>・<after>)を外して送る。応答に依頼の書式が混ざる型の失敗(F17・F21・F59)は、
        // 参考の欄の書式を応答がなぞることで起きる。参考が無ければ、欄ごと依頼に出ない(`llm::user_message`)。
        let sub: Vec<Item> = todo
            .iter()
            .map(|&p| {
                let mut it = items[ks[p]].clone();
                if attempt > 0 {
                    it.before.clear();
                    it.after.clear();
                }
                it
            })
            .collect();
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
                        None => Err(annotate(
                            "応答に該当する段落がありません".to_string(),
                            &model,
                            Some(&resp),
                        )),
                        Some(out) => verify(&base[ks[p]], out).map_err(|reason| {
                            // 応答の書式が崩れたときだけ、先頭を添える(原因の特定に要る。伏せ字の欠けなどは、
                            // 段落の本文を理由に載せない)。
                            let head = reason.contains("依頼の書式").then_some(out.as_str());
                            annotate(reason, &model, head)
                        }),
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

/// textlint の直し(`fixer` の戻り)のうち、照合([`verify`])を通るものだけを採る。外れた段落、または
/// 戻りの数が合わないときは、入力のまま。Gemini の出力と同じ照合なので、伏せ字・数値・記法・分量を壊す
/// 直しは入らない。
fn accept_fixes(inputs: &[String], fixed: &[String]) -> Vec<String> {
    if fixed.len() != inputs.len() {
        return inputs.to_vec();
    }
    inputs
        .iter()
        .zip(fixed)
        .map(|(inp, out)| verify(inp, out).unwrap_or_else(|_| inp.clone()))
        .collect()
}

/// 文書の未通過の日本語の段落を書き直す。
///
/// - `baseline`: 基点の文書。あれば、そこに無い(追加・変更された)段落だけが対象。
/// - `records`: 通した段落の `sha` の集合。
/// - 順は「前段の textlint → Gemini → 後段の textlint」(D13)。前段の直しは、モデルが止まっても
///   残る(素通しの段落も、前段が直した本文を最終の本文にし、その `sha` を記録する)。
/// - 対象の段落は `cfg.chunk_paras` 個ずつの束に分け、最大 `cfg.parallel` 本を同時に送る
///   (1 回の依頼が大きいほど応答が遅くなり、時間切れにもなりやすいため)。束の中の再依頼と予備の
///   モデルは `call`(非公開)。照合で外れた段落だけ、もう 1 回依頼する。通せなかった段落は、前段の
///   直し以外は元の本文のまま素通しにして理由を残す。
pub fn polish(
    text: &str,
    baseline: Option<&str>,
    records: &HashSet<String>,
    style: &str,
    cfg: &Config,
    backend: &dyn Backend,
    fixer: &dyn Fixer,
) -> PolishResult {
    let mut blocks = split_for(text, baseline);
    let idxs = pending_indexes(&blocks, baseline, records);
    if idxs.is_empty() {
        return PolishResult {
            text: text.to_string(),
            paras: Vec::new(),
            entries: Vec::new(),
        };
    }

    let masked: Vec<Masked> = idxs.iter().map(|&i| mask::mask(&blocks[i].text)).collect();
    // 前段: textlint が定型の冗長表現を直す。モデルへ送る本文と、戻りの照合の基準は、直したあとの本文。
    let originals: Vec<String> = masked.iter().map(|m| m.text.clone()).collect();
    let base: Vec<String> = accept_fixes(&originals, &fixer.fix(&originals));
    let items: Vec<Item> = idxs
        .iter()
        .zip(&base)
        .map(|(&i, b)| Item {
            before: context(&blocks, i, false),
            text: b.clone(),
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
                let out = process_chunk(ks, &items, &base, &system, cfg, backend);
                slots.lock().expect("slots lock")[c] = Some(out);
            });
        }
    });
    let mut outputs: Vec<Result<Verified, Skip>> = slots
        .into_inner()
        .expect("slots lock")
        .into_iter()
        .flat_map(|o| o.expect("every chunk is processed"))
        .collect();

    // 後段: モデルが書き直した段落に、もう一度 textlint の直しを当てる(モデルが持ち込む冗長表現を直す)。
    let answered: Vec<usize> = (0..outputs.len()).filter(|&k| outputs[k].is_ok()).collect();
    let answers: Vec<String> = answered
        .iter()
        .filter_map(|&k| outputs[k].as_ref().ok().map(|v| v.text.clone()))
        .collect();
    for (&k, text) in answered
        .iter()
        .zip(accept_fixes(&answers, &fixer.fix(&answers)))
    {
        if let Ok(v) = &mut outputs[k] {
            v.text = text;
        }
    }

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
                // モデルが通せなかった段落でも、前段の直しは残す(直していなければ、元の本文のまま)。
                let fixed = if base[k] == originals[k] {
                    original.clone()
                } else {
                    mask::unmask(&base[k], &masked[k].slots)
                };
                if blocks::normalize(&fixed) != blocks::normalize(&original) {
                    blocks[i].text = fixed.clone();
                }
                entries.push(
                    Entry::skipped(blocks::hash(&fixed), &cfg.model, &skip.reason)
                        .with_transient(skip.transient),
                );
                paras.push(Para {
                    before: original,
                    after: fixed,
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
    use crate::textlint::NoFixer;
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
        run_fixed(text, baseline, cfg, fake, &NoFixer)
    }

    fn run_fixed(
        text: &str,
        baseline: Option<&str>,
        cfg: &Config,
        fake: &Fake,
        fixer: &dyn Fixer,
    ) -> PolishResult {
        polish(text, baseline, &HashSet::new(), "# 規約", cfg, fake, fixer)
    }

    /// 偽の textlint: 各段落に関数を当てる。
    struct FnFixer(Box<dyn Fn(&str) -> String + Send + Sync>);

    impl Fixer for FnFixer {
        fn fix(&self, texts: &[String]) -> Vec<String> {
            texts.iter().map(|t| (self.0)(t)).collect()
        }
    }

    /// 「することができる」を「できる」に直す、偽の textlint。
    fn redundant_fixer() -> FnFixer {
        FnFixer(Box::new(|t| t.replace("することができる", "できる")))
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
    fn a_table_paragraph_is_not_pending_and_stays_as_written() {
        let text = "表の前の説明をする文章です。\n\n| 項目 | 内容 |\n|---|---|\n| 先行例 | 既存の仕組みを使う |\n\n表の後の説明をする文章です。\n";
        let p = pending(text, None, &HashSet::new());
        assert_eq!(
            p.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            vec![
                "表の前の説明をする文章です。",
                "表の後の説明をする文章です。"
            ],
            "表は書き直しの対象にしない"
        );
    }

    #[test]
    fn verify_refuses_a_response_that_echoes_the_context_fields() {
        let input = "一つ目の段落です。";
        for echoed in [
            "<before>前の段落</before>一つ目の段落です。",
            "一つ目の段落です。<after>後の段落</after>",
        ] {
            assert_eq!(
                verify(input, echoed),
                Err("応答に依頼の書式が混ざっています".to_string()),
                "{echoed}"
            );
        }
        // 段落の本文が、元からこの語を持つときは、混入とみなさない。
        let doc = "依頼の書式にある <before> という語を説明する段落です。";
        assert!(verify(doc, doc).is_ok());
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
    fn a_front_matter_skip_declaration_exempts_the_document_everywhere() {
        // F19・F38・F58: 書き直してはいけない文書は、front matter の宣言で、書き直しも関所も飛ばす。
        let body = "第三者の問題文を逐語で写した段落です。\n";
        let declared = format!("---\ntitle: 問題\nja-polish: skip\n---\n\n{body}");
        assert!(pending(&declared, None, &HashSet::new()).is_empty());
        // 書き直しの本体も、モデルを呼ばず、文書を変えない。
        let fake = Fake::new(vec![]);
        let r = run(&declared, None, &fake);
        assert!(fake.calls().is_empty(), "宣言した文書は、モデルに送らない");
        assert_eq!(r.text, declared);
        assert!(r.paras.is_empty() && r.entries.is_empty());
        // 値の大文字・小文字と、引用符は問わない。
        for v in ["Skip", "\"skip\"", "'SKIP'"] {
            let t = format!("---\nja-polish: {v}\n---\n\n{body}");
            assert!(pending(&t, None, &HashSet::new()).is_empty(), "{v}");
        }
    }

    #[test]
    fn a_skip_line_is_not_a_declaration_outside_the_front_matter_or_with_another_value() {
        let body = "通常の文書の段落です。\n";
        // 宣言の無い front matter は、対象のまま。
        let plain = format!("---\ntitle: 通常\n---\n\n{body}");
        assert_eq!(pending(&plain, None, &HashSet::new()).len(), 1);
        // 別の値は、宣言でない。
        let other = format!("---\nja-polish: enable\n---\n\n{body}");
        assert_eq!(pending(&other, None, &HashSet::new()).len(), 1);
        // front matter の外(本文)の同じ行は、宣言と読まない。
        let in_body = format!("{body}\nja-polish: skip\n");
        assert_eq!(pending(&in_body, None, &HashSet::new()).len(), 1);
        // 閉じる `---` の後ろ(本文側)の行も、宣言と読まない。
        let after = format!("---\ntitle: 通常\n---\nja-polish: skip\n\n{body}");
        assert_eq!(pending(&after, None, &HashSet::new()).len(), 1);
    }

    #[test]
    fn quoted_words_never_reach_the_model_and_survive_verbatim() {
        // F8・F18: 「…」の中の逐語の引用は、伏せ字にして送る。モデルは見ず、最終の本文に逐語で残る。
        let text =
            "報告では「することができる」という原文の言い回しを引いたことを説明する段落です。\n";
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">報告では ⟦P0⟧ という原文の言い回しを引いたことを説明する段落だ。</para>",
        )]);
        let r = run(text, None, &fake);
        let calls = fake.calls();
        assert!(
            !calls[0].user.contains("することができる"),
            "{}",
            calls[0].user
        );
        assert!(r.text.contains("「することができる」"), "{}", r.text);
        assert_eq!(r.paras[0].status, Status::Polished);
    }

    #[test]
    fn the_daily_limit_goes_straight_to_the_fallback_without_retrying() {
        // F6: 無料枠の 1 日の上限は、同じモデルへ再依頼しても通らない。主を 1 回だけ送って、予備へ移る。
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![
            Err(LlmError::new(
                FailureKind::DailyLimit,
                "requests per day limit exceeded on Free Tier",
            )),
            ok("<para n=\"1\">一つ目の段落だ。</para>"),
        ]);
        let c = Config {
            fallback_model: Some("fb".to_string()),
            ..cfg()
        };
        let r = run_with(text, None, &c, &fake);
        assert_eq!(r.text, "一つ目の段落だ。\n");
        let calls = fake.calls();
        assert_eq!(
            calls.iter().map(|c| c.model.as_str()).collect::<Vec<_>>(),
            vec!["m", "fb"],
            "主を 1 回だけ。再依頼しない"
        );
        assert_eq!(r.entries[0].model, "fb");
    }

    #[test]
    fn the_daily_limit_without_a_fallback_is_skipped_as_transient() {
        let text = "一つ目の段落です。\n";
        let fake = Fake::new(vec![Err(LlmError::new(
            FailureKind::DailyLimit,
            "requests per day limit exceeded on Free Tier",
        ))]);
        let r = run(text, None, &fake);
        assert_eq!(fake.calls().len(), 1, "予備が無ければ 1 回で止める");
        assert_eq!(r.text, text);
        assert!(
            r.entries[0].transient,
            "課金の検討を知らせる判定の入力になる"
        );
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
        // 答えたモデルを添える。応答の本文は、書式が崩れたとき以外は載せない。
        assert_eq!(r.skipped(), vec!["数値の集合が変わりました(モデル m)"]);
        assert!(!r.entries[0].transient, "照合の外れは一時的ではない");
    }

    #[test]
    fn a_response_wrapped_in_the_request_text_tag_is_unwrapped_and_passes() {
        // 依頼の囲み(<text>)ごと返すモデルの応答。外して読めば、通る。
        let text = "翻訳調の文章を直すことができる。\n";
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">\n<text>\n翻訳調の文章を直せる。\n</text>\n</para>",
        )]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, "翻訳調の文章を直せる。\n");
        assert_eq!(r.paras[0].status, Status::Polished);
        assert_eq!(fake.calls().len(), 1, "再依頼にならない");
    }

    #[test]
    fn a_format_failure_reason_carries_the_model_and_the_head_of_the_response() {
        // <before> も混ざる応答は外せない。2 回とも外れて素通しになり、理由から何が返ったかが読める。
        let text = "一つ目の段落です。\n";
        let bad = "<para n=\"1\">\n<before>\n前の段落\n</before>\n<text>\n一つ目の段落です。\n</text>\n</para>";
        let fake = Fake::new(vec![ok(bad), ok(bad)]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        let reason = r.skipped()[0].to_string();
        assert!(
            reason.starts_with("応答に依頼の書式が混ざっています(モデル m。応答の先頭: 「"),
            "{reason}"
        );
        assert!(reason.contains("<before>⏎前の段落⏎</before>"), "{reason}");
    }

    #[test]
    fn annotate_cuts_the_head_and_marks_newlines() {
        let long = "あ".repeat(HEAD_CHARS + 50);
        let r = annotate("理由".to_string(), "fb", Some(&long));
        assert_eq!(r.chars().filter(|c| *c == 'あ').count(), HEAD_CHARS);
        assert_eq!(
            annotate("理由".to_string(), "fb", Some("a\r\nb")),
            "理由(モデル fb。応答の先頭: 「a⏎⏎b」)"
        );
        assert_eq!(annotate("理由".to_string(), "fb", None), "理由(モデル fb)");
    }

    const LIST: &str = "- 承認済みの一つ目の項目です。\n- 承認済みの二つ目の項目です。\n";
    const ADDED: &str = "- 新しく足した三つ目の項目です。\n";

    #[test]
    fn a_baseline_compares_list_items_one_by_one() {
        // 承認済みの 2 項目に 1 項目を足した。基点があれば、足した項目だけが対象。
        let text = format!("{LIST}{ADDED}");
        let p = pending(&text, Some(LIST), &HashSet::new());
        assert_eq!(
            p.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            vec!["- 新しく足した三つ目の項目です。"]
        );
        assert_eq!(p[0].index, 2, "項目ごとの段落の並びでの番号");
        // 基点なしは、従来どおり段落全体(箇条書きは 1 段落)。
        let p = pending(&text, None, &HashSet::new());
        assert_eq!(p.len(), 1);
        assert!(p[0].text.contains("承認済みの一つ目") && p[0].text.contains("新しく足した"));
    }

    #[test]
    fn polish_sends_only_the_added_item_and_keeps_approved_items_verbatim() {
        let text = format!("## 課題\n\n{LIST}{ADDED}\nThe next paragraph is English.\n");
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">- 新しく足した三つ目の項目だ。</para>",
        )]);
        let c = cfg();
        let r = polish(
            &text,
            Some(LIST),
            &HashSet::new(),
            "# 規約",
            &c,
            &fake,
            &NoFixer,
        );
        let sent = fake.calls()[0].user.clone();
        assert_eq!(texts_in(&sent).len(), 1, "依頼は足した項目だけ: {sent}");
        // 隣の項目は文脈として見せるが、本文(`<text>`)には入れない。一つ目は隣でもない。
        assert!(!sent.contains("承認済みの一つ目"), "{sent}");
        assert_eq!(
            r.text,
            format!("## 課題\n\n{LIST}- 新しく足した三つ目の項目だ。\n\nThe next paragraph is English.\n")
        );
        assert_eq!(r.paras.len(), 1);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(
            r.entries[0].sha,
            blocks::hash("- 新しく足した三つ目の項目だ。")
        );
    }

    #[test]
    fn a_list_whose_items_are_all_recorded_passes_without_a_baseline() {
        // 基点つきで項目ごとに通した記録(項目のハッシュ)だけがある。基点なしで見ても、同じ箇条書きを求めない。
        let text = format!("{LIST}{ADDED}");
        let mut rec = HashSet::new();
        for item in [
            "- 承認済みの一つ目の項目です。",
            "- 承認済みの二つ目の項目です。",
        ] {
            rec.insert(blocks::hash(item));
        }
        assert_eq!(pending(&text, None, &rec).len(), 1, "足した項目が未記録");
        rec.insert(blocks::hash("- 新しく足した三つ目の項目です。"));
        assert!(pending(&text, None, &rec).is_empty());
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
    fn textlint_fixes_before_and_after_the_model() {
        let text = "検証することができる。\n";
        // モデルは前段が直した本文(「検証できる。」)を受け取り、冗長な言い回しを持ち込んで返す。
        let fake = Fake::new(vec![ok("<para n=\"1\">確認することができる。</para>")]);
        let r = run_fixed(text, None, &cfg(), &fake, &redundant_fixer());
        assert!(
            fake.calls()[0]
                .user
                .contains("<text>\n検証できる。\n</text>"),
            "前段で直した本文を送る: {}",
            fake.calls()[0].user
        );
        // 後段が、モデルの持ち込んだ冗長表現を直す。
        assert_eq!(r.text, "確認できる。\n");
        assert_eq!(r.paras[0].status, Status::Polished);
        assert_eq!(r.entries[0].sha, blocks::hash("確認できる。"));
    }

    #[test]
    fn the_pre_fix_survives_when_the_model_fails() {
        let text = "検証することができる。\n";
        let fake = Fake::new(vec![Err(LlmError::permanent("API キーが未設定"))]);
        let r = run_fixed(text, None, &cfg(), &fake, &redundant_fixer());
        // モデルが通せなくても、前段の直しは入る。記録の鍵は、最終の本文(直したあと)。
        assert_eq!(r.text, "検証できる。\n");
        assert_eq!(r.skipped(), vec!["API キーが未設定"]);
        assert_eq!(r.paras[0].before, "検証することができる。");
        assert_eq!(r.paras[0].after, "検証できる。");
        assert_eq!(r.entries[0].state, "skipped");
        assert_eq!(r.entries[0].sha, blocks::hash("検証できる。"));
    }

    #[test]
    fn a_skipped_paragraph_the_pre_fix_did_not_touch_stays_as_it_was() {
        let text = "そのままの段落です。\n";
        let fake = Fake::new(vec![Err(LlmError::permanent("キー無し"))]);
        let r = run_fixed(text, None, &cfg(), &fake, &redundant_fixer());
        assert_eq!(r.text, text);
        assert_eq!(r.entries[0].sha, blocks::hash("そのままの段落です。"));
    }

    #[test]
    fn a_fix_that_breaks_the_verification_is_dropped() {
        // 数値を変える直しは、照合(数値の集合)で外れる。
        let text = "検証は 3 件の段落です。\n";
        let bad = FnFixer(Box::new(|t| t.replace('3', "4")));
        let fake = Fake::new(vec![ok("<para n=\"1\">検証は 3 件の段落だ。</para>")]);
        let r = run_fixed(text, None, &cfg(), &fake, &bad);
        assert!(
            fake.calls()[0].user.contains("検証は 3 件の段落です。"),
            "直しを捨てて、元の本文を送る"
        );
        assert_eq!(r.text, "検証は 3 件の段落だ。\n");
    }

    #[test]
    fn a_fixer_that_returns_the_wrong_count_changes_nothing() {
        struct Short;
        impl Fixer for Short {
            fn fix(&self, _: &[String]) -> Vec<String> {
                Vec::new()
            }
        }
        let text = "一つ目の段落です。\n\n二つ目の段落です。\n";
        let fake = Fake::new(vec![ok(
            "<para n=\"1\">一つ目の段落だ。</para>\n<para n=\"2\">二つ目の段落だ。</para>",
        )]);
        let r = run_fixed(text, None, &cfg(), &fake, &Short);
        assert_eq!(r.text, "一つ目の段落だ。\n\n二つ目の段落だ。\n");
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
    fn verify_rejects_a_stray_closing_text_tag() {
        // F22: 囲みの外れた `</text>` が単独で残った応答は、文書に入れない。
        let input = "一つ目の段落です。";
        assert_eq!(
            verify(input, "一つ目の段落だ。\n</text>"),
            Err("応答に依頼の書式が混ざっています".to_string())
        );
        // 元から持つ段落は、混入とみなさない。
        let doc = "閉じタグ </text> という語を説明する段落です。";
        assert!(verify(doc, doc).is_ok());
    }

    #[test]
    fn verify_keeps_the_paren_width_of_the_document() {
        // F7: 半角の括弧の文書を、全角に書き直さない。
        let input = "設定(既定の値)を直す段落です。";
        assert!(verify(input, "設定(既定の値)を直す段落だ。").is_ok());
        assert!(verify(input, "設定（既定の値）を直す段落だ。")
            .unwrap_err()
            .contains("括弧"));
        // 全角の文書を半角にも変えない。
        let wide = "設定（既定の値）を直す段落です。";
        assert!(verify(wide, "設定(既定の値)を直す段落だ。")
            .unwrap_err()
            .contains("括弧"));
        assert!(verify(wide, "設定（既定の値）を直す段落だ。").is_ok());
    }

    #[test]
    fn verify_requires_a_line_head_token_to_stay_at_the_line_head() {
        // F36: `No-Visual:` などの接頭辞(伏せ字)が行頭から文中へ動いた書き直しは、PR 本文の骨組みを壊す。
        let input = "⟦P0⟧ 表の適性の説明を書いた段落です。";
        assert!(verify(input, "⟦P0⟧ 表の適性を説明した段落だ。").is_ok());
        let moved = verify(input, "変更は ⟦P0⟧ 表の適性を説明した段落だ。").unwrap_err();
        assert!(moved.contains("行頭にあった伏せ字"), "{moved}");
        // 文中の伏せ字は、動いてもよい(行頭の伏せ字ではない)。
        let mid = "実行するのは ⟦P0⟧ という手順です。";
        assert!(verify(mid, "⟦P0⟧ という手順を実行する。").is_ok());
    }

    #[test]
    fn verify_rejects_a_new_simplified_char_and_new_entity_refs() {
        // F40: 簡体字の混入。入力に元からあるなら、混入とみなさない。
        let input = "機能を換える段落です。";
        let e = verify(input, "机能を换える段落だ。").unwrap_err();
        assert!(e.contains("簡体字"), "{e}");
        let quoting = "中国語の「这」という字を説明する段落です。";
        assert!(verify(quoting, "中国語の「这」という字を説明した段落だ。").is_ok());
        // F68: 記号の実体参照が増えた書き直しは、元の文書に無い誤りを入れている。
        let cmp = "a > b を確認する段落です。";
        assert!(verify(cmp, "a > b を確認する段落だ。").is_ok());
        let e = verify(cmp, "a &gt; b を確認する段落だ。").unwrap_err();
        assert!(e.contains("実体参照"), "{e}");
    }

    #[test]
    fn the_second_request_drops_the_context_fields() {
        // F17・F21・F59: 応答に依頼の書式が混ざる型の失敗は、参考の欄の書式をなぞることで起きる。
        // 2 回目は、前後の参考を外して送る。
        let text = "前の段落です。\n\n真ん中は 3 件の段落です。\n\n後の段落です。\n";
        let fake = Fake::new(vec![
            ok("<para n=\"1\">前の段落だ。</para>\n<para n=\"2\">真ん中は 4 件の段落だ。</para>\n<para n=\"3\">後の段落だ。</para>"),
            ok("<para n=\"1\">真ん中は 3 件の段落だ。</para>"),
        ]);
        let r = run(text, None, &fake);
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].user.contains("<before>"), "1 回目は参考を付ける");
        assert!(
            !calls[1].user.contains("<before>") && !calls[1].user.contains("<after>"),
            "2 回目は参考を付けない: {}",
            calls[1].user
        );
        assert_eq!(
            r.text,
            "前の段落だ。\n\n真ん中は 3 件の段落だ。\n\n後の段落だ。\n"
        );
    }

    #[test]
    fn nothing_pending_means_no_request() {
        let fake = Fake::new(vec![]);
        let r = run("English only.\n", None, &fake);
        assert!(fake.calls().is_empty());
        assert!(r.paras.is_empty() && r.entries.is_empty());
    }
}
