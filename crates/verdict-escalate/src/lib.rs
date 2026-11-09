//! verdict-escalate — 判定レッジャー(`agent-verdicts/*.jsonl`、ADR-478)を
//! セッション単位・fingerprint 単位に集約し、閾値を超えたら
//! wrap-up inbox(`issue-inbox --add`)へ 1 行追記する。
//!
//! 起票そのものは行わない — 追記する行には常に `"go":"ask"` を付け、
//! 実際の Issue 化は Stop の指示文経由で人間の明示的な GO を得てから
//! 行う(ADR-478 D6)。「収集・集約は自動、起票は人間の GO 後」という
//! 確定要件を、既存の wrap-up inbox 配管に最小差分で乗せる。
//!
//! どの失敗経路でも fail-open — 判定レッジャーが読めない・
//! issue-inbox が見つからない等はすべて「今回は何もしない」で
//! 縮退する(ADR-0005)。

pub mod record;

use agent_session::SessionLedger;
use record::{Verdict, VerdictRecord};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 同一セッション・同一 fingerprint でこの件数以上 deny/ask されたら
/// inbox 候補に昇格する(ADR-478 D2)。件数は「まとまり」の数で、並列の呼び出しは 1 つに数える
/// ([`BURST_WINDOW_SECS`])。
pub const THRESHOLD: usize = 3;

/// 同じ fingerprint の判定の `ts` がこの秒数以内に連続したものは、1 回の応答で並列に出した呼び出しと
/// みなし、1 つのまとまり(= 1 回の再発)に数える。「5 回弾かれ続けた」と「同じ応答の 5 本が同時に
/// 弾かれた」を同じ重みにしないため。値は実例(10 秒以内の 5 本、2026-10-28)から置いた仮値で、
/// ts は秒精度なので、これより細かくは分けられない。
pub const BURST_WINDOW_SECS: i64 = 15;

/// `${AGENT_VERDICTS_DIR}` → `${XDG_STATE_HOME}/agent-verdicts` →
/// `$HOME/.local/state/agent-verdicts`。bleep(bash)の `LEDGER_DIR` 解決と
/// 同じ優先順位(bleep 側は `BLEEP_LEDGER_DIR` を最優先に持つが、
/// 複数ツールが書く前提の共有ディレクトリなのでここでは汎用の
/// `AGENT_VERDICTS_DIR` だけを持つ)。
pub fn ledger_dir() -> Option<PathBuf> {
    if let Some(d) = non_empty_env("AGENT_VERDICTS_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = non_empty_env("XDG_STATE_HOME") {
        return Some(PathBuf::from(d).join("agent-verdicts"));
    }
    let home = non_empty_env("HOME")?;
    Some(PathBuf::from(home).join(".local/state/agent-verdicts"))
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// `dir` 内の `*.jsonl` すべてから、`session_id` に一致し verdict が
/// deny/ask のレコードを読む。壊れた行・スキーマ外の値を持つ行(将来の
/// 書き手が拡張した未知の reason_id 等)・レッジャーが担当外のファイル
/// (`hmac-key`・`*.lock`)は無視する(fail-open — 1 行の破損で他の行の
/// 集計を止めない)。
pub fn read_session_records(dir: &Path, session_id: &str) -> Vec<VerdictRecord> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(r) = serde_json::from_str::<VerdictRecord>(line) else {
                continue;
            };
            if r.session_id.as_deref() == Some(session_id)
                && matches!(r.verdict, Verdict::Deny | Verdict::Ask)
            {
                out.push(r);
            }
        }
    }
    out
}

/// fingerprint(閉語彙タプルの文字列表現、ADR-478 D3)。理由文やコマンド
/// 本文ではなく、これで群化する。
pub fn fingerprint(r: &VerdictRecord) -> String {
    format!(
        "{}|{:?}|{:?}|{}",
        r.tool,
        r.reason_id,
        r.match_class,
        r.term_hash.as_deref().unwrap_or("")
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ`(UTC)をエポック秒にする。形が違えば `None`(依存を増やさないため手で読む)。
fn epoch_secs(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let num = |a: usize, z: usize| ts.get(a..z)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Howard Hinnant の days_from_civil。
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// 判定の `ts` の列を、[`BURST_WINDOW_SECS`] 以内に連続するものを 1 つにしたまとまりの数にする。
/// `ts` が読めないものは、まとめず 1 件ずつ数える(数え漏らす側でなく、数える側に倒す)。
fn burst_count(times: &[Option<i64>]) -> usize {
    let unreadable = times.iter().filter(|t| t.is_none()).count();
    let mut readable: Vec<i64> = times.iter().filter_map(|t| *t).collect();
    readable.sort_unstable();
    let bursts = readable
        .windows(2)
        .filter(|w| w[1] - w[0] > BURST_WINDOW_SECS)
        .count()
        + usize::from(!readable.is_empty());
    unreadable + bursts
}

/// 昇格候補: `(fingerprint, 代表レコード, まとまりの数)`。まとまりの数 >= [`THRESHOLD`] の
/// ものだけを返す。入力の出現順を保つ(最初に見た代表レコードを使う)。
pub fn escalation_candidates(records: &[VerdictRecord]) -> Vec<(String, VerdictRecord, usize)> {
    let mut groups: Vec<(String, VerdictRecord, Vec<Option<i64>>)> = Vec::new();
    for r in records {
        let fp = fingerprint(r);
        let t = epoch_secs(&r.ts);
        if let Some(existing) = groups.iter_mut().find(|(f, _, _)| *f == fp) {
            existing.2.push(t);
        } else {
            groups.push((fp, r.clone(), vec![t]));
        }
    }
    groups
        .into_iter()
        .map(|(fp, r, times)| (fp, r, burst_count(&times)))
        .filter(|(_, _, count)| *count >= THRESHOLD)
        .collect()
}

/// 判定レッジャーの鍵ファイル(`hmac-key`)の SHA-256 の先頭 8 桁(hex)。`term_hash` の鍵は機体ごとに
/// 違い、`bleep explain` は鍵が同じ機体でしか語に一致しないので、どの機体で実行すべきかを、機体名を
/// 出さずに本文へ書くための識別子。鍵が読めなければ `None`。
pub fn key_id(dir: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(dir.join("hmac-key")).ok()?;
    let digest = Sha256::digest(bytes);
    Some(digest.iter().take(4).map(|b| format!("{b:02x}")).collect())
}

/// wrap-up inbox の 1 行(`{ts,title,detail,repo,go}`)を組み立てる。平文の
/// マッチ語・コマンド本文は含めない — `term_hash` はハッシュのまま本文に
/// 出す(短い辞書語は総当たり可能なため、生の語はどこにも書かない)。
pub fn inbox_line(
    now_iso8601: &str,
    fp: &str,
    record: &VerdictRecord,
    count: usize,
    key_id: Option<&str>,
) -> String {
    let title = format!(
        "{}: 同一セッションで同じ判定に {} 回弾かれた(reason_id={:?}, host={:?})",
        record.tool, count, record.reason_id, record.host
    );
    let detail = format!(
        "fingerprint={fp} count={count} tool_name={tool_name} session_id={session_id} key_id={key_id}\n\
count は、{window} 秒以内に連続した同じ判定を 1 回と数えた回数です(並列の呼び出しは 1 回)。\n\
詳細はローカルの agent-verdicts/{tool}.jsonl を session_id で grep してください。\n\
`bleep explain` は、key_id が同じ鍵を持つ機体で実行してください(term_hash の鍵は機体ごとに違い、\n\
別の機体では語に一致しません。機体名は書きません)。\n\
平文の語・コマンド本文はこの本文に含めていません(term_hash はマッチ語の\n\
ハッシュであり、短い辞書語は総当たり可能なため本文には出しません)。\n\
設定起因の偽陽性だと判断した場合は起票先を振り直してください。",
        tool_name = record.tool_name,
        session_id = record.session_id.as_deref().unwrap_or("unknown"),
        key_id = key_id.unwrap_or("unknown"),
        window = BURST_WINDOW_SECS,
        tool = record.tool,
    );
    serde_json::json!({
        "ts": now_iso8601,
        "title": title,
        "detail": detail,
        "repo": record.repo,
        "go": "ask",
    })
    .to_string()
}

/// `date -u +%Y-%m-%dT%H:%M:%SZ` に委ねる(依存を増やさないため)。取得に
/// 失敗しても不変(epoch)の値で続行する — 昇格の判定自体は止めない。
pub fn now_iso8601() -> String {
    let out = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "1970-01-01T00:00:00Z".to_string(),
    }
}

/// stamp 台帳: `${VERDICT_ESCALATE_STATE_DIR:-$HOME/.claude/verdict-escalate/state}`。
/// キーは fingerprint 文字列(改行を含まないので `SessionLedger` の制約を
/// 満たす)。
pub fn stamp_ledger() -> Option<SessionLedger> {
    let dir = match non_empty_env("VERDICT_ESCALATE_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(non_empty_env("HOME")?).join(".claude/verdict-escalate/state"),
    };
    Some(SessionLedger::new(dir, "stamped"))
}

/// `issue-inbox` のパス。`${ISSUE_INBOX_BIN}` で上書き可能
/// (テスト用 — 実配備では `~/.claude/hooks/issue-inbox`)。
pub fn issue_inbox_bin() -> Option<PathBuf> {
    if let Some(p) = non_empty_env("ISSUE_INBOX_BIN") {
        return Some(PathBuf::from(p));
    }
    Some(PathBuf::from(non_empty_env("HOME")?).join(".claude/hooks/issue-inbox"))
}

/// `session_id` の判定レッジャーを集約し、閾値を超えた未 stamp の
/// fingerprint を `inbox` へ追記して stamp する。追記に成功した件数を返す。
/// 前提(レッジャー dir・stamp 台帳・`issue-inbox`)のいずれかが
/// 揃わなければ何もせず `0` を返す(fail-open)。
pub fn run(session_id: &str, inbox: &Path) -> usize {
    let Some(dir) = ledger_dir() else {
        return 0;
    };
    let records = read_session_records(&dir, session_id);
    let candidates = escalation_candidates(&records);
    if candidates.is_empty() {
        return 0;
    }
    let Some(stamp) = stamp_ledger() else {
        return 0;
    };
    let Some(bin) = issue_inbox_bin() else {
        return 0;
    };
    let now = now_iso8601();
    let key = key_id(&dir);
    let mut added = 0;
    for (fp, record, count) in candidates {
        if stamp.contains(session_id, &fp) {
            continue;
        }
        let line = inbox_line(&now, &fp, &record, count, key.as_deref());
        let status = Command::new(&bin)
            .arg("--add")
            .arg(inbox)
            .arg(&line)
            .status();
        if matches!(status, Ok(s) if s.success()) {
            // stamp 失敗は次回リトライに任せる(inbox には既に書けている
            // ため、重複追記を避ける方を優先する側に倒す)。
            let _ = stamp.append(session_id, &fp);
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use record::{Host, MatchClass, ReasonId};

    fn rec(session_id: &str, term_hash: &str) -> VerdictRecord {
        rec_at(session_id, term_hash, "2026-09-25T00:00:00Z")
    }

    fn rec_at(session_id: &str, term_hash: &str, ts: &str) -> VerdictRecord {
        VerdictRecord {
            v: 1,
            ts: ts.into(),
            tool: "bleep".into(),
            tool_version: "0.2.0".into(),
            repo: "tarotene/bleep".into(),
            host: Host::Claude,
            session_id: Some(session_id.into()),
            verdict: Verdict::Deny,
            reason_id: ReasonId::RepoRef,
            match_class: MatchClass::Plain,
            term_hash: Some(term_hash.into()),
            tool_name: "Bash".into(),
            detail: None,
        }
    }

    /// 1 分おきの ts(= 並列ではない、別々の再発)。
    fn minute(n: u32) -> String {
        format!("2026-09-25T00:{n:02}:00Z")
    }

    #[test]
    fn groups_by_fingerprint_and_applies_threshold() {
        let records = vec![
            rec_at("s1", "hashA", &minute(0)),
            rec_at("s1", "hashA", &minute(1)),
            rec_at("s1", "hashB", &minute(2)), // 別 fingerprint、1 件のみ
            rec_at("s1", "hashA", &minute(3)),
        ];
        let candidates = escalation_candidates(&records);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, 3);
    }

    #[test]
    fn below_threshold_is_not_a_candidate() {
        let records = vec![
            rec_at("s1", "hashA", &minute(0)),
            rec_at("s1", "hashA", &minute(1)),
        ];
        assert!(escalation_candidates(&records).is_empty());
    }

    #[test]
    fn parallel_calls_in_one_response_count_as_one() {
        // 2026-10-28 の実例: 10 秒以内の 5 本は「5 回弾かれ続けた」ではなく、1 回の応答の並列。
        let records: Vec<_> = (47..=57)
            .step_by(2)
            .map(|s| rec_at("s1", "hashA", &format!("2026-10-28T02:35:{s:02}Z")))
            .collect();
        assert_eq!(records.len(), 6);
        assert!(escalation_candidates(&records).is_empty());
    }

    #[test]
    fn retries_after_a_gap_count_separately() {
        // 並列の塊が 1 つと、間を置いた再試行が 2 回 = 3 回。
        let records = vec![
            rec_at("s1", "hashA", "2026-10-28T02:35:47Z"),
            rec_at("s1", "hashA", "2026-10-28T02:35:50Z"),
            rec_at("s1", "hashA", "2026-10-28T02:36:30Z"),
            rec_at("s1", "hashA", "2026-10-28T02:40:00Z"),
        ];
        let candidates = escalation_candidates(&records);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, 3);
    }

    #[test]
    fn the_burst_window_boundary_is_inclusive() {
        // 15 秒ちょうどは同じまとまり、16 秒は別のまとまり。
        let same = [
            rec_at("s1", "hashA", "2026-10-28T02:00:00Z"),
            rec_at("s1", "hashA", "2026-10-28T02:00:15Z"),
        ];
        assert_eq!(
            burst_count(&same.iter().map(|r| epoch_secs(&r.ts)).collect::<Vec<_>>()),
            1
        );
        let apart = [
            rec_at("s1", "hashA", "2026-10-28T02:00:00Z"),
            rec_at("s1", "hashA", "2026-10-28T02:00:16Z"),
        ];
        assert_eq!(
            burst_count(&apart.iter().map(|r| epoch_secs(&r.ts)).collect::<Vec<_>>()),
            2
        );
    }

    #[test]
    fn unreadable_timestamps_are_counted_one_by_one() {
        let records = vec![
            rec_at("s1", "hashA", "not-a-time"),
            rec_at("s1", "hashA", "not-a-time"),
            rec_at("s1", "hashA", "not-a-time"),
        ];
        let candidates = escalation_candidates(&records);
        assert_eq!(candidates[0].2, 3);
    }

    #[test]
    fn epoch_secs_reads_utc_timestamps() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_secs("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(epoch_secs("2026-10-28T02:35:47Z"), Some(1_793_154_947));
        assert_eq!(epoch_secs("2026-10-28 02:35:47"), None);
        assert_eq!(epoch_secs("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn key_id_is_a_short_fingerprint_of_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(key_id(dir.path()), None); // 鍵が無い
        std::fs::write(dir.path().join("hmac-key"), "secret").unwrap();
        let id = key_id(dir.path()).unwrap();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        // 鍵の中身は出さない。
        assert!(!id.contains("secret"));
        // 別の鍵なら別の指紋。
        std::fs::write(dir.path().join("hmac-key"), "another").unwrap();
        assert_ne!(key_id(dir.path()).unwrap(), id);
    }

    #[test]
    fn read_session_records_ignores_other_sessions_and_verdicts() {
        let dir = tempfile::tempdir().unwrap();
        let mut pass = rec("s1", "hashA");
        pass.verdict = Verdict::Deny; // 後で pass 相当の行を手書きで混ぜる
        let lines = format!(
            "{}\n{}\n{{\"not\":\"json\"\n{}\n",
            serde_json::to_string(&rec("s1", "hashA")).unwrap(),
            serde_json::to_string(&rec("s2", "hashA")).unwrap(),
            serde_json::to_string(&rec("s1", "hashB")).unwrap(),
        );
        std::fs::write(dir.path().join("bleep.jsonl"), lines).unwrap();
        std::fs::write(dir.path().join("hmac-key"), "not-jsonl").unwrap();
        let got = read_session_records(dir.path(), "s1");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn inbox_line_never_contains_plaintext_term() {
        let r = rec("s1", "deadbeef");
        let line = inbox_line("2026-09-25T00:00:00Z", "fp", &r, 3, Some("0a1b2c3d"));
        assert!(line.contains("\"repo\":\"tarotene/bleep\""));
        assert!(line.contains("key_id=0a1b2c3d"));
        assert!(!line.contains("deadbeef")); // term_hash は本文に出さない
        assert!(line.contains("\"go\":\"ask\""));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["repo"], "tarotene/bleep");
        assert_eq!(v["go"], "ask");
    }
}
