//! promotion-detect — Q2(LLM/散文 → 決定論への昇格)の昇格・降格候補を
//! 検出し、wrap-up inbox へ 1 候補 1 行で流す日次バッチ(ADR-543
//! 「既存手段の前倒し接地と、決定論への昇格導線」段4)。
//!
//! 判定ロジックはすべてこのモジュールに閉じる(純粋関数 + `#[test]`)。
//! `main.rs` は `gh`/ファイル探索・`wrapup-stop-gate` の起動だけを担う。
//!
//! 兆候は3種類(ADR-543 D4・D6):
//! - 再発: 同じ `Target:` を持つ feedback Issue が [`RECURRENCE_THRESHOLD`]
//!   件以上 → 規範・skill の gate 化を検討。
//! - 逐語反復: 同じ正規化コマンドが [`VERBATIM_MIN_SESSIONS`] セッション
//!   以上で実行され、かつ SKILL.md のコードブロックと一致する → スクリプト化
//!   を検討。
//! - 降格: gate の skip ファイルが [`SKIP_STALE_DAYS`] 日以上有効なまま、
//!   または gate-events.jsonl の skip 回数が [`DEMOTION_SKIP_THRESHOLD`]
//!   件以上 → 見直しを検討。

use std::collections::{HashMap, HashSet};

pub const RECURRENCE_THRESHOLD: usize = 2;
pub const VERBATIM_MIN_SESSIONS: usize = 3;
pub const SKIP_STALE_DAYS: u64 = 30;
pub const DEMOTION_SKIP_THRESHOLD: usize = 3;

// ---------------------------------------------------------------------------
// 再発(feedback Issue の Target: 集計)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackIssue {
    pub number: u64,
    pub target: Option<String>,
}

/// `feedback-target-guard.sh` の `find_target_value` と同じ切り出し規則
/// (最初の `Target: <値>` 行の値だけを取る)。
pub fn parse_target(body: &str) -> Option<String> {
    for line in body.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("Target:") {
            if let Some(value) = rest.split_whitespace().next() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecurrenceCandidate {
    pub target: String,
    pub numbers: Vec<u64>,
}

/// 同じ `Target:` を持つ feedback Issue が `threshold` 件以上のものを返す
/// (`target` の昇順)。
pub fn detect_recurrence(issues: &[FeedbackIssue], threshold: usize) -> Vec<RecurrenceCandidate> {
    let mut by_target: HashMap<&str, Vec<u64>> = HashMap::new();
    for issue in issues {
        if let Some(t) = &issue.target {
            by_target.entry(t.as_str()).or_default().push(issue.number);
        }
    }
    let mut out: Vec<RecurrenceCandidate> = by_target
        .into_iter()
        .filter(|(_, nums)| nums.len() >= threshold)
        .map(|(target, mut numbers)| {
            numbers.sort_unstable();
            RecurrenceCandidate {
                target: target.to_string(),
                numbers,
            }
        })
        .collect();
    out.sort_by(|a, b| a.target.cmp(&b.target));
    out
}

// ---------------------------------------------------------------------------
// 逐語反復(cmd-hashes.jsonl × SKILL.md コードブロック)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CmdHashRecord {
    pub session_id: String,
    pub hash: String,
}

/// hash → 出現した distinct session_id の集合。`min_sessions` 件以上の
/// hash だけを返す(hash → session 数)。
pub fn detect_verbatim_hashes(
    records: &[CmdHashRecord],
    min_sessions: usize,
) -> HashMap<String, usize> {
    let mut sessions_by_hash: HashMap<&str, HashSet<&str>> = HashMap::new();
    for r in records {
        sessions_by_hash
            .entry(r.hash.as_str())
            .or_default()
            .insert(r.session_id.as_str());
    }
    sessions_by_hash
        .into_iter()
        .filter(|(_, s)| s.len() >= min_sessions)
        .map(|(h, s)| (h.to_string(), s.len()))
        .collect()
}

/// ```` ```bash ```` / ```` ```sh ```` フェンスの中身をそのまま抜き出す
/// (言語タグ無しの ```` ``` ```` は対象外 — 例示以外の一般散文を誤検出
/// しないため)。
pub fn extract_shell_code_blocks(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = markdown.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let is_open = trimmed == "```bash" || trimmed == "```sh";
        if !is_open {
            continue;
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            if inner.trim_start().starts_with("```") {
                break;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(inner);
        }
        out.push(body);
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerbatimCandidate {
    pub skill: String,
    pub session_count: usize,
}

/// skill 名 → コードブロック本文一覧、を受け取り、逐語反復と判定された
/// ブロックを持つ skill を返す(`skill` の昇順、1 skill につき最大1件)。
/// プレースホルダ(`<...>`)を含むブロックは照合対象外。
pub fn match_skill_blocks(
    skill_blocks: &HashMap<String, Vec<String>>,
    repeated_hashes: &HashMap<String, usize>,
) -> Vec<VerbatimCandidate> {
    let mut out = Vec::new();
    for (skill, blocks) in skill_blocks {
        for block in blocks {
            let normalized = command_inspection::cmd_hash::normalize(block);
            if normalized.is_empty() || command_inspection::cmd_hash::has_placeholder(&normalized) {
                continue;
            }
            let h = command_inspection::cmd_hash::fnv1a_hex(&normalized);
            if let Some(&count) = repeated_hashes.get(&h) {
                out.push(VerbatimCandidate {
                    skill: skill.clone(),
                    session_count: count,
                });
                break;
            }
        }
    }
    out.sort_by(|a, b| a.skill.cmp(&b.skill));
    out
}

// ---------------------------------------------------------------------------
// 降格(skip ファイルの滞留 / gate-events.jsonl の skip 多発)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct StaleSkip {
    pub gate: String,
    pub age_days: u64,
}

/// `entries`: (gate名, skip ファイルの経過秒数)。`threshold_days` 日以上
/// 経過しているものだけを返す(`gate` の昇順)。
pub fn detect_stale_skips(entries: &[(String, u64)], threshold_days: u64) -> Vec<StaleSkip> {
    let mut out: Vec<StaleSkip> = entries
        .iter()
        .filter(|(_, age_secs)| age_secs / 86400 >= threshold_days)
        .map(|(gate, age_secs)| StaleSkip {
            gate: gate.clone(),
            age_days: age_secs / 86400,
        })
        .collect();
    out.sort_by(|a, b| a.gate.cmp(&b.gate));
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateEventRecord {
    pub gate: String,
    pub decision: String, // "deny" | "skip"
}

#[derive(Debug, Clone, PartialEq)]
pub struct DemotionCandidate {
    pub gate: String,
    pub deny_count: usize,
    pub skip_count: usize,
}

/// skip 回数が `threshold` 件以上の gate を返す(`gate` の昇順)。
pub fn detect_demotion_candidates(
    events: &[GateEventRecord],
    threshold: usize,
) -> Vec<DemotionCandidate> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for e in events {
        let c = counts.entry(e.gate.as_str()).or_default();
        match e.decision.as_str() {
            "deny" => c.0 += 1,
            "skip" => c.1 += 1,
            _ => {}
        }
    }
    let mut out: Vec<DemotionCandidate> = counts
        .into_iter()
        .filter(|(_, (_, skip))| *skip >= threshold)
        .map(|(gate, (deny_count, skip_count))| DemotionCandidate {
            gate: gate.to_string(),
            deny_count,
            skip_count,
        })
        .collect();
    out.sort_by(|a, b| a.gate.cmp(&b.gate));
    out
}

// ---------------------------------------------------------------------------
// wrap-up inbox のパス(wrapup-stop-gate `normalize_remote_url` と同じ
// 正規化規則。owner/repo は github.com 固定でよい — このバイナリは
// tarotene/dotfiles 自身の inbox にしか書かないため)。
// ---------------------------------------------------------------------------

/// `owner/repo` → `github-com-owner-repo`(`wrapup-stop-gate` の
/// `normalize_remote_url` と同じ規則: 小文字化 → `/`・`.` を `-` に置換)。
pub fn repo_slug(nwo: &str) -> String {
    format!("github.com/{nwo}")
        .to_lowercase()
        .replace(['/', '.'], "-")
}

// ---------------------------------------------------------------------------
// wrap-up inbox 向けの文面組み立て
// ---------------------------------------------------------------------------

/// `wrapup-stop-gate --add` に渡す1行JSON。`repo` を必ず付ける: timer から動くときは作業ディレクトリが
/// `$HOME` で、`--add` が repo を取り違える(jsonl のファイル名由来の値になる)。
pub fn inbox_line(title: &str, detail: &str, repo: &str) -> String {
    serde_json::json!({ "title": title, "detail": detail, "repo": repo }).to_string()
}

pub fn recurrence_title(c: &RecurrenceCandidate) -> String {
    format!(
        "同じ Target: {} を持つ feedback Issue が{}件あります(昇格候補、ADR-543)",
        c.target,
        c.numbers.len()
    )
}

pub fn recurrence_detail(c: &RecurrenceCandidate) -> String {
    let nums: Vec<String> = c.numbers.iter().map(|n| format!("#{n}")).collect();
    format!(
        "対象: {}。同じ規範・skill・hook への feedback が繰り返し起票されています。決定論的な gate への昇格を検討してください(ADR-543 段4)。",
        nums.join(", ")
    )
}

pub fn verbatim_title(c: &VerbatimCandidate) -> String {
    format!(
        "skill {} のコードブロックが{}セッションで逐語反復実行されています(昇格候補、ADR-543)",
        c.skill, c.session_count
    )
}

pub fn verbatim_detail(c: &VerbatimCandidate) -> String {
    format!(
        "config/agents/skills/{}/SKILL.md のコードブロックが改変なしに繰り返し実行されています。決定論的なスクリプトへの昇格を検討してください(散文はフォールバック手順として残す、ADR-543 段4)。",
        c.skill
    )
}

pub fn stale_skip_title(c: &StaleSkip) -> String {
    format!(
        "gate {} の skip が {}日前から有効なままです(降格候補、ADR-543)",
        c.gate, c.age_days
    )
}

pub fn stale_skip_detail(c: &StaleSkip) -> String {
    format!(
        "{} の skip ファイルが長期間存在しています。gate が実情に合わなくなっていないか見直してください(ADR-543 段4)。",
        c.gate
    )
}

pub fn demotion_title(c: &DemotionCandidate) -> String {
    format!(
        "gate {} の skip が{}件記録されています(降格候補、ADR-543)",
        c.gate, c.skip_count
    )
}

pub fn demotion_detail(c: &DemotionCandidate) -> String {
    format!(
        "gate-events.jsonl によると gate {} は deny {}件・skip {}件です。skip が多用されていないか見直してください(ADR-543 段4)。",
        c.gate, c.deny_count, c.skip_count
    )
}

// ---------------------------------------------------------------------------
// 語彙の較正(兆候ごとの採用・却下。ADR-1276)
// ---------------------------------------------------------------------------

use issue_inbox::codify::Vocabulary;

/// Inbox のノートのうち、as-code 化の候補(`signal` を持つもの)の、較正に要る欄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalNote {
    pub signal: String,
    pub status: String,
    pub drop_reason: String,
    pub first_seen: String,
}

fn adjudicated(status: &str) -> bool {
    matches!(status, "fixed" | "issued" | "dropped")
}

fn reasons_text(notes: &[&SignalNote]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for n in notes.iter().filter(|n| !n.drop_reason.is_empty()) {
        match counts.iter_mut().find(|(r, _)| *r == n.drop_reason) {
            Some((_, c)) => *c += 1,
            None => counts.push((&n.drop_reason, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    if counts.is_empty() {
        return "理由なし".to_string();
    }
    counts
        .iter()
        .map(|(r, c)| format!("{r}×{c}"))
        .collect::<Vec<_>>()
        .join("、")
}

/// 兆候ごとの採用・却下を数え、語彙を直す候補を `(題, 本文)` で返す。しきい値は語彙の `calibration`。
///
/// - 却下が続く兆候: 却下 `min_dropped` 件以上かつ却下率 `drop_ratio` 以上、または `misread-signal` が
///   `misread_min` 件以上 → 兆候の定義(`look_for`・既定の段)を直す候補。
/// - `unclassified` が `unclassified_min` 件以上 → 語彙に足す候補。
/// - `existing-suffices` の却下が全兆候で `existing_suffices_min` 件以上 → 既存手段の探索が浅い。
///
/// 兆候の `since` より前に初出のノートは数えない(定義を直したら数え直す)。語彙に無い兆候は無視する。
/// 本文には件数と理由の id だけを書く(ノートの中身は書かない)。
pub fn calibrate(notes: &[SignalNote], vocab: &Vocabulary) -> Vec<(String, String)> {
    let cal = &vocab.calibration;
    let (num, den) = (cal.drop_ratio.0 as usize, cal.drop_ratio.1 as usize);
    let mut out = Vec::new();
    let mut suffices = 0usize;
    for sig in &vocab.signals {
        let mine: Vec<&SignalNote> = notes
            .iter()
            .filter(|n| n.signal == sig.id && n.first_seen.as_str() >= sig.since.as_str())
            .collect();
        if sig.id == "unclassified" {
            if mine.len() >= cal.unclassified_min as usize {
                out.push((
                    "語彙に足す兆候の候補: どれにも入らない気づきが溜まっています(ADR-1276)".to_string(),
                    format!(
                        "兆候 `unclassified`(どれにも入らない)の候補が {} 件あります。同じ種類のものがあれば、\
                         `config/agents/codify-signals/vocabulary.json` に兆候として足す。",
                        mine.len()
                    ),
                ));
            }
            continue;
        }
        let done: Vec<&SignalNote> = mine
            .iter()
            .copied()
            .filter(|n| adjudicated(&n.status))
            .collect();
        let dropped: Vec<&SignalNote> = done
            .iter()
            .copied()
            .filter(|n| n.status == "dropped")
            .collect();
        suffices += dropped
            .iter()
            .filter(|n| n.drop_reason == "existing-suffices")
            .count();
        let misread = dropped
            .iter()
            .filter(|n| n.drop_reason == "misread-signal")
            .count();
        let ratio_ok = !done.is_empty() && dropped.len() * den >= num * done.len();
        if (dropped.len() >= cal.min_dropped as usize && ratio_ok)
            || misread >= cal.misread_min as usize
        {
            out.push((
                format!("兆候 {} の定義を直す候補: 却下が続いています(ADR-1276)", sig.id),
                format!(
                    "兆候 `{}`(既定の段 `{}`)の候補 {} 件のうち、{} 件が却下、{} 件が採用されました(理由: {})。\
                     `config/agents/codify-signals/vocabulary.json` の `look_for` と既定の段を見直す。\
                     直したら `since` を更新すると、数え直します。",
                    sig.id,
                    sig.default_level,
                    done.len(),
                    dropped.len(),
                    done.len() - dropped.len(),
                    reasons_text(&dropped)
                ),
            ));
        }
    }
    if suffices >= cal.existing_suffices_min as usize {
        out.push((
            "既存手段の探索が浅い候補: 「既存で足りた」による却下が続いています(ADR-1276)".to_string(),
            format!(
                "`existing-suffices` による却下が {suffices} 件あります。retro の `searched`(リポジトリ内の探索)が浅い。\
                 手順書の探索の案内を見直す。"
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_takes_first_value() {
        assert_eq!(
            parse_target("本文\nTarget: skill/precedent-grounding\n続き"),
            Some("skill/precedent-grounding".to_string())
        );
        assert_eq!(parse_target("本文だけ"), None);
        assert_eq!(
            parse_target("  Target: hook/foo  \n他"),
            Some("hook/foo".to_string())
        );
    }

    #[test]
    fn detect_recurrence_groups_by_target() {
        let issues = vec![
            FeedbackIssue {
                number: 10,
                target: Some("skill/x".to_string()),
            },
            FeedbackIssue {
                number: 20,
                target: Some("skill/x".to_string()),
            },
            FeedbackIssue {
                number: 30,
                target: Some("skill/y".to_string()),
            },
            FeedbackIssue {
                number: 40,
                target: None,
            },
        ];
        let out = detect_recurrence(&issues, 2);
        assert_eq!(
            out,
            vec![RecurrenceCandidate {
                target: "skill/x".to_string(),
                numbers: vec![10, 20],
            }]
        );
    }

    #[test]
    fn detect_verbatim_hashes_counts_distinct_sessions() {
        let records = vec![
            CmdHashRecord {
                session_id: "s1".into(),
                hash: "h1".into(),
            },
            CmdHashRecord {
                session_id: "s2".into(),
                hash: "h1".into(),
            },
            CmdHashRecord {
                session_id: "s1".into(),
                hash: "h1".into(),
            }, // 同一セッションの重複は1回に数える
            CmdHashRecord {
                session_id: "s3".into(),
                hash: "h1".into(),
            },
            CmdHashRecord {
                session_id: "s1".into(),
                hash: "h2".into(),
            },
        ];
        let out = detect_verbatim_hashes(&records, 3);
        assert_eq!(out.len(), 1);
        assert_eq!(out.get("h1"), Some(&3));
    }

    #[test]
    fn extract_shell_code_blocks_only_bash_and_sh() {
        let md = "文\n```bash\necho a\n```\n```text\necho b\n```\n```sh\necho c\n```\n";
        let out = extract_shell_code_blocks(md);
        assert_eq!(out, vec!["echo a".to_string(), "echo c".to_string()]);
    }

    #[test]
    fn match_skill_blocks_skips_placeholders() {
        let mut blocks = HashMap::new();
        blocks.insert("s1".to_string(), vec!["real command --flag".to_string()]);
        blocks.insert("s2".to_string(), vec!["cmd <path>".to_string()]);
        let h = command_inspection::cmd_hash::hash("real command --flag");
        let mut repeated = HashMap::new();
        repeated.insert(h, 3);
        let out = match_skill_blocks(&blocks, &repeated);
        assert_eq!(
            out,
            vec![VerbatimCandidate {
                skill: "s1".to_string(),
                session_count: 3,
            }]
        );
    }

    #[test]
    fn detect_stale_skips_filters_by_threshold() {
        let entries = vec![
            ("gate-a".to_string(), 40 * 86400),
            ("gate-b".to_string(), 5 * 86400),
        ];
        let out = detect_stale_skips(&entries, 30);
        assert_eq!(
            out,
            vec![StaleSkip {
                gate: "gate-a".to_string(),
                age_days: 40,
            }]
        );
    }

    #[test]
    fn detect_demotion_candidates_filters_by_skip_count() {
        let events = vec![
            GateEventRecord {
                gate: "g1".into(),
                decision: "deny".into(),
            },
            GateEventRecord {
                gate: "g1".into(),
                decision: "skip".into(),
            },
            GateEventRecord {
                gate: "g1".into(),
                decision: "skip".into(),
            },
            GateEventRecord {
                gate: "g1".into(),
                decision: "skip".into(),
            },
            GateEventRecord {
                gate: "g2".into(),
                decision: "skip".into(),
            },
        ];
        let out = detect_demotion_candidates(&events, 3);
        assert_eq!(
            out,
            vec![DemotionCandidate {
                gate: "g1".to_string(),
                deny_count: 1,
                skip_count: 3,
            }]
        );
    }

    #[test]
    fn repo_slug_matches_wrapup_stop_gate_convention() {
        assert_eq!(
            repo_slug("tarotene/dotfiles"),
            "github-com-tarotene-dotfiles"
        );
        assert_eq!(repo_slug("Owner/Repo.Name"), "github-com-owner-repo-name");
    }

    #[test]
    fn inbox_line_is_valid_json() {
        let line = inbox_line("t", "d", "o/r");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["title"], "t");
        assert_eq!(v["detail"], "d");
        // timer から動くとき、`--add` が repo を取り違えないように、行に repo を持たせる。
        assert_eq!(v["repo"], "o/r");
    }

    fn note(signal: &str, status: &str, reason: &str, first_seen: &str) -> SignalNote {
        SignalNote {
            signal: signal.into(),
            status: status.into(),
            drop_reason: reason.into(),
            first_seen: first_seen.into(),
        }
    }

    fn many(n: usize, signal: &str, status: &str, reason: &str) -> Vec<SignalNote> {
        (0..n)
            .map(|_| note(signal, status, reason, "2026-11-01"))
            .collect()
    }

    fn titles(v: &[(String, String)]) -> Vec<&str> {
        v.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn a_signal_dropped_often_is_flagged_but_the_floor_and_ratio_hold() {
        let vocab = Vocabulary::get();
        // 却下 2 件(下限 3 に届かない)。
        let mut notes = many(2, "form-check", "dropped", "one-off");
        assert!(calibrate(&notes, vocab).is_empty());
        // 却下 3 件、採用 0 件 → 却下率 100%。
        notes.extend(many(1, "form-check", "dropped", "preference"));
        let found = calibrate(&notes, vocab);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("form-check"), "{found:?}");
        assert!(found[0].1.contains("one-off×2"), "{found:?}");
        // 採用が増えて却下率が 2/3 を割ると、旗は下りる(却下 3、採用 2 → 60%)。
        notes.extend(many(2, "form-check", "fixed", ""));
        assert!(calibrate(&notes, vocab).is_empty());
        // 却下 4、採用 2 → ちょうど 2/3 → 旗が立つ(境界は含む)。
        notes.extend(many(1, "form-check", "dropped", "one-off"));
        assert_eq!(calibrate(&notes, vocab).len(), 1);
    }

    #[test]
    fn open_notes_are_not_counted_as_adjudicated() {
        let vocab = Vocabulary::get();
        let mut notes = many(3, "form-check", "dropped", "one-off");
        notes.extend(many(10, "form-check", "open", ""));
        assert_eq!(calibrate(&notes, vocab).len(), 1);
    }

    #[test]
    fn two_misreads_flag_a_signal_even_below_the_floor() {
        let vocab = Vocabulary::get();
        let notes = many(2, "copy-drift", "dropped", "misread-signal");
        let found = calibrate(&notes, vocab);
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("misread-signal×2"), "{found:?}");
        // 1 件では足りない。
        assert!(calibrate(&many(1, "copy-drift", "dropped", "misread-signal"), vocab).is_empty());
    }

    #[test]
    fn notes_before_the_signals_since_date_are_not_counted() {
        let vocab = Vocabulary::get();
        // since は 2026-10-30。定義を直す前のノートは数えない。
        let old: Vec<SignalNote> = (0..5)
            .map(|_| note("form-check", "dropped", "one-off", "2026-10-29"))
            .collect();
        assert!(calibrate(&old, vocab).is_empty());
    }

    #[test]
    fn unclassified_piling_up_suggests_a_new_signal() {
        let vocab = Vocabulary::get();
        assert!(calibrate(&many(4, "unclassified", "open", ""), vocab).is_empty());
        let found = calibrate(&many(5, "unclassified", "open", ""), vocab);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("語彙に足す"), "{found:?}");
    }

    #[test]
    fn existing_suffices_across_signals_points_at_the_search() {
        let vocab = Vocabulary::get();
        let mut notes = many(1, "form-check", "dropped", "existing-suffices");
        notes.extend(many(1, "copy-drift", "dropped", "existing-suffices"));
        assert!(calibrate(&notes, vocab).is_empty());
        notes.extend(many(1, "guard-misfire", "dropped", "existing-suffices"));
        let found = calibrate(&notes, vocab);
        assert_eq!(titles(&found).len(), 1, "{found:?}");
        assert!(found[0].0.contains("探索が浅い"), "{found:?}");
    }

    #[test]
    fn unknown_signals_are_ignored_and_details_carry_no_note_text() {
        let vocab = Vocabulary::get();
        assert!(calibrate(&many(9, "vibes", "dropped", "one-off"), vocab).is_empty());
        let found = calibrate(&many(3, "form-check", "dropped", "one-off"), vocab);
        // 件数と理由の id と語彙のパスだけ。
        assert!(found[0].1.chars().count() < 400, "{found:?}");
    }
}
