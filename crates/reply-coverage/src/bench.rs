//! 計測: 見本(会話・下書き・期待)で、モデルごとに検出の質と時間を測る(`reply-coverage bench`)。
//!
//! 見本は 1 件 1 ディレクトリ: `conversation.thread`(会話の全文)・`draft.draft`(下書き)・
//! `expected.json`(`{"verdict":"pass|fail","unanswered":["引用の一部",…]}`)。
//! 拡張子を `.md`・`.txt` にしないのは、見本の擬似メッセージが文書として書き直しの対象にならないようにするため。
//! 何も書き換えず、記録も残さない。表に出すのは、見本の id と判定だけ(本文・相手の名前は出さない)。
//! 本物の見本(個人の会話)は非公開リポジトリに置き、`--cases` か環境変数 `REPLY_COVERAGE_PRIVATE_CASES` で指す。

use crate::backend::Backend;
use crate::check;
use crate::text::contains_verbatim;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
pub struct Expected {
    pub verdict: String,
    #[serde(default)]
    pub unanswered: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub id: String,
    pub conversation: String,
    pub draft: String,
    pub expected: Expected,
    pub dir: PathBuf,
}

/// `dir` の直下の、見本のディレクトリを名前順に読む。形の違うディレクトリは理由つきで失敗にする。
pub fn load_cases(dir: &Path) -> Result<Vec<Case>, String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{} を読めません: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    names.sort();
    let mut cases = Vec::new();
    for p in names {
        let id = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let read = |f: &str| {
            std::fs::read_to_string(p.join(f))
                .map_err(|e| format!("見本 {id} の {f} を読めません: {e}"))
        };
        let expected: Expected = serde_json::from_str(&read("expected.json")?)
            .map_err(|e| format!("見本 {id} の expected.json を読めません: {e}"))?;
        if expected.verdict != "pass" && expected.verdict != "fail" {
            return Err(format!("見本 {id} の verdict は pass か fail です"));
        }
        let (conversation, draft) = (read("conversation.thread")?, read("draft.draft")?);
        cases.push(Case {
            id,
            conversation,
            draft,
            expected,
            dir: p,
        });
    }
    Ok(cases)
}

#[derive(Debug, Clone)]
pub struct Row {
    pub case: String,
    pub model: String,
    pub secs: f64,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// 検出が成功した。`verdict_ok` は、合否が期待と一致したか。`found`/`expected` は、不合格の見本で、
    /// 期待の未回答の依頼のうち、検出が未回答として挙げられた数。
    Done {
        verdict_ok: bool,
        expected_pass: bool,
        found: usize,
        expected: usize,
    },
    Failed(String),
}

pub fn run(
    cases: &[Case],
    models: &[String],
    backend: &dyn Backend,
    cwd: &Path,
    me_hint: Option<&str>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for model in models {
        for c in cases {
            let start = Instant::now();
            let res = check::detect(backend, cwd, model, &c.conversation, &c.draft, me_hint);
            let secs = start.elapsed().as_secs_f64();
            let outcome = match res {
                Err(e) => Outcome::Failed(e.chars().take(60).collect()),
                Ok(v) => {
                    let expected_pass = c.expected.verdict == "pass";
                    let found = c
                        .expected
                        .unanswered
                        .iter()
                        .filter(|e| {
                            v.unanswered()
                                .iter()
                                .any(|a| contains_verbatim(&a.quote, e))
                        })
                        .count();
                    Outcome::Done {
                        verdict_ok: v.pass == expected_pass,
                        expected_pass,
                        found,
                        expected: c.expected.unanswered.len(),
                    }
                }
            };
            rows.push(Row {
                case: c.id.clone(),
                model: model.clone(),
                secs,
                outcome,
            });
        }
    }
    rows
}

/// 見本ごとの表と、モデルごとの集計。本文は出さない。
pub fn render(rows: &[Row]) -> String {
    let mut s = String::from("見本\tモデル\t判定\t未回答の検出\t秒\n");
    for r in rows {
        let (verdict, found) = match &r.outcome {
            Outcome::Failed(e) => (format!("失敗({e})"), "-".to_string()),
            Outcome::Done {
                verdict_ok,
                expected_pass,
                found,
                expected,
            } => (
                if *verdict_ok { "一致" } else { "不一致" }.to_string(),
                if *expected_pass {
                    "-".to_string()
                } else {
                    format!("{found}/{expected}")
                },
            ),
        };
        s.push_str(&format!(
            "{}\t{}\t{verdict}\t{found}\t{:.1}\n",
            r.case, r.model, r.secs
        ));
    }
    let mut models: Vec<&str> = rows.iter().map(|r| r.model.as_str()).collect();
    models.dedup();
    s.push_str(
        "\nモデル\t判定の一致\t不合格の見本の検出(再現)\t合格の見本の誤検出\t失敗\t平均秒\n",
    );
    for m in models {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.model == m).collect();
        let (mut ok, mut total, mut f, mut e, mut fp, mut failed) = (0, 0, 0, 0, 0, 0);
        for r in &mine {
            match &r.outcome {
                Outcome::Failed(_) => failed += 1,
                Outcome::Done {
                    verdict_ok,
                    expected_pass,
                    found,
                    expected,
                } => {
                    total += 1;
                    ok += usize::from(*verdict_ok);
                    f += found;
                    e += expected;
                    fp += usize::from(*expected_pass && !verdict_ok);
                }
            }
        }
        let avg = mine.iter().map(|r| r.secs).sum::<f64>() / mine.len().max(1) as f64;
        s.push_str(&format!(
            "{m}\t{ok}/{total}\t{f}/{e}\t{fp}\t{failed}\t{avg:.1}\n"
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendError, Request};

    struct Canned(String);
    impl Backend for Canned {
        fn complete(&self, _: &Request) -> Result<String, BackendError> {
            Ok(self.0.clone())
        }
    }

    fn write_case(root: &Path, id: &str, conv: &str, draft: &str, expected: &str) {
        let d = root.join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("conversation.thread"), conv).unwrap();
        std::fs::write(d.join("draft.draft"), draft).unwrap();
        std::fs::write(d.join("expected.json"), expected).unwrap();
    }

    #[test]
    fn loads_cases_in_name_order_and_rejects_a_bad_verdict() {
        let dir = tempfile::tempdir().unwrap();
        write_case(dir.path(), "b", "c", "d", r#"{"verdict":"pass"}"#);
        write_case(
            dir.path(),
            "a",
            "c",
            "d",
            r#"{"verdict":"fail","unanswered":["x"]}"#,
        );
        let cs = load_cases(dir.path()).unwrap();
        assert_eq!(
            cs.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        write_case(dir.path(), "c", "c", "d", r#"{"verdict":"maybe"}"#);
        assert!(load_cases(dir.path()).is_err());
    }

    #[test]
    fn run_scores_verdict_recall_and_false_positives_without_leaking_text() {
        let dir = tempfile::tempdir().unwrap();
        let conv = "## them 2026-01-01\n日程の候補を出します。火曜か水曜はどうですか。";
        write_case(
            dir.path(),
            "missed",
            conv,
            "了解です。",
            r#"{"verdict":"fail","unanswered":["火曜か水曜"]}"#,
        );
        write_case(
            dir.path(),
            "fine",
            conv,
            "了解です。",
            r#"{"verdict":"pass"}"#,
        );
        let cases = load_cases(dir.path()).unwrap();
        // 検出は「火曜か水曜はどうですか」を未回答と返す。missed では一致、fine では誤検出になる。
        let canned = Canned(
            r#"{"asks":[{"id":"1","quote":"火曜か水曜はどうですか","by":"them","kind":"date","status":"unanswered","evidence":"","late":false}],"apology_present":false,"apology_evidence":""}"#
                .to_string(),
        );
        let rows = run(&cases, &["m1".to_string()], &canned, dir.path(), None);
        let table = render(&rows);
        assert!(table.contains("fine\tm1\t不一致"), "{table}");
        assert!(table.contains("missed\tm1\t一致\t1/1"), "{table}");
        assert!(table.contains("m1\t1/2\t1/1\t1\t0"), "{table}");
        assert!(
            !table.contains("日程の候補") && !table.contains("了解です"),
            "{table}"
        );
    }
}
