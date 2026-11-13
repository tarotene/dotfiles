//! 書き直しの 1 回分の流れ(基準の読み込み → 書き直し → 記録 → gate イベント)と、
//! 利用者(Claude)に見せる報告の組み立て。CLI と統合テストが同じものを呼ぶ。

use crate::config::Config;
use crate::llm::{Backend, LlmError, Request};
use crate::polish::{polish, PolishResult, Status};
use crate::{records, style};
use std::path::PathBuf;

/// 実行の外部状態。テストでは一時ディレクトリを指す。
pub struct Env {
    pub config: Config,
    pub records_path: Option<PathBuf>,
    pub gate_events: Option<PathBuf>,
}

impl Env {
    pub fn from_environment(config: Config) -> Self {
        Env {
            config,
            records_path: records::records_path(),
            gate_events: agent_session::gate_event::default_path(),
        }
    }
}

/// 基準が読めないとき、依頼を出さずに全段落を素通しにするための送り先。待っても直らない失敗として返す
/// (再依頼も予備のモデルも使わない)。
struct Refuse(String);

impl Backend for Refuse {
    fn complete(&self, _: &Request) -> Result<String, LlmError> {
        Err(LlmError::permanent(self.0.clone()))
    }
}

/// 文書を書き直す。`dry_run` なら記録も gate イベントも書かない。
///
/// 通せなかった段落があれば、gate イベントに `fail-open`(`gate-failopen-advisory` が、
/// 続けて失敗しているときにセッションの冒頭で知らせる)。すべて通れば `pass`。
/// 書き直す段落が無かったときは、何も書かない。
pub fn run_polish(
    env: &Env,
    text: &str,
    baseline: Option<&str>,
    backend: &dyn Backend,
    dry_run: bool,
) -> PolishResult {
    // 素通しの記録は「通した」とみなさない(関所は逆。`records::load_polished` の説明)。
    let known = env
        .records_path
        .as_deref()
        .map(records::load_polished)
        .unwrap_or_default();

    let result = match style::load() {
        Ok(style) => polish(text, baseline, &known, &style, &env.config, backend),
        Err(reason) => polish(text, baseline, &known, "", &env.config, &Refuse(reason)),
    };

    if !dry_run && !result.paras.is_empty() {
        if let Some(path) = &env.records_path {
            if let Err(e) = records::append(path, &result.entries) {
                eprintln!("ja-polish: 記録を書けません({}): {e}", path.display());
            }
        }
        if let Some(path) = &env.gate_events {
            let decision = if result.skipped().is_empty() {
                "pass"
            } else {
                "fail-open"
            };
            let _ = agent_session::gate_event::record(path, GATE_NAME, decision);
        }
    }
    result
}

/// gate イベントに書く名前。`gate-failopen-advisory` の監視表と同じ。
pub const GATE_NAME: &str = "ja-polish";

/// 利用者に見せる報告。書き直した段落は元と新しい本文を並べる(意味がずれていないかを
/// 読んでもらうため)。
pub fn report(label: &str, result: &PolishResult) -> String {
    let polished = result.count(|s| matches!(s, Status::Polished));
    let unchanged = result.count(|s| matches!(s, Status::Unchanged));
    let skipped = result.count(|s| matches!(s, Status::Skipped(_)));
    let mut out = format!(
        "ja-polish: {label}: 書き直し {polished} / 変更なし {unchanged} / 素通し {skipped}\n"
    );
    for (i, p) in result.paras.iter().enumerate() {
        match &p.status {
            Status::Polished => {
                out.push_str(&format!("  [段落 {}] 書き直し\n", i + 1));
                for l in p.before.lines() {
                    out.push_str(&format!("  - {l}\n"));
                }
                for l in p.after.lines() {
                    out.push_str(&format!("  + {l}\n"));
                }
            }
            Status::Skipped(reason) => {
                out.push_str(&format!("  [段落 {}] 素通し: {reason}\n", i + 1));
            }
            Status::Unchanged => {}
        }
    }
    out
}
