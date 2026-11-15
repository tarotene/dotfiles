//! 架空の見本(`tests/fixtures/`)で、照合と判定の流れを決定論的に確かめる。
//!
//! 各見本の `canned.json` は「正しい検出器が返すはずの出力」を手で書いたもの。本物のモデルは呼ばない
//! (検出の質そのものは `reply-coverage bench` で測る)。ここで守るのは 2 つ:
//! 1. 正しい検出が、期待の合否になる(見本の整合。引用は会話・下書きに逐語である)。
//! 2. 検出が偽って「全部答えた」と返しても、根拠の照合で不合格が残る(負例。test-grounding §5)。

use reply_coverage::bench::{load_cases, Case};
use reply_coverage::detect::{judge, Detection};
use std::path::PathBuf;

fn cases() -> Vec<Case> {
    load_cases(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")).unwrap()
}

fn canned(c: &Case) -> Detection {
    let s = std::fs::read_to_string(c.dir.join("canned.json")).unwrap();
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", c.id))
}

#[test]
fn the_fixture_set_has_both_failing_and_passing_cases() {
    let cs = cases();
    assert!(cs.len() >= 10, "{} 件", cs.len());
    assert!(cs.iter().any(|c| c.expected.verdict == "fail"));
    assert!(cs.iter().any(|c| c.expected.verdict == "pass"));
}

#[test]
fn a_correct_detection_gives_the_expected_verdict_with_nothing_dropped() {
    for c in cases() {
        let v = judge(&canned(&c), &c.conversation, &c.draft);
        assert_eq!(v.pass, c.expected.verdict == "pass", "{}: {v:?}", c.id);
        assert_eq!(v.dropped, 0, "{}: 引用が会話に逐語でない", c.id);
        assert!(
            v.asks.iter().all(|a| !a.downgraded),
            "{}: 根拠が照合に外れた",
            c.id
        );
        for e in &c.expected.unanswered {
            assert!(
                v.unanswered().iter().any(|a| a.quote.contains(e.as_str())),
                "{}: 期待の未回答「{e}」が出ない",
                c.id
            );
        }
    }
}

#[test]
fn a_lying_detector_cannot_turn_a_failing_draft_into_a_pass() {
    let mut checked = 0;
    for c in cases().into_iter().filter(|c| c.expected.verdict == "fail") {
        let mut det = canned(&c);
        // すべての依頼を「答えた」に偽り、下書きに無い根拠と、下書きに無い詫びを添える。
        for a in &mut det.asks {
            if !a.status.responded_now() {
                a.status = serde_json::from_value(serde_json::json!("answered")).unwrap();
                a.evidence = "ご提案の日程で問題ありません".to_string();
            }
        }
        det.apology_present = true;
        det.apology_evidence = "下書きに無い詫びの文".to_string();
        let v = judge(&det, &c.conversation, &c.draft);
        assert!(!v.pass, "{}: 偽りの検出が通った: {v:?}", c.id);
        checked += 1;
    }
    assert!(checked >= 5, "不合格の見本が少ない: {checked}");
}

#[test]
fn a_detection_that_invents_asks_does_not_break_a_passing_draft() {
    // 存在しない依頼(引用が会話に無い)を挙げても、合格の下書きは合格のまま(捨てるだけ)。
    for c in cases().into_iter().filter(|c| c.expected.verdict == "pass") {
        let mut det = canned(&c);
        det.asks.push(
            serde_json::from_value(serde_json::json!({
                "id": "x", "quote": "この引用は会話のどこにも無い", "by": "them",
                "kind": "question", "status": "unanswered", "evidence": "", "late": false
            }))
            .unwrap(),
        );
        let v = judge(&det, &c.conversation, &c.draft);
        assert!(v.pass && v.dropped == 1, "{}: {v:?}", c.id);
    }
}
