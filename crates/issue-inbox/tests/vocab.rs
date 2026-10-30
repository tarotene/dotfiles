//! `config/agents/codify-signals/vocabulary.json` の公開上の検査。
//!
//! 語彙は public リポジトリに載るので、事例(`examples`)はこのリポジトリの ADR・PR の番号だけにする
//! (ADR-0034。他のリポジトリや業務の行は件数にだけ使う)。形式と ADR の実在を機械で固定すると、
//! 外部の事例を書き込めなくなる。

use issue_inbox::codify::Vocabulary;
use std::path::PathBuf;

fn adr_numbers() -> Vec<u64> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/adr");
    std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} が読めない: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.split('-').next()?.parse::<u64>().ok()
        })
        .collect()
}

#[test]
fn examples_are_adr_or_issue_numbers_of_this_repository() {
    let adrs = adr_numbers();
    let v = Vocabulary::get();
    for s in &v.signals {
        for ex in &s.examples {
            if let Some(n) = ex.strip_prefix("ADR-") {
                let n: u64 = n
                    .parse()
                    .unwrap_or_else(|_| panic!("{}: '{ex}' は ADR-<数字> の形でない", s.id));
                assert!(
                    adrs.contains(&n),
                    "{}: '{ex}' に対応する docs/adr/<番号>-*.md が無い",
                    s.id
                );
            } else if let Some(n) = ex.strip_prefix('#') {
                assert!(
                    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()),
                    "{}: '{ex}' は #<数字> の形でない",
                    s.id
                );
            } else {
                panic!("{}: '{ex}' は ADR-<数字> か #<数字> のどちらかで書く", s.id);
            }
        }
    }
}

#[test]
fn drop_reasons_are_the_closed_six() {
    let v = Vocabulary::get();
    assert_eq!(
        v.drop_reasons,
        [
            "semantic-judgment",
            "unobservable",
            "preference",
            "one-off",
            "existing-suffices",
            "misread-signal"
        ]
    );
}

#[test]
fn each_signal_has_a_since_date() {
    let v = Vocabulary::get();
    for s in &v.signals {
        let b = s.since.as_bytes();
        assert!(
            b.len() == 10 && b[4] == b'-' && b[7] == b'-',
            "{}: since '{}' は YYYY-MM-DD でない",
            s.id,
            s.since
        );
    }
}
