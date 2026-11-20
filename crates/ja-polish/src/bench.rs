//! 測定(`ja-polish bench`): 固定の見本の文書で、「モデル × 思考量」の組ごとに、応答時間・書き直しの結果・
//! 書き直しが持ち込む劣化を数える(docs/adr/1387-ja-polish.md の D11・D15)。
//!
//! 速さだけで思考量やモデルを選ぶと、書き直しがかえって文を硬くしても気づけない(2026-11-12 の観察:
//! 半角括弧が全角になる、冗長表現が残る・増える)。同じ見本で、時間と質の物差しを並べて選ぶ。
//!
//! - 書き直しは記録を使わず(すべて未通過として扱う)、基点も使わない。何も書き換えず、記録もしない。
//! - 予備のモデルは使わない(混雑で予備に流れて、組の違いが見えなくなるのを避ける)。textlint の前後段も
//!   挟まない(モデル自身の出力を測る)。textlint は、指摘数を数えるためだけに使う。

use crate::blocks;
use crate::config::Config;
use crate::llm::Backend;
use crate::mask;
use crate::polish::{polish, Status};
use crate::textlint::{Fixer, NoFixer};
use std::collections::HashSet;
use std::time::Instant;

/// 見本の文書 1 本。
#[derive(Debug, Clone)]
pub struct Sample {
    /// 一覧に書いた指定(`<commit>:<path>`)。
    pub spec: String,
    pub text: String,
}

/// 見本の一覧を読む。1 行 1 件の `<commit>:<path>`。空行と、`#` で始まる行は読み飛ばす。中身は `show`
/// (`git show <commit>:<path>` を呼ぶ関数)で取り出す。1 件でも取れなければ、全体を `Err` にする
/// (見本が欠けたまま測ると、組どうしの比較が合わなくなる)。
pub fn load_samples(
    list: &str,
    show: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Vec<Sample>, String> {
    let mut out = Vec::new();
    for line in list.lines() {
        let spec = line.trim();
        if spec.is_empty() || spec.starts_with('#') {
            continue;
        }
        let text = show(spec).map_err(|e| format!("見本 {spec} を取れません: {e}"))?;
        out.push(Sample {
            spec: spec.to_string(),
            text,
        });
    }
    if out.is_empty() {
        return Err("見本が 1 件もありません".to_string());
    }
    Ok(out)
}

/// 測る組: モデルと思考量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub model: String,
    pub thinking: Option<String>,
}

impl Variant {
    /// `model` か `model@level` を読む。
    pub fn parse(s: &str) -> Variant {
        match s.split_once('@') {
            Some((m, l)) if !l.trim().is_empty() => Variant {
                model: m.trim().to_string(),
                thinking: Some(l.trim().to_string()),
            },
            _ => Variant {
                model: s.trim_end_matches('@').trim().to_string(),
                thinking: None,
            },
        }
    }

    pub fn label(&self) -> String {
        match &self.thinking {
            Some(l) => format!("{}@{l}", self.model),
            None => format!("{}@既定", self.model),
        }
    }
}

/// 1 つの組の、全見本を通した結果。
#[derive(Debug, Clone, Default)]
pub struct Row {
    pub label: String,
    pub secs: f64,
    pub paras: usize,
    pub polished: usize,
    pub unchanged: usize,
    pub skipped: usize,
    /// 素通しのうち、混雑・時間切れによるもの(組の比較が、混雑に左右されたかを見る)。
    pub transient_skipped: usize,
    /// 書き直しで、全角の括弧(`（` `）`)が増えた数(減れば負)。文体規約に指定が無いのに全角にする劣化。
    pub fullwidth_parens_added: i64,
    /// 書き直しが、元の語を別の語に置き換えた数([`WATCHED_SWAPS`] の組。2026-11-12 の実測の語から始める)。
    pub term_swaps: usize,
    /// 答えた段落(書き直し・変更なし)のうち、書き直しの後に、前の本文に無い行の数と、行の総数。
    /// 割合が高いほど、直す必要が無い所まで書き換えている。
    pub changed_lines: usize,
    pub total_lines: usize,
    /// textlint の指摘(冗長表現)の数。全段落の、書き直しの前と後。textlint が無いときは `None`。
    pub issues_before: Option<usize>,
    pub issues_after: Option<usize>,
}

fn fullwidth_parens(s: &str) -> i64 {
    s.chars().filter(|c| matches!(c, '（' | '）')).count() as i64
}

/// 置き換えを数える語の組(元の語, 持ち込まれた語)。2026-11-12 に、書き直しが承認済みの文の語を変えた実測
/// (機体→マシン、会社契約→法人契約、戻す条件→復元条件、通さない→送信してはならない)から始める。
/// 語の選定は、測定の入力を増やすたびに足す。辞書(prh)の語の選定とは別(辞書は本人の文体の判断)。
pub const WATCHED_SWAPS: &[(&str, &str)] = &[
    ("機体", "マシン"),
    ("会社契約", "法人契約"),
    ("戻す条件", "復元条件"),
    ("通さない", "送信してはならない"),
];

/// 1 段落の、書き直しの前後での置き換えの数。組ごとに、元の語が減り、持ち込まれた語が増えた回数のうち、少ない方。
fn term_swaps(before: &str, after: &str) -> usize {
    WATCHED_SWAPS
        .iter()
        .map(|(from, to)| {
            let lost = before
                .matches(from)
                .count()
                .saturating_sub(after.matches(from).count());
            let gained = after
                .matches(to)
                .count()
                .saturating_sub(before.matches(to).count());
            lost.min(gained)
        })
        .sum()
}

/// 書き直しの後の本文のうち、前の本文に無い行の数と、行の総数(空行は数えない)。行は空白をそろえて比べる。
fn changed_lines(before: &str, after: &str) -> (usize, usize) {
    let old: HashSet<String> = before.lines().map(blocks::normalize).collect();
    let lines: Vec<&str> = after.lines().filter(|l| !l.trim().is_empty()).collect();
    let changed = lines
        .iter()
        .filter(|l| !old.contains(&blocks::normalize(l)))
        .count();
    (changed, lines.len())
}

/// 1 つの組で、全見本を書き直して数える。`counter` は指摘を数えるだけ(`Fixer::issues`)。
pub fn measure(
    variant: &Variant,
    base: &Config,
    samples: &[Sample],
    style: &str,
    backend: &dyn Backend,
    counter: &dyn Fixer,
) -> Row {
    let cfg = Config {
        model: variant.model.clone(),
        thinking_level: variant.thinking.clone(),
        fallback_model: None,
        ..base.clone()
    };
    let mut row = Row {
        label: variant.label(),
        ..Row::default()
    };
    let mut before: Vec<String> = Vec::new();
    let mut after: Vec<String> = Vec::new();
    let started = Instant::now();
    for sample in samples {
        let result = polish(
            &sample.text,
            None,
            &HashSet::new(),
            style,
            &cfg,
            backend,
            &NoFixer,
        );
        for (p, e) in result.paras.iter().zip(&result.entries) {
            row.paras += 1;
            match &p.status {
                Status::Polished => {
                    row.polished += 1;
                    row.fullwidth_parens_added +=
                        fullwidth_parens(&p.after) - fullwidth_parens(&p.before);
                    row.term_swaps += term_swaps(&p.before, &p.after);
                    let (changed, total) = changed_lines(&p.before, &p.after);
                    row.changed_lines += changed;
                    row.total_lines += total;
                }
                Status::Unchanged => {
                    row.unchanged += 1;
                    row.total_lines += changed_lines(&p.before, &p.after).1;
                }
                Status::Skipped(_) => {
                    row.skipped += 1;
                    if e.transient {
                        row.transient_skipped += 1;
                    }
                }
            }
            before.push(mask::mask(&p.before).text);
            after.push(mask::mask(&p.after).text);
        }
    }
    row.secs = started.elapsed().as_secs_f64();
    row.issues_before = counter.issues(&before);
    row.issues_after = counter.issues(&after);
    row
}

/// 結果の表(Markdown)。
pub fn render_table(rows: &[Row], samples: &[Sample]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "見本 {} 本、対象の段落 {} 個(組ごとに同じ)\n\n",
        samples.len(),
        rows.first().map_or(0, |r| r.paras)
    ));
    out.push_str("| 組 | 時間(秒) | 書き直し | 変更なし | 素通し(混雑・時間切れ) | 全角括弧の増減 | 用語の置き換え | 変更行の割合 | 冗長表現の指摘(前→後) |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for r in rows {
        let issues = match (r.issues_before, r.issues_after) {
            (Some(b), Some(a)) => format!("{b} → {a}"),
            _ => "未計測".to_string(),
        };
        let ratio = if r.total_lines == 0 {
            "-".to_string()
        } else {
            format!(
                "{:.0}%",
                r.changed_lines as f64 * 100.0 / r.total_lines as f64
            )
        };
        out.push_str(&format!(
            "| {} | {:.0} | {} | {} | {} ({}) | {:+} | {} | {} | {} |\n",
            r.label,
            r.secs,
            r.polished,
            r.unchanged,
            r.skipped,
            r.transient_skipped,
            r.fullwidth_parens_added,
            r.term_swaps,
            ratio,
            issues
        ));
    }
    out
}

/// 一覧の見本の数の目安(docs/agents/ja-polish.md)。
pub fn paragraph_count(text: &str) -> usize {
    blocks::split(text)
        .iter()
        .filter(|b| {
            !b.verbatim && blocks::jp_count(&mask::mask(&b.text).text) >= blocks::MIN_JP_CHARS
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmError, Request};
    use regex::Regex;

    /// 各段落の「です」を「だ」に直し、`wide` なら半角括弧を全角にする偽のモデル。
    struct Model {
        wide: bool,
    }

    impl Backend for Model {
        fn complete(&self, req: &Request) -> Result<String, LlmError> {
            let re = Regex::new(r#"(?s)<para n="(\d+)">.*?<text>\n(.*?)\n</text>"#).unwrap();
            Ok(re
                .captures_iter(req.user)
                .map(|c| {
                    let mut t = c[2].replace("です", "だ");
                    if self.wide {
                        t = t.replace('(', "（").replace(')', "）");
                    }
                    format!("<para n=\"{}\">{t}</para>", &c[1])
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }

    /// 冗長表現(「することができる」)の数を数える偽の textlint。
    struct Counter;

    impl Fixer for Counter {
        fn fix(&self, texts: &[String]) -> Vec<String> {
            texts.to_vec()
        }
        fn issues(&self, texts: &[String]) -> Option<usize> {
            Some(
                texts
                    .iter()
                    .map(|t| t.matches("することができる").count())
                    .sum(),
            )
        }
    }

    fn sample(text: &str) -> Sample {
        Sample {
            spec: "x:y".into(),
            text: text.into(),
        }
    }

    fn cfg() -> Config {
        Config {
            retry_base: std::time::Duration::ZERO,
            ..Config::default()
        }
    }

    #[test]
    fn variant_parse_and_label() {
        let v = Variant::parse("gemini-3.6-flash@minimal");
        assert_eq!(v.model, "gemini-3.6-flash");
        assert_eq!(v.thinking.as_deref(), Some("minimal"));
        assert_eq!(v.label(), "gemini-3.6-flash@minimal");
        let d = Variant::parse("gemini-flash-lite-latest");
        assert_eq!(d.thinking, None);
        assert_eq!(d.label(), "gemini-flash-lite-latest@既定");
        assert_eq!(Variant::parse("m@").thinking, None);
    }

    #[test]
    fn load_samples_skips_comments_and_fails_on_a_missing_sample() {
        let show = |s: &str| {
            if s == "abc:ok.md" {
                Ok("本文です。\n".to_string())
            } else {
                Err("無い".to_string())
            }
        };
        let list = "# 見本\n\nabc:ok.md\n";
        let s = load_samples(list, &show).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].spec, "abc:ok.md");
        assert!(load_samples("abc:ok.md\nabc:none.md\n", &show)
            .unwrap_err()
            .contains("abc:none.md"));
        assert!(load_samples("# だけ\n", &show).is_err());
    }

    #[test]
    fn measure_counts_statuses_parens_and_issues() {
        let samples = vec![sample(
            "検証することができる段落です。\n\n設定(任意)を読み込む段落です。\n",
        )];
        let plain = measure(
            &Variant::parse("m"),
            &cfg(),
            &samples,
            "# 規約",
            &Model { wide: false },
            &Counter,
        );
        assert_eq!((plain.paras, plain.polished, plain.skipped), (2, 2, 0));
        assert_eq!(plain.fullwidth_parens_added, 0);
        assert_eq!(
            (plain.issues_before, plain.issues_after),
            (Some(1), Some(1)),
            "冗長表現は、モデルが直さなければ残る"
        );

        let wide = measure(
            &Variant::parse("m@low"),
            &cfg(),
            &samples,
            "# 規約",
            &Model { wide: true },
            &Counter,
        );
        // 「（任意）」で全角が 2 つ増える応答は、照合(`polish::verify`)が括弧の幅の変化として拒否する(F7)。
        // その段落は素通しになり、`skipped` に数えられる。照合を通った出力には、増えた全角の括弧が残らない。
        assert_eq!((wide.paras, wide.polished, wide.skipped), (2, 1, 1));
        assert_eq!(wide.fullwidth_parens_added, 0);
        assert_eq!(wide.label, "m@low");
    }

    #[test]
    fn measure_marks_transient_skips_and_never_uses_the_fallback() {
        struct Busy;
        impl Backend for Busy {
            fn complete(&self, req: &Request) -> Result<String, LlmError> {
                assert_ne!(req.model, "fb", "測定は予備のモデルを使わない");
                Err(LlmError::new(crate::llm::FailureKind::Busy, "high demand"))
            }
        }
        let c = Config {
            fallback_model: Some("fb".into()),
            ..cfg()
        };
        let r = measure(
            &Variant::parse("m"),
            &c,
            &[sample("一つ目の段落です。\n")],
            "# 規約",
            &Busy,
            &Counter,
        );
        assert_eq!((r.skipped, r.transient_skipped), (1, 1));
    }

    #[test]
    fn render_table_has_one_row_per_variant() {
        let rows = vec![
            Row {
                label: "a@既定".into(),
                secs: 12.4,
                paras: 5,
                polished: 4,
                unchanged: 1,
                issues_before: Some(3),
                issues_after: Some(1),
                ..Row::default()
            },
            Row {
                label: "b@low".into(),
                paras: 5,
                skipped: 2,
                transient_skipped: 2,
                fullwidth_parens_added: 3,
                term_swaps: 2,
                changed_lines: 1,
                total_lines: 4,
                ..Row::default()
            },
        ];
        let t = render_table(&rows, &[sample("x")]);
        assert!(
            t.contains("| a@既定 | 12 | 4 | 1 | 0 (0) | +0 | 0 | - | 3 → 1 |"),
            "{t}"
        );
        assert!(
            t.contains("| b@low | 0 | 0 | 0 | 2 (2) | +3 | 2 | 25% | 未計測 |"),
            "{t}"
        );
        assert!(t.contains("対象の段落 5 個"), "{t}");
    }

    #[test]
    fn term_swaps_count_a_lost_word_replaced_by_a_new_one() {
        // 機体→マシンの置き換え 2 回。元から「マシン」がある所は、増えた分だけ数える。
        assert_eq!(
            term_swaps(
                "この機体と別の機体を使う。",
                "このマシンと別のマシンを使う。"
            ),
            2
        );
        assert_eq!(term_swaps("機体を直す。", "機体を直す。"), 0);
        // 元の語が減っても、持ち込まれた語が増えていなければ、置き換えとは数えない(削っただけ)。
        assert_eq!(term_swaps("機体を直す。", "直す。"), 0);
        // 持ち込まれた語が元からあり、増えていない。
        assert_eq!(term_swaps("機体とマシン。", "マシンとマシン。"), 1);
        assert_eq!(term_swaps("機体とマシン。", "機体とマシン。"), 0);
        // 組ごとに数える。
        assert_eq!(term_swaps("戻す条件と機体。", "復元条件とマシン。"), 2);
    }

    #[test]
    fn changed_lines_counts_lines_not_in_the_before_text_and_ignores_spacing() {
        let before = "一行目です。\n二行目です。\n  三行目です。\n";
        // 二行目だけが変わった。空白の違いと空行は数えない。
        let after = "一行目です。\n\n二行目だ。\n三行目です。\n";
        assert_eq!(changed_lines(before, after), (1, 3));
        assert_eq!(changed_lines(before, before), (0, 3));
        assert_eq!(changed_lines("", ""), (0, 0));
    }

    #[test]
    fn measure_counts_term_swaps_and_changed_lines() {
        struct Swap;
        impl Backend for Swap {
            fn complete(&self, req: &Request) -> Result<String, LlmError> {
                let re = Regex::new(r#"(?s)<para n="(\d+)">.*?<text>\n(.*?)\n</text>"#).unwrap();
                Ok(re
                    .captures_iter(req.user)
                    .map(|c| {
                        format!(
                            "<para n=\"{}\">{}</para>",
                            &c[1],
                            c[2].replace("機体", "マシン")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        // 1 段落目は置き換えられ、2 段落目は変わらない(変更なし)。
        let samples = vec![sample("この機体を使う段落です。\n\n変えない段落です。\n")];
        let r = measure(
            &Variant::parse("m"),
            &cfg(),
            &samples,
            "# 規約",
            &Swap,
            &Counter,
        );
        assert_eq!((r.polished, r.unchanged), (1, 1));
        assert_eq!(r.term_swaps, 1);
        assert_eq!((r.changed_lines, r.total_lines), (1, 2));
    }

    #[test]
    fn paragraph_count_skips_code_and_short_paragraphs() {
        assert_eq!(
            paragraph_count(
                "日本語の段落です。\n\n```\n日本語のコードです\n```\n\n検証\n\nEnglish.\n"
            ),
            1
        );
    }
}
