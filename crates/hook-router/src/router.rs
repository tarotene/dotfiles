//! 規則を並列に走らせ、結果を 1 つの応答にまとめる。
//!
//! 契約(旧: 検査ごとに別の hook として登録し、Claude Code が並列に走らせ、結果をまとめていた):
//! - 各規則は別スレッドで走る。1 つが panic しても、ほかの規則は走り、panic した規則は
//!   素通り(fail-open)にする。`agent-permissions` だけは拒否する。
//! - 各規則には、かつて個別の hook として宣言していた `timeout` と同じ時間(`Rule::budget`)を
//!   許す。越えた規則の結果は捨てて素通りにする。スレッドは外から止められないので、
//!   呼び出し側(`main`)は応答を出したあと、プロセスごと終わる。
//! - PreToolUse: deny → ask → allow → 通常処理。理由は、表の順にすべて並べる。
//! - `additionalContext`: すべて、表の順に改行でつなぐ。
//! - 素通りさせた規則(panic・時間切れ)があれば、`systemMessage` で利用者に知らせる。

use crate::rules::{Event, Rule};
use hook_io::{Agent, Outcome};
use serde_json::{json, Map, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Instant;

/// 規則 1 件の、実行の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 最後まで走った(`None` は「判定しない」)。
    Done(Option<Outcome>),
    /// panic した(素通りにした)。
    Panicked,
    /// 許した時間を越えた(素通りにした)。
    TimedOut,
}

/// 規則を並列に走らせ、`rules` と同じ並びで結果を返す。
pub fn execute(rules: &[Rule], raw: &str) -> Vec<(&'static str, Verdict)> {
    execute_for(Agent::Claude, rules, raw)
}

/// Run the same rule table with the caller identity, without process-global environment changes.
pub fn execute_for(agent: Agent, rules: &[Rule], raw: &str) -> Vec<(&'static str, Verdict)> {
    let inputs: Vec<_> = rules.iter().map(|rule| (*rule, raw.to_owned())).collect();
    execute_inputs_for(agent, &inputs)
}

/// Run original-payload checks and normalized tool checks in the same bounded batch.
pub fn execute_inputs_for(agent: Agent, inputs: &[(Rule, String)]) -> Vec<(&'static str, Verdict)> {
    let (tx, rx) = mpsc::channel::<(usize, Verdict)>();
    let start = Instant::now();
    let mut results: Vec<Option<Verdict>> = vec![None; inputs.len()];
    let mut pending: Vec<(usize, Instant)> = Vec::new();

    for (i, (rule, raw)) in inputs.iter().enumerate() {
        let tx = tx.clone();
        let raw = Arc::new(raw.to_owned());
        let run = rule.run;
        let spawned = std::thread::Builder::new()
            .name(format!("rule-{}", rule.node))
            .spawn(move || {
                let verdict = match catch_unwind(AssertUnwindSafe(|| run(agent, &raw))) {
                    Ok(outcome) => Verdict::Done(outcome),
                    Err(_) => Verdict::Panicked,
                };
                let _ = tx.send((i, verdict));
            });
        match spawned {
            Ok(_) => pending.push((i, start + rule.budget)),
            // スレッドを作れなかった規則も、素通りにして、ほかを止めない。
            Err(_) => results[i] = Some(Verdict::Panicked),
        }
    }
    drop(tx);

    while let Some(next) = pending.iter().map(|(_, d)| *d).min() {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok((i, verdict)) => {
                // 時間切れにした規則の、遅れて届いた結果は採らない。
                if let Some((_, deadline)) = pending.iter().find(|(j, _)| *j == i) {
                    results[i] = Some(if Instant::now() <= *deadline {
                        verdict
                    } else {
                        Verdict::TimedOut
                    });
                    pending.retain(|(j, _)| *j != i);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                pending.retain(|(j, deadline)| {
                    if *deadline <= now {
                        results[*j] = Some(Verdict::TimedOut);
                        false
                    } else {
                        true
                    }
                });
            }
            // 送らずに終わったスレッド(起こらないはず)は、素通りにする。
            Err(RecvTimeoutError::Disconnected) => {
                for (j, _) in pending.drain(..) {
                    results[j] = Some(Verdict::Panicked);
                }
            }
        }
    }

    inputs
        .iter()
        .zip(results)
        .map(|((r, _), v)| (r.node, v.unwrap_or(Verdict::TimedOut)))
        .collect()
}

/// 結果をまとめたもの。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Merged {
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    pub allow: Vec<String>,
    pub context: Vec<String>,
    /// 素通りにした規則(node, 理由)。
    pub failed_open: Vec<(&'static str, &'static str)>,
}

pub fn merge(results: &[(&'static str, Verdict)]) -> Merged {
    let mut m = Merged::default();
    for (node, verdict) in results {
        match verdict {
            Verdict::Done(Some(Outcome::Deny(reason))) => m.deny.push(reason.clone()),
            Verdict::Done(Some(Outcome::Ask(reason))) => m.ask.push(reason.clone()),
            Verdict::Done(Some(Outcome::Allow(reason))) => m.allow.push(reason.clone()),
            Verdict::Done(Some(Outcome::Context(text))) => m.context.push(text.clone()),
            Verdict::Done(None) => {}
            Verdict::Panicked | Verdict::TimedOut if *node == "agent-permissions" => {
                let why = if matches!(verdict, Verdict::Panicked) {
                    "panic"
                } else {
                    "時間切れ"
                };
                m.deny.push(format!("[agent-permissions] 権限検査が{why}で完了しなかったため拒否しました。agent-permissions inspect で診断してください。"));
            }
            Verdict::Panicked => m.failed_open.push((node, "panic")),
            Verdict::TimedOut => m.failed_open.push((node, "時間切れ")),
        }
    }
    m
}

/// Claude Code に返す 1 行の JSON。何も出すものが無ければ `None`。
pub fn render(event: Event, m: &Merged) -> Option<String> {
    let mut specific = Map::new();
    specific.insert("hookEventName".into(), json!(event.name()));
    let mut any = false;
    if event == Event::Pre {
        for (decision, reasons) in [("deny", &m.deny), ("ask", &m.ask), ("allow", &m.allow)] {
            if !reasons.is_empty() {
                specific.insert("permissionDecision".into(), json!(decision));
                specific.insert(
                    "permissionDecisionReason".into(),
                    json!(reasons.join("\n\n")),
                );
                any = true;
                break;
            }
        }
    }
    if !m.context.is_empty() {
        specific.insert("additionalContext".into(), json!(m.context.join("\n")));
        any = true;
    }

    let mut out = Map::new();
    if any {
        out.insert("hookSpecificOutput".into(), Value::Object(specific));
    }
    if !m.failed_open.is_empty() {
        let list: Vec<String> = m
            .failed_open
            .iter()
            .map(|(node, why)| format!("{node}({why})"))
            .collect();
        out.insert(
            "systemMessage".into(),
            json!(format!(
                "[hook-router] 次の検査を素通りにしました(検査が panic したか、時間を越えました): {}",
                list.join(", ")
            )),
        );
    }
    (!out.is_empty()).then(|| Value::Object(out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Tools;
    use std::time::Duration;

    fn rule(node: &'static str, ms: u64, run: fn(Agent, &str) -> Option<Outcome>) -> Rule {
        Rule {
            node,
            event: Event::Pre,
            tools: Tools::Bash,
            budget: Duration::from_millis(ms),
            run,
        }
    }

    fn pass(_: Agent, _: &str) -> Option<Outcome> {
        None
    }
    fn deny_a(_: Agent, _: &str) -> Option<Outcome> {
        Outcome::deny("理由 A")
    }
    fn deny_b(_: Agent, _: &str) -> Option<Outcome> {
        Outcome::deny("理由 B")
    }
    fn ctx(_: Agent, _: &str) -> Option<Outcome> {
        Outcome::context("文脈")
    }
    fn boom(_: Agent, _: &str) -> Option<Outcome> {
        panic!("検査が壊れた");
    }
    fn slow(_: Agent, _: &str) -> Option<Outcome> {
        std::thread::sleep(Duration::from_millis(1500));
        Outcome::deny("遅れて届いた理由")
    }
    fn echo_len(_: Agent, raw: &str) -> Option<Outcome> {
        Outcome::context(format!("len={}", raw.len()))
    }

    #[test]
    fn results_keep_the_order_of_the_table() {
        let rules = [
            rule("a", 1000, deny_a),
            rule("p", 1000, pass),
            rule("b", 1000, deny_b),
        ];
        let r = execute(&rules, "{}");
        assert_eq!(
            r.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            ["a", "p", "b"]
        );
        let m = merge(&r);
        assert_eq!(m.deny, ["理由 A", "理由 B"]);
    }

    #[test]
    fn contexts_are_joined_in_the_order_of_the_table() {
        let rules = [rule("c", 1000, ctx), rule("e", 1000, echo_len)];
        let m = merge(&execute(&rules, "{}"));
        assert_eq!(m.context, ["文脈", "len=2"]);
    }

    #[test]
    fn every_rule_gets_the_raw_input() {
        let r = execute(&[rule("e", 1000, echo_len)], "{\"x\":1}");
        assert_eq!(
            merge(&r).context,
            ["len=7"],
            "規則には、標準入力の生の JSON がそのまま渡る"
        );
    }

    #[test]
    fn original_and_normalized_checks_receive_their_own_payloads() {
        let checks = [
            (rule("agent-permissions", 1000, echo_len), "original".into()),
            (rule("normalized", 1000, echo_len), "patch-path".into()),
        ];
        assert_eq!(
            merge(&execute_inputs_for(Agent::Codex, &checks)).context,
            ["len=8", "len=10"]
        );
    }

    #[test]
    fn a_panicking_rule_does_not_stop_the_others() {
        let rules = [rule("boom", 1000, boom), rule("a", 1000, deny_a)];
        let m = merge(&execute(&rules, "{}"));
        assert_eq!(m.deny, ["理由 A"], "ほかの規則の結果は残る");
        assert_eq!(m.failed_open, [("boom", "panic")]);
    }

    #[test]
    fn a_rule_over_budget_is_failed_open_and_its_late_result_is_dropped() {
        let rules = [rule("slow", 100, slow), rule("a", 1000, deny_a)];
        let t = Instant::now();
        let m = merge(&execute(&rules, "{}"));
        assert!(
            t.elapsed() < Duration::from_millis(1200),
            "遅い規則を待たない: {:?}",
            t.elapsed()
        );
        assert_eq!(m.deny, ["理由 A"]);
        assert_eq!(m.failed_open, [("slow", "時間切れ")]);
    }

    #[test]
    fn each_rule_has_its_own_budget() {
        // 短い budget の遅い規則だけが切られ、長い budget の規則は待たれる。
        fn medium(_: Agent, _: &str) -> Option<Outcome> {
            std::thread::sleep(Duration::from_millis(300));
            Outcome::deny("300ms かかった理由")
        }
        let rules = [rule("short", 100, slow), rule("long", 2000, medium)];
        let m = merge(&execute(&rules, "{}"));
        assert_eq!(m.deny, ["300ms かかった理由"]);
        assert_eq!(m.failed_open, [("short", "時間切れ")]);
    }

    #[test]
    fn no_rules_means_no_output() {
        assert!(execute(&[], "{}").is_empty());
        assert_eq!(render(Event::Pre, &merge(&[])), None);
    }

    #[test]
    fn pre_deny_lists_every_reason_in_order() {
        let m = Merged {
            deny: vec!["A".into(), "B".into()],
            ..Default::default()
        };
        let v: Value = serde_json::from_str(&render(Event::Pre, &m).unwrap()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecisionReason"],
            "A\n\nB"
        );
        assert!(v.get("systemMessage").is_none());
    }

    #[test]
    fn context_without_deny_is_additional_context_only() {
        let m = Merged {
            context: vec!["x".into(), "y".into()],
            ..Default::default()
        };
        let v: Value = serde_json::from_str(&render(Event::Pre, &m).unwrap()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "x\ny");
        assert!(v["hookSpecificOutput"].get("permissionDecision").is_none());
    }

    #[test]
    fn post_never_emits_a_decision() {
        let m = Merged {
            deny: vec!["A".into()],
            context: vec!["c".into()],
            ..Default::default()
        };
        let v: Value = serde_json::from_str(&render(Event::Post, &m).unwrap()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert!(v["hookSpecificOutput"].get("permissionDecision").is_none());
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "c");
    }

    #[test]
    fn failed_open_rules_are_announced_with_a_system_message() {
        let m = Merged {
            failed_open: vec![("pkexec-guard", "panic"), ("stack-base-guard", "時間切れ")],
            ..Default::default()
        };
        let v: Value = serde_json::from_str(&render(Event::Pre, &m).unwrap()).unwrap();
        assert!(v.get("hookSpecificOutput").is_none());
        let msg = v["systemMessage"].as_str().unwrap();
        assert!(msg.contains("pkexec-guard(panic)") && msg.contains("stack-base-guard(時間切れ)"));
    }

    #[test]
    fn decision_priority_is_deny_then_ask_then_allow() {
        let outcomes = [
            (
                "allow",
                Verdict::Done(Some(Outcome::Allow("allowed".into()))),
            ),
            ("ask", Verdict::Done(Some(Outcome::Ask("one time".into())))),
            ("deny", Verdict::Done(Some(Outcome::Deny("blocked".into())))),
        ];
        for (len, want) in [(1, "allow"), (2, "ask"), (3, "deny")] {
            let merged = merge(&outcomes[..len]);
            let value: Value = serde_json::from_str(&render(Event::Pre, &merged).unwrap()).unwrap();
            assert_eq!(value["hookSpecificOutput"]["permissionDecision"], want);
            // The result is independent of rule ordering.
            let reversed: Vec<_> = outcomes[..len].iter().rev().cloned().collect();
            assert_eq!(
                render(Event::Pre, &merged),
                render(Event::Pre, &merge(&reversed))
            );
        }
    }

    #[test]
    fn permission_check_panic_and_timeout_fail_closed_only_for_that_rule() {
        for run in [boom as fn(Agent, &str) -> Option<Outcome>, slow] {
            let checks = [rule("agent-permissions", 20, run), rule("other", 20, run)];
            let merged = merge(&execute(&checks, "{}"));
            assert_eq!(merged.deny.len(), 1);
            assert!(merged.deny[0].contains("agent-permissions"));
            assert_eq!(merged.failed_open.len(), 1);
            assert_eq!(merged.failed_open[0].0, "other");
            let value: Value = serde_json::from_str(&render(Event::Pre, &merged).unwrap()).unwrap();
            assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        }
    }
}
