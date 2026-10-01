//! lifecycle ドメイン(#275、ADR-0023): 休眠候補のスコアリング。
//!
//! 他ドメインと性質が違う(#261 の裁定): 他は「あるべき設定から外れている」
//! という drift 判定、こちらは「活動が止まっているかもしれない」という候補
//! 提示にすぎない(意図的に完了させたプロジェクトかもしれない)。verdict は
//! ok / dormancy-candidate の 2 値で、後者は [`crate::any_drift`] から除外
//! する。Maintain/Archive/Delete の最終判断は docs/repo-lifecycle.md の人間
//! 裁定。
//!
//! シグナルは決定論的に取れるものだけ:
//! - 最終 push からの経過日数(pushedAt)
//! - 最終 tagged release からの経過日数(release が 1 件も無ければ計上しない)
//! - 最も新しく更新された open issue の経過日数(open issue が無ければ計上
//!   しない — 0 件は停滞の証拠にならない)
//! - CI の有無(無ければ +1)・直近の完了 run の成否(failure なら +1)

use crate::model::{Detail, Finding, RepoGql, Verdict};
use std::process::{Command, Stdio};

pub const LIFECYCLE_STALE_PUSH_DAYS_WARN: i64 = 180;
pub const LIFECYCLE_STALE_PUSH_DAYS_HIGH: i64 = 365;
pub const LIFECYCLE_STALE_RELEASE_DAYS: i64 = 365;
pub const LIFECYCLE_STALE_ISSUE_DAYS: i64 = 180;
pub const LIFECYCLE_CANDIDATE_SCORE_THRESHOLD: i64 = 3;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant の days_from_civil(1970-01-01 起点の日数)。
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn num(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// GitHub が返す形(`YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`)の epoch 秒。
fn parse_iso8601(ts: &str) -> Option<i64> {
    let (date, rest) = ts.split_once('T')?;
    let mut dp = date.split('-');
    let (y, mo, d) = (num(dp.next()?)?, num(dp.next()?)?, num(dp.next()?)?);
    if dp.next().is_some() || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (time, offset) = if let Some(t) = rest.strip_suffix('Z') {
        (t, 0)
    } else {
        let i = rest.rfind(['+', '-'])?;
        let (t, off) = rest.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let (oh, om) = off[1..].split_once(':')?;
        (t, sign * (num(oh)? * 3600 + num(om)? * 60))
    };
    let time = time.split('.').next()?;
    let mut tp = time.split(':');
    let (h, mi, s) = (num(tp.next()?)?, num(tp.next()?)?, num(tp.next()?)?);
    if tp.next().is_some() || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s - offset)
}

/// `date -u -d <ts> +%s`。上の形で読めなければ GNU date に委ねる(bash 版と
/// 同じ解釈の幅を保つ)。
fn epoch(ts: &str) -> Option<i64> {
    if let Some(e) = parse_iso8601(ts) {
        return Some(e);
    }
    let o = Command::new("date")
        .args(["-u", "-d", ts, "+%s"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

fn now_utc() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// bash: `lifecycle_days_since`。`now` は基準時刻(`None` なら現在時刻)。
/// `ts` が空・読めなければ `None`。端数は 0 方向に切り捨て(bash の整数除算)。
pub fn lifecycle_days_since(ts: &str, now: Option<&str>) -> Option<i64> {
    if ts.is_empty() {
        return None;
    }
    let now_epoch = match now {
        Some(n) => epoch(n)?,
        None => now_utc(),
    };
    let ts_epoch = epoch(ts)?;
    Some((now_epoch - ts_epoch) / 86400)
}

/// bash: `judge_lifecycle`。`ci_conclusion` は直近の完了 run の conclusion
/// (CI が無い・取得不能なら空)。
pub fn judge_lifecycle(
    pushed_at: &str,
    has_workflows: bool,
    gql: &RepoGql,
    ci_conclusion: &str,
    now: Option<&str>,
) -> Finding {
    let mut score = 0i64;
    let mut signals: Vec<String> = Vec::new();

    if let Some(push_days) = lifecycle_days_since(pushed_at, now) {
        if push_days > LIFECYCLE_STALE_PUSH_DAYS_HIGH {
            score += 2;
            signals.push(format!("stale-push:{push_days}d"));
        } else if push_days > LIFECYCLE_STALE_PUSH_DAYS_WARN {
            score += 1;
            signals.push(format!("stale-push:{push_days}d"));
        }
    }

    let release_at = gql
        .latest_release
        .as_ref()
        .and_then(|r| r.nodes.as_ref())
        .and_then(|n| n.first())
        .and_then(|n| n.created_at.as_deref())
        .unwrap_or("");
    if !release_at.is_empty() {
        if let Some(days) = lifecycle_days_since(release_at, now) {
            if days > LIFECYCLE_STALE_RELEASE_DAYS {
                score += 1;
                signals.push(format!("stale-release:{days}d"));
            }
        }
    }

    let activity = gql.open_issue_activity.as_ref();
    let open_issue_count = activity.and_then(|a| a.total_count).unwrap_or(0);
    if open_issue_count > 0 {
        let issue_at = activity
            .and_then(|a| a.nodes.as_ref())
            .and_then(|n| n.first())
            .and_then(|n| n.updated_at.as_deref())
            .unwrap_or("");
        if let Some(days) = lifecycle_days_since(issue_at, now) {
            if days > LIFECYCLE_STALE_ISSUE_DAYS {
                score += 1;
                signals.push(format!("stale-issues:{days}d"));
            }
        }
    }

    if !has_workflows {
        score += 1;
        signals.push("ci-absent".into());
    } else if ci_conclusion == "failure" {
        score += 1;
        signals.push("ci-failing".into());
    }

    Finding {
        verdict: if score >= LIFECYCLE_CANDIDATE_SCORE_THRESHOLD {
            Verdict::DormancyCandidate
        } else {
            Verdict::Ok
        },
        missing: signals,
        detail: Detail::Lifecycle { score },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse() {
        assert_eq!(parse_iso8601("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(parse_iso8601("1970-01-01T09:00:00+09:00"), Some(0));
        assert_eq!(
            lifecycle_days_since("2025-08-19T00:00:00Z", Some("2026-09-23T00:00:00Z")),
            Some(400)
        );
        assert_eq!(lifecycle_days_since("", Some("2026-09-23T00:00:00Z")), None);
    }
}
