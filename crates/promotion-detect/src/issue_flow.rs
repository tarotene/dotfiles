//! Issue の流量の検出(ADR-1529「Issue に置くのは、マージ後も残る状態を持ち、ほかに置き場が無いものだけ」)。
//!
//! 共有の AGENTS.md「Issue に置くのは…」の節は、起票の前に置き場を選ぶ順を決めた。この順から外れた起票を、
//! 日次で数えて wrap-up inbox に流す(ADR-543 の昇格型の「検出」の段。再発が続けば、作成時の拒否へ昇格を検討する)。
//!
//! 数えるのは 3 つ:
//! - PR の影: PR で閉じ、起票から [`SHADOW_MAX_LIFETIME_SECS`] 以内に閉じ、コメントが 0 件の Issue。
//!   PR 本文(`No-Issue:`)で足りた起票で、直近 [`WINDOW_DAYS`] 日に [`SHADOW_THRESHOLD`] 件以上あれば候補にする。
//! - 台帳の外の実機確認: 直近 [`WINDOW_DAYS`] 日に、確認の台帳(題が [`LEDGER_TITLE_PREFIX`] で始まる Issue)の外に
//!   起票された実機確認の Issue。1 件でもあれば候補にする。
//! - open の件数: bot の起票を除く open の Issue が [`OPEN_THRESHOLD`] 件を超えたら候補にする。
//!
//! 判定はすべて純粋関数で、`gh` の呼び出しは `main.rs` が持つ。

/// 数える期間(日)。
pub const WINDOW_DAYS: i64 = 7;
/// PR の影とみなす寿命の上限(秒)。
pub const SHADOW_MAX_LIFETIME_SECS: i64 = 86_400;
/// PR の影を候補にする件数(ADR-543 の「繰り返し」。feedback の再発と同じ 2 件)。
pub const SHADOW_THRESHOLD: usize = 2;
/// open の件数の上限。超えたら候補にする。
pub const OPEN_THRESHOLD: usize = 30;
/// 確認の台帳の Issue の題の接頭辞(`verify-ledger: <host>`)。
pub const LEDGER_TITLE_PREFIX: &str = "verify-ledger:";
/// 詳細に並べる番号の最大件数。超えた分は「ほか N 件」と数だけを書く。
pub const LISTED: usize = 10;

/// 検出に要る Issue の欄。`gh issue list --json` の値から作る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowIssue {
    pub number: u64,
    pub title: String,
    /// 起票の unix 秒。
    pub created: i64,
    /// 閉じた unix 秒。open なら `None`。
    pub closed: Option<i64>,
    pub comments: usize,
    /// PR の `Closes` で閉じたか(`closedByPullRequestsReferences` が空でない)。
    pub closed_by_pr: bool,
    pub labels: Vec<String>,
    /// 起票者の login。bot は `app/` で始まる。
    pub author: String,
}

impl FlowIssue {
    fn is_bot(&self) -> bool {
        self.author.starts_with("app/")
    }

    fn in_window(&self, now: i64) -> bool {
        now - self.created <= WINDOW_DAYS * 86_400 && self.created <= now
    }
}

/// PR の影か: PR で閉じ、起票から 24 時間以内に閉じ、コメントが 0 件。
pub fn is_shadow(i: &FlowIssue) -> bool {
    match i.closed {
        Some(closed) => {
            i.closed_by_pr && i.comments == 0 && closed - i.created <= SHADOW_MAX_LIFETIME_SECS
        }
        None => false,
    }
}

/// 確認の台帳そのものか。
pub fn is_ledger(i: &FlowIssue) -> bool {
    i.title.starts_with(LEDGER_TITLE_PREFIX)
}

/// 実機確認の Issue か: 題が `human:`・`verify:`・`verify(` で始まるか、`handoff:human` のラベルを持つ。
/// 台帳そのものは含めない。
pub fn is_device_check(i: &FlowIssue) -> bool {
    if is_ledger(i) {
        return false;
    }
    let t = i.title.as_str();
    t.starts_with("human:")
        || t.starts_with("verify:")
        || t.starts_with("verify(")
        || i.labels.iter().any(|l| l == "handoff:human")
}

/// 候補 1 件(題・詳細)。題は日をまたいで変えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub detail: String,
}

/// `recent` は直近の起票(open と closed の両方)、`open` は open の全件。`now` は unix 秒。
pub fn detect(recent: &[FlowIssue], open: &[FlowIssue], now: i64) -> Vec<Finding> {
    let mut out = Vec::new();

    let shadows: Vec<u64> = sorted_numbers(
        recent
            .iter()
            .filter(|i| !i.is_bot() && i.in_window(now) && is_shadow(i)),
    );
    if shadows.len() >= SHADOW_THRESHOLD {
        out.push(Finding {
            title: shadow_title(),
            detail: shadow_detail(&shadows),
        });
    }

    let outside: Vec<u64> = sorted_numbers(
        recent
            .iter()
            .filter(|i| !i.is_bot() && i.in_window(now) && is_device_check(i)),
    );
    if !outside.is_empty() {
        out.push(Finding {
            title: outside_ledger_title(),
            detail: outside_ledger_detail(&outside),
        });
    }

    let mut humans: Vec<&FlowIssue> = open
        .iter()
        .filter(|i| !i.is_bot() && i.closed.is_none())
        .collect();
    if humans.len() > OPEN_THRESHOLD {
        humans.sort_by_key(|i| (i.created, i.number));
        let oldest: Vec<u64> = humans.iter().take(5).map(|i| i.number).collect();
        out.push(Finding {
            title: open_title(),
            detail: open_detail(humans.len(), &oldest),
        });
    }

    out
}

fn sorted_numbers<'a>(it: impl Iterator<Item = &'a FlowIssue>) -> Vec<u64> {
    let mut v: Vec<u64> = it.map(|i| i.number).collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn listed(numbers: &[u64]) -> String {
    let shown: Vec<String> = numbers
        .iter()
        .take(LISTED)
        .map(|n| format!("#{n}"))
        .collect();
    let rest = numbers.len().saturating_sub(LISTED);
    if rest > 0 {
        format!("{} ほか{rest}件", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

pub fn shadow_title() -> String {
    "Issue の流量: PR で 1 日以内に閉じた、コメントの無い Issue が続いています(ADR-1529)"
        .to_string()
}

pub fn shadow_detail(numbers: &[u64]) -> String {
    format!(
        "直近{WINDOW_DAYS}日に、PR の `Closes` で起票から 24 時間以内に閉じ、コメントが 0 件の Issue: {}。\
         PR で完結する仕事は `No-Issue:` で出し、`Closes #N` を書くためだけに直前の Issue を立てない\
         (共有 AGENTS.md「Issue に置くのは…」の置き場の順 2)。自分の計画の段を sub-issue に起こしていないかも確かめてください。\
         同じ型が続くなら、作成時の拒否(issue-create-guard)への昇格を検討します(ADR-543)。",
        listed(numbers)
    )
}

pub fn outside_ledger_title() -> String {
    "Issue の流量: 実機確認が、確認の台帳の外に起票されました(ADR-1529)".to_string()
}

pub fn outside_ledger_detail(numbers: &[u64]) -> String {
    format!(
        "直近{WINDOW_DAYS}日に、`{LEDGER_TITLE_PREFIX} <host>` の台帳の外に起票された実機確認の Issue: {}。\
         マージ後の実機確認は、機体ごとの台帳の本文に 1 行で追記し、新しく起票しない\
         (共有 AGENTS.md「実装タスクの完了定義」、pr-description §6)。台帳に移して、この Issue は閉じてください。",
        listed(numbers)
    )
}

pub fn open_title() -> String {
    format!("Issue の流量: open の Issue が {OPEN_THRESHOLD} 件を超えています(ADR-1529)")
}

/// 件数は 10 件単位に丸める(毎日変わる数が本文に入ると、同じ候補を毎日入れ直してしまう)。
pub fn open_detail(count: usize, oldest: &[u64]) -> String {
    let band = count / 10 * 10;
    format!(
        "bot の起票を除く open の Issue が {band} 件台です。古い順: {}。\
         `/my-cleanup` で、置き場の順(共有 AGENTS.md「Issue に置くのは…」)に照らして、\
         ほかの記録(ADR・docs・コードのコメント・確認の台帳・予定データ)へ移せるものを裁定してください。",
        listed(oldest)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn issue(number: u64, title: &str) -> FlowIssue {
        FlowIssue {
            number,
            title: title.to_string(),
            created: NOW - DAY,
            closed: None,
            comments: 0,
            closed_by_pr: false,
            labels: Vec::new(),
            author: "tarotene".to_string(),
        }
    }

    fn shadow(number: u64) -> FlowIssue {
        FlowIssue {
            closed: Some(NOW - DAY + 1800),
            closed_by_pr: true,
            ..issue(number, "fix: 何か")
        }
    }

    #[test]
    fn shadow_requires_pr_quick_close_and_no_comments() {
        assert!(is_shadow(&shadow(1)));
        assert!(!is_shadow(&FlowIssue {
            comments: 1,
            ..shadow(1)
        }));
        assert!(!is_shadow(&FlowIssue {
            closed_by_pr: false,
            ..shadow(1)
        }));
        assert!(!is_shadow(&issue(1, "open のまま")));
    }

    #[test]
    fn shadow_lifetime_boundary_is_24_hours_inclusive() {
        let at = |secs| FlowIssue {
            closed: Some(NOW - DAY + secs),
            ..shadow(1)
        };
        assert!(is_shadow(&at(SHADOW_MAX_LIFETIME_SECS)));
        assert!(!is_shadow(&at(SHADOW_MAX_LIFETIME_SECS + 1)));
    }

    #[test]
    fn one_shadow_is_not_a_candidate_two_are() {
        assert!(detect(&[shadow(1)], &[], NOW).is_empty());
        let found = detect(&[shadow(2), shadow(1)], &[], NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, shadow_title());
        assert!(found[0].detail.contains("#1, #2"), "{}", found[0].detail);
    }

    #[test]
    fn window_excludes_old_and_future_issues() {
        let old = FlowIssue {
            created: NOW - (WINDOW_DAYS * DAY) - 1,
            closed: Some(NOW - (WINDOW_DAYS * DAY) + 100),
            ..shadow(3)
        };
        assert!(detect(&[shadow(1), old], &[], NOW).is_empty());
        let edge = FlowIssue {
            created: NOW - WINDOW_DAYS * DAY,
            closed: Some(NOW - WINDOW_DAYS * DAY + 100),
            ..shadow(4)
        };
        assert_eq!(detect(&[shadow(1), edge], &[], NOW).len(), 1);
    }

    #[test]
    fn bots_are_ignored() {
        let bot = |n| FlowIssue {
            author: "app/renovate".to_string(),
            ..shadow(n)
        };
        assert!(detect(&[bot(1), bot(2)], &[], NOW).is_empty());
    }

    #[test]
    fn device_check_by_title_or_label_but_not_the_ledger() {
        assert!(is_device_check(&issue(1, "human: 実機で確かめる")));
        assert!(is_device_check(&issue(1, "verify: hms 後に読む")));
        assert!(is_device_check(&issue(1, "verify(hooks): 読む")));
        let labelled = FlowIssue {
            labels: vec!["handoff:human".to_string()],
            ..issue(1, "何かを確かめる")
        };
        assert!(is_device_check(&labelled));
        let ledger = FlowIssue {
            labels: vec!["handoff:human".to_string()],
            ..issue(1, "verify-ledger: vega")
        };
        assert!(!is_device_check(&ledger));
        assert!(!is_device_check(&issue(1, "feat: 何か")));
    }

    #[test]
    fn one_device_check_outside_the_ledger_is_a_candidate() {
        let found = detect(&[issue(7, "human: 確かめる")], &[], NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, outside_ledger_title());
        assert!(found[0].detail.contains("#7"));
    }

    #[test]
    fn open_count_over_threshold_lists_oldest_and_rounds_count() {
        let open: Vec<FlowIssue> = (1..=OPEN_THRESHOLD as u64 + 7)
            .map(|n| FlowIssue {
                created: NOW - n as i64 * DAY,
                ..issue(n, "feat: 何か")
            })
            .collect();
        let found = detect(&[], &open, NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, open_title());
        assert!(found[0].detail.contains("30 件台"), "{}", found[0].detail);
        // 古い順(番号が大きいほど古い)に 5 件。
        assert!(
            found[0].detail.contains("#37, #36, #35, #34, #33"),
            "{}",
            found[0].detail
        );
    }

    #[test]
    fn open_count_at_threshold_is_not_a_candidate_and_bots_do_not_count() {
        let mut open: Vec<FlowIssue> = (1..=OPEN_THRESHOLD as u64)
            .map(|n| issue(n, "feat: 何か"))
            .collect();
        assert!(detect(&[], &open, NOW).is_empty());
        open.push(FlowIssue {
            author: "app/renovate".to_string(),
            ..issue(999, "Dependency Dashboard")
        });
        assert!(detect(&[], &open, NOW).is_empty());
    }

    #[test]
    fn listed_truncates_after_limit() {
        let numbers: Vec<u64> = (1..=LISTED as u64 + 3).collect();
        assert!(listed(&numbers).ends_with("ほか3件"));
    }
}
