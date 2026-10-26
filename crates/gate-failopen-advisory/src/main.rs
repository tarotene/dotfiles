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

use agent_session::gate_event::default_path;
use agent_session::gate_event::trailing_fail_open;
use serde_json::json;
use std::path::Path;

/// 既定の閾値。`promotion-detect` の `DEMOTION_SKIP_THRESHOLD`(3)にそろえる。タイムアウト等の
/// 一過性の失敗は 1〜2 回で収まり、ExitPlanMode は 1 日に数回なので、当日中に検出できる
/// (従来は約 2 週間半)。
const DEFAULT_THRESHOLD: usize = 3;

/// 監視する gate と、閾値に届いたときに添える復旧の手がかり。gate 名は、その gate が
/// `gate-events.jsonl` に書く名前(`crates/copilot-plan-review` の `GATE_NAME`)と同じ。
/// 他の fail-open の hook を足すときは、ここに 1 行足す。
const GATES: &[(&str, &str)] = &[(
    "copilot-plan-review",
    "ExitPlanMode のたびに出る systemMessage に、実行に失敗した原因(copilot の stderr の先頭行)が書かれています。copilot CLI が起動できるか、固定しているモデルが catalog に残っているか(`model-pin-update` スキル)、ログインが切れていないかを確かめてください。手順は docs/agents/copilot-plan-review.md。",
)];

fn threshold_from(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_THRESHOLD)
}

/// 閾値に届いた gate ごとの警告文。届いた gate が無ければ `None`。
fn advisory(events: &Path, threshold: usize) -> Option<String> {
    let lines: Vec<String> = GATES
        .iter()
        .filter_map(|(gate, hint)| {
            let n = trailing_fail_open(events, gate);
            (n >= threshold).then(|| {
                format!(
                    "{gate} が {n} 回連続で fail-open(依存先の不調でレビューを実行できないまま素通り)しています。{hint}"
                )
            })
        })
        .collect();
    (!lines.is_empty()).then(|| format!("[gate-failopen-advisory] {}", lines.join("\n")))
}

fn run() -> Option<()> {
    let events = default_path()?;
    let threshold = threshold_from(std::env::var("GATE_FAILOPEN_THRESHOLD").ok().as_deref());
    let ctx = advisory(&events, threshold)?;
    println!(
        "{}",
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": ctx}})
    );
    Some(())
}

fn main() {
    let _ = run();
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
}
