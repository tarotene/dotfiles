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
            let normalized = hook_io::cmd_hash::normalize(block);
            if normalized.is_empty() || hook_io::cmd_hash::has_placeholder(&normalized) {
                continue;
            }
            let h = hook_io::cmd_hash::fnv1a_hex(&normalized);
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

/// `wrapup-stop-gate --add` に渡す1行JSON。
pub fn inbox_line(title: &str, detail: &str) -> String {
    serde_json::json!({ "title": title, "detail": detail }).to_string()
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
        "config/claude/skills/{}/SKILL.md のコードブロックが改変なしに繰り返し実行されています。決定論的なスクリプトへの昇格を検討してください(散文はフォールバック手順として残す、ADR-543 段4)。",
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
        let h = hook_io::cmd_hash::hash("real command --flag");
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
        let line = inbox_line("t", "d");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["title"], "t");
        assert_eq!(v["detail"], "d");
    }
}
