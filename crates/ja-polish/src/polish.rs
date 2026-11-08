//! 書き直しの本体と、「どの段落が未通過か」の判定。
//!
//! 判定([`pending`])は LLM を呼ばず、記録だけで決まる。投稿の関所と文書ファイルの関所は
//! これだけを呼ぶ。書き直し([`polish`])は同じ判定で対象を決め、モデルに依頼し、戻ってきた
//! 段落を照合してから差し替える。

use crate::blocks::{self, Block, MIN_JP_CHARS};
use crate::llm::{self, Backend, Item};
use crate::mask::{self, Masked};
use crate::records::Entry;
use std::collections::HashSet;
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

/// 文書の未通過の日本語の段落を書き直す。
///
/// - `baseline`: 基点の文書。あれば、そこに無い(追加・変更された)段落だけが対象。
/// - `records`: 通した段落の `sha` の集合。
/// - 依頼は 1 回。照合で外れた段落だけ、もう 1 回依頼する。モデルが答えない・失敗したときは
///   再依頼せず、対象の段落すべてを素通し(元の本文のまま)にして理由を残す。
pub fn polish(
    text: &str,
    baseline: Option<&str>,
    records: &HashSet<String>,
    style: &str,
    model: &str,
    timeout: Duration,
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

    let mut outputs: Vec<Result<String, String>> =
        vec![Err("依頼していません".to_string()); idxs.len()];
    let mut todo: Vec<usize> = (0..idxs.len()).collect();
    for _attempt in 0..2 {
        if todo.is_empty() {
            break;
        }
        let sub: Vec<Item> = todo.iter().map(|&k| items[k].clone()).collect();
        match backend.complete(model, &system, &llm::user_message(&sub), timeout) {
            Err(reason) => {
                for &k in &todo {
                    outputs[k] = Err(reason.clone());
                }
                break;
            }
            Ok(resp) => {
                let parsed = llm::parse_response(&resp);
                let mut next = Vec::new();
                for (pos, &k) in todo.iter().enumerate() {
                    let verdict = match parsed.get(&(pos + 1)) {
                        None => Err("応答に該当する段落がありません".to_string()),
                        Some(out) => verify(&masked[k].text, out),
                    };
                    if verdict.is_err() {
                        next.push(k);
                    }
                    outputs[k] = verdict;
                }
                todo = next;
            }
        }
    }

    let mut paras = Vec::new();
    let mut entries = Vec::new();
    for (k, &i) in idxs.iter().enumerate() {
        let original = blocks[i].text.clone();
        match &outputs[k] {
            Ok(out) => {
                let restored = mask::unmask(out, &masked[k].slots);
                let changed = blocks::normalize(&restored) != blocks::normalize(&original);
                if changed {
                    blocks[i].text = restored.clone();
                }
                entries.push(Entry::polished(blocks::hash(&restored), model));
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
            Err(reason) => {
                entries.push(Entry::skipped(blocks::hash(&original), model, reason));
                paras.push(Para {
                    before: original.clone(),
                    after: original,
                    status: Status::Skipped(reason.clone()),
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
    use std::cell::RefCell;

    /// 依頼を記録し、決めた応答を返す。
    struct Fake {
        replies: RefCell<Vec<Result<String, String>>>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(replies: Vec<Result<String, String>>) -> Self {
            Fake {
                replies: RefCell::new(replies),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Backend for Fake {
        fn complete(&self, _m: &str, _s: &str, user: &str, _t: Duration) -> Result<String, String> {
            self.calls.borrow_mut().push(user.to_string());
            let mut r = self.replies.borrow_mut();
            if r.is_empty() {
                Err("応答が尽きました".to_string())
            } else {
                r.remove(0)
            }
        }
    }

    fn run(text: &str, baseline: Option<&str>, fake: &Fake) -> PolishResult {
        polish(
            text,
            baseline,
            &HashSet::new(),
            "# 規約",
            "m",
            Duration::from_secs(1),
            fake,
        )
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
        let fake = Fake::new(vec![Ok(
            "<para n=\"1\">\n文章を直す。⟦P0⟧ を残す。\n</para>".to_string(),
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
        assert_eq!(
            r.entries[0].sha,
            blocks::hash("文章を直す。`code` を残す。")
        );
        // 見出しは依頼に載らない。
        let sent = fake.calls.borrow()[0].clone();
        assert!(
            !sent.contains("課題\n</text>"),
            "見出しは対象にしない: {sent}"
        );
        assert!(sent.contains("<before>"), "前の段落は参考として渡す");
    }

    #[test]
    fn unchanged_when_model_returns_same_text() {
        let text = "すでに自然な日本語の文です。\n";
        let fake = Fake::new(vec![Ok(
            "<para n=\"1\">すでに自然な日本語の文です。</para>".to_string(),
        )]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.paras[0].status, Status::Unchanged);
        assert_eq!(r.entries[0].state, "polished");
    }

    #[test]
    fn backend_failure_skips_every_target_without_retry() {
        let text = "一つ目の段落です。\n\n二つ目の段落です。\n";
        let fake = Fake::new(vec![Err("API キーが未設定".into())]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.skipped(), vec!["API キーが未設定", "API キーが未設定"]);
        assert!(r.entries.iter().all(|e| e.state == "skipped"));
        assert_eq!(fake.calls.borrow().len(), 1, "失敗では再依頼しない");
    }

    #[test]
    fn failed_verification_retries_only_that_paragraph_once() {
        let text = "一つ目は 3 件の段落です。\n\n二つ目の段落です。\n";
        let fake = Fake::new(vec![
            // 1 回目: 一つ目は数値を変えた(照合で外れる)、二つ目は良い。
            Ok("<para n=\"1\">一つ目は 4 件の段落です。</para>\n<para n=\"2\">二つ目の段落だ。</para>".to_string()),
            // 2 回目: 一つ目だけ再依頼されて、直った。
            Ok("<para n=\"1\">一つ目は 3 件の段落だ。</para>".to_string()),
        ]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, "一つ目は 3 件の段落だ。\n\n二つ目の段落だ。\n");
        assert_eq!(fake.calls.borrow().len(), 2);
        assert!(fake.calls.borrow()[1].contains("3 件"));
        assert!(
            !fake.calls.borrow()[1].contains("<para n=\"2\">"),
            "再依頼は 1 段落だけ"
        );
    }

    #[test]
    fn still_failing_after_retry_is_skipped_with_the_reason() {
        let text = "一つ目は 3 件の段落です。\n";
        let bad = "<para n=\"1\">一つ目は 4 件の段落です。</para>".to_string();
        let fake = Fake::new(vec![Ok(bad.clone()), Ok(bad)]);
        let r = run(text, None, &fake);
        assert_eq!(r.text, text);
        assert_eq!(r.skipped(), vec!["数値の集合が変わりました"]);
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
        assert!(fake.calls.borrow().is_empty());
        assert!(r.paras.is_empty() && r.entries.is_empty());
    }
}
