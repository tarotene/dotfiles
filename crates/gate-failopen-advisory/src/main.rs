//! gate-failopen-advisory — fail-open で素通りし続けている gate を、SessionStart で知らせる
//! (#822、docs/agents/gate-failopen-advisory.md)。
//!
//! fail-open で設計した hook(例: copilot-plan-review)は、依存先(外部 CLI・モデル・認証)が
//! 壊れても止まらない。その代わり「壊れたまま素通りし続けている」ことに気づく手段が、毎回の
//! 警告を人が読むことしか無かった。実例では、固定していたモデルが catalog から消えた
//! 2026-09-29 から約 2 週間半、ExitPlanMode のたびに警告を出して素通りしていたが、利用者が
//! 「呼ばれていない気がする」と疑うまで検出されなかった。
//!
//! gate が `gate-events.jsonl` に残す `fail-open` / `pass` を末尾から数え(状態ファイルは
//! 持たない)、同じ gate の最後の `pass` より後の `fail-open` が閾値(既定 3、環境変数
//! `GATE_FAILOPEN_THRESHOLD`)以上なら、`additionalContext` で警告する。何も実行しない。
//! ログが無い・読めない・閾値に届かない場合は何も出さない(全経路 fail-open)。hook の入力
//! (stdin)は使わない。

use agent_session::entry::{enter, Spec};
use agent_session::gate_event::default_path;
use agent_session::gate_event::trailing_fail_open;
use agent_session::{Agent, HookEvent, Reply};
use std::path::Path;
use std::process::ExitCode;

const SPEC: Spec = Spec {
    name: "gate-failopen-advisory",
    agents: &[Agent::Claude, Agent::Codex],
    event: Some(HookEvent::SessionStart),
};

/// 既定の閾値。`promotion-detect` の `DEMOTION_SKIP_THRESHOLD`(3)にそろえる。タイムアウト等の
/// 一過性の失敗は 1〜2 回で収まり、ExitPlanMode は 1 日に数回なので、当日中に検出できる
/// (従来は約 2 週間半)。
const DEFAULT_THRESHOLD: usize = 3;

/// 監視する gate と、閾値に届いたときに添える復旧の手がかり。gate 名は、その gate が
/// `gate-events.jsonl` に書く名前(`crates/copilot-plan-review` の `GATE_NAME`)と同じ。
/// 他の fail-open の hook を足すときは、ここに 1 行足す。
const GATES: &[(&str, &str)] = &[
    (
        "copilot-plan-review",
        "ExitPlanMode のたびに出る systemMessage に、実行に失敗した原因(copilot の stderr の先頭行)が書かれています。copilot CLI が起動できるか、固定しているモデルが catalog に残っているか(`model-pin-update` スキル)、ログインが切れていないかを確かめてください。手順は docs/agents/copilot-plan-review.md。",
    ),
    (
        "ja-polish",
        "`ja-polish` の報告(素通しの理由)に原因が書かれています。API キーが入っているか(`llm keys list`)、無料枠の上限、スタイルガイドのハブが解決できるか(`writing-style-hub`)、設定のモデル名が `llm models` に残っているかを確かめてください。手順は docs/agents/ja-polish.md。",
    ),
];

fn threshold_from(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_THRESHOLD)
}

/// 閾値に届いた gate ごとの警告文。届いた gate が無ければ空。
fn gate_lines(events: &Path, threshold: usize) -> Vec<String> {
    GATES
        .iter()
        .filter_map(|(gate, hint)| {
            let n = trailing_fail_open(events, gate);
            (n >= threshold).then(|| {
                format!(
                    "{gate} が {n} 回連続で fail-open(依存先の不調で処理を実行できないまま素通り)しています。{hint}"
                )
            })
        })
        .collect()
}

/// 課金の検討を知らせる、一時的な原因による素通しの割合(%)の既定の閾値。
const DEFAULT_BILLING_PERCENT: f64 = 5.0;

fn billing_percent_from(raw: Option<&str>) -> f64 {
    raw.and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|p| p.is_finite() && *p > 0.0)
        .unwrap_or(DEFAULT_BILLING_PERCENT)
}

/// ja-polish の課金の検討の知らせ(ADR-1387 の D6 の続き、段 4)。直近 7 日の記録のうち、混雑・時間切れで
/// 素通しになった段落の割合が `percent_threshold` を超えたら、1 行の警告を返す。キーの未設定や照合の外れ
/// による素通しは数えない(課金では直らない)。記録が `TRANSIENT_MIN_SAMPLES` 件に満たないときは、率が
/// 不安定なので何も言わない。
fn billing_line(records: &Path, now_unix: u64, percent_threshold: f64) -> Option<String> {
    use ja_polish::records::{transient_rate, TRANSIENT_MIN_SAMPLES, TRANSIENT_WINDOW};
    let rate = transient_rate(records, now_unix, TRANSIENT_WINDOW);
    if rate.total < TRANSIENT_MIN_SAMPLES || rate.percent() <= percent_threshold {
        return None;
    }
    Some(format!(
        "ja-polish が、直近 7 日の {total} 段落のうち {n} 段落({pct:.0}%)を、混雑・時間切れで素通しにしています(閾値 {th}%)。無料枠は、混んだときに先に切り捨てられる枠で処理される(推論)ので、有料枠を検討してください。有料枠は、送った文面を Google の製品の改善に使わず、切り捨てられにくい。gemini-3.6-flash で月の目安は $30 前後(推定)。判断の材料は docs/adr/1387-ja-polish.md の「有料の API キー・Vertex AI」。閾値は環境変数 `JA_POLISH_TRANSIENT_ADVISORY_PERCENT` で変えられる。",
        total = rate.total,
        n = rate.transient,
        pct = rate.percent(),
        th = percent_threshold,
    ))
}

/// ja-polish がこの機体で有効か(設定ファイルがあるか。`ja_polish::config` の定義と同じ)。
fn ja_polish_enabled() -> bool {
    ja_polish::config::config_path().is_some_and(|p| p.is_file())
}

fn run() -> Option<()> {
    let events = default_path()?;
    let threshold = threshold_from(std::env::var("GATE_FAILOPEN_THRESHOLD").ok().as_deref());
    let mut lines = gate_lines(&events, threshold);
    if ja_polish_enabled() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let percent = billing_percent_from(
            std::env::var("JA_POLISH_TRANSIENT_ADVISORY_PERCENT")
                .ok()
                .as_deref(),
        );
        if let Some(l) =
            ja_polish::records::records_path().and_then(|p| billing_line(&p, now, percent))
        {
            lines.push(l);
        }
    }
    if lines.is_empty() {
        return None;
    }
    let ctx = format!("[gate-failopen-advisory] {}", lines.join("\n"));
    print!(
        "{}",
        Reply::context(ctx).render(Agent::Claude, HookEvent::SessionStart)
    );
    Some(())
}

fn main() -> ExitCode {
    // 標準入力は読まない(読まずに閉じてよい)ので、引数だけを解析する。
    if let Err(code) = enter(&SPEC) {
        return code;
    }
    let _ = run();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_parsing() {
        assert_eq!(threshold_from(None), 3);
        assert_eq!(threshold_from(Some("5")), 5);
        assert_eq!(threshold_from(Some(" 2 ")), 2);
        assert_eq!(
            threshold_from(Some("0")),
            3,
            "0 は常時警告になるので既定に戻す"
        );
        assert_eq!(threshold_from(Some("-1")), 3);
        assert_eq!(threshold_from(Some("abc")), 3);
        assert_eq!(threshold_from(Some("")), 3);
    }

    #[test]
    fn billing_percent_parsing() {
        assert_eq!(billing_percent_from(None), 5.0);
        assert_eq!(billing_percent_from(Some(" 12.5 ")), 12.5);
        assert_eq!(billing_percent_from(Some("0")), 5.0, "0 は常時警告になる");
        assert_eq!(billing_percent_from(Some("-3")), 5.0);
        assert_eq!(billing_percent_from(Some("abc")), 5.0);
        assert_eq!(billing_percent_from(Some("NaN")), 5.0);
    }

    fn write_records(dir: &Path, now: u64, transient: usize, others: usize) -> std::path::PathBuf {
        use ja_polish::records::{append, Entry};
        let path = dir.join("records.jsonl");
        let mut entries = Vec::new();
        for i in 0..transient {
            let mut e = Entry::skipped(format!("t{i}"), "m", "混雑").with_transient(true);
            e.ts_unix = now - 60;
            entries.push(e);
        }
        for i in 0..others {
            let mut e = Entry::polished(format!("p{i}"), "m");
            e.ts_unix = now - 60;
            entries.push(e);
        }
        append(&path, &entries).unwrap();
        path
    }

    #[test]
    fn billing_line_needs_enough_samples_and_a_rate_above_the_threshold() {
        let d = tempfile::tempdir().unwrap();
        let now = 1_000_000;
        // 20 件中 2 件(10%)で、閾値 5% を超える: 知らせる。
        let p = write_records(d.path(), now, 2, 18);
        let line = billing_line(&p, now, 5.0).expect("10% > 5%");
        assert!(line.contains("20 段落のうち 2 段落(10%)"), "{line}");
        assert!(line.contains("docs/adr/1387-ja-polish.md"), "{line}");
        // 同じ率でも、閾値ちょうどは超えていない: 黙る。
        assert!(billing_line(&p, now, 10.0).is_none());
        // 率が高くても、件数が下限(20)に満たない: 黙る。
        let d2 = tempfile::tempdir().unwrap();
        let few = write_records(d2.path(), now, 3, 6);
        assert!(billing_line(&few, now, 5.0).is_none());
        // 記録が無い: 黙る。
        assert!(billing_line(&d.path().join("none.jsonl"), now, 5.0).is_none());
    }
}
