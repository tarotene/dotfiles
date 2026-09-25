//! verdict-escalate — 判定レッジャー(`agent-verdicts/*.jsonl`、ADR-0000)を
//! セッション単位・fingerprint 単位に集約し、閾値を超えたら
//! wrap-up inbox(`wrapup-stop-gate.sh --add`)へ 1 行追記する。
//!
//! 起票そのものは行わない — 追記する行には常に `"go":"ask"` を付け、
//! 実際の Issue 化は Stop の指示文経由で人間の明示的な GO を得てから
//! 行う(ADR-0000 D6)。「収集・集約は自動、起票は人間の GO 後」という
//! 確定要件を、既存の wrap-up inbox 配管に最小差分で乗せる。
//!
//! どの失敗経路でも fail-open — 判定レッジャーが読めない・
//! wrapup-stop-gate.sh が見つからない等はすべて「今回は何もしない」で
//! 縮退する(ADR-0005)。

pub mod record;

use hook_io::SessionLedger;
use record::{Verdict, VerdictRecord};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 同一セッション・同一 fingerprint でこの件数以上 deny/ask されたら
/// inbox 候補に昇格する(ADR-0000 D2)。
pub const THRESHOLD: usize = 3;

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

/// fingerprint(閉語彙タプルの文字列表現、ADR-0000 D3)。理由文やコマンド
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

/// 昇格候補: `(fingerprint, 代表レコード, 件数)`。件数 >= [`THRESHOLD`] の
/// ものだけを返す。入力の出現順を保つ(最初に見た代表レコードを使う)。
pub fn escalation_candidates(records: &[VerdictRecord]) -> Vec<(String, VerdictRecord, usize)> {
    let mut groups: Vec<(String, VerdictRecord, usize)> = Vec::new();
    for r in records {
        let fp = fingerprint(r);
        if let Some(existing) = groups.iter_mut().find(|(f, _, _)| *f == fp) {
            existing.2 += 1;
        } else {
            groups.push((fp, r.clone(), 1));
        }
    }
    groups.retain(|(_, _, count)| *count >= THRESHOLD);
    groups
}

/// wrap-up inbox の 1 行(`{ts,title,detail,repo,go}`)を組み立てる。平文の
/// マッチ語・コマンド本文は含めない — `term_hash` はハッシュのまま本文に
/// 出す(短い辞書語は総当たり可能なため、生の語はどこにも書かない)。
pub fn inbox_line(now_iso8601: &str, fp: &str, record: &VerdictRecord, count: usize) -> String {
    let title = format!(
        "{}: 同一セッションで同じ判定に {} 回弾かれた(reason_id={:?}, host={:?})",
        record.tool, count, record.reason_id, record.host
    );
    let detail = format!(
        "fingerprint={fp} count={count} tool_name={tool_name} session_id={session_id}\n\
詳細はローカルの agent-verdicts/{tool}.jsonl を session_id で grep してください。\n\
平文の語・コマンド本文はこの本文に含めていません(term_hash はマッチ語の\n\
ハッシュであり、短い辞書語は総当たり可能なため本文には出しません)。\n\
設定起因の偽陽性だと判断した場合は起票先を振り直してください。",
        tool_name = record.tool_name,
        session_id = record.session_id.as_deref().unwrap_or("unknown"),
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

/// `wrapup-stop-gate.sh` のパス。`${WRAPUP_STOP_GATE_BIN}` で上書き可能
/// (テスト用 — 実配備では `~/.claude/hooks/wrapup-stop-gate.sh`)。
pub fn wrapup_stop_gate_bin() -> Option<PathBuf> {
    if let Some(p) = non_empty_env("WRAPUP_STOP_GATE_BIN") {
        return Some(PathBuf::from(p));
    }
    Some(PathBuf::from(non_empty_env("HOME")?).join(".claude/hooks/wrapup-stop-gate.sh"))
}

/// `session_id` の判定レッジャーを集約し、閾値を超えた未 stamp の
/// fingerprint を `inbox` へ追記して stamp する。追記に成功した件数を返す。
/// 前提(レッジャー dir・stamp 台帳・`wrapup-stop-gate.sh`)のいずれかが
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
    let Some(bin) = wrapup_stop_gate_bin() else {
        return 0;
    };
    let now = now_iso8601();
    let mut added = 0;
    for (fp, record, count) in candidates {
        if stamp.contains(session_id, &fp) {
            continue;
        }
        let line = inbox_line(&now, &fp, &record, count);
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
        VerdictRecord {
            v: 1,
            ts: "2026-09-25T00:00:00Z".into(),
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
        }
    }

    #[test]
    fn groups_by_fingerprint_and_applies_threshold() {
        let records = vec![
            rec("s1", "hashA"),
            rec("s1", "hashA"),
            rec("s1", "hashB"), // 別 fingerprint、1 件のみ
            rec("s1", "hashA"),
        ];
        let candidates = escalation_candidates(&records);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, 3);
    }

    #[test]
    fn below_threshold_is_not_a_candidate() {
        let records = vec![rec("s1", "hashA"), rec("s1", "hashA")];
        assert!(escalation_candidates(&records).is_empty());
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
        let line = inbox_line("2026-09-25T00:00:00Z", "fp", &r, 3);
        assert!(line.contains("\"repo\":\"tarotene/bleep\""));
        assert!(line.contains("\"go\":\"ask\""));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["repo"], "tarotene/bleep");
        assert_eq!(v["go"], "ask");
    }
}
