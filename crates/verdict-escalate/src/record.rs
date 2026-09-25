//! 判定レッジャー(`agent-verdicts/<tool>.jsonl`)1 レコードの契約(ADR-478)。
//!
//! この型が正本で、`docs/schemas/agent-verdict.schema.json` はここからの
//! 生成物(`examples/gen_schema.rs`、`tests/schema.rs` が一致を検証する)。
//! 書き手(現状 tarotene/bleep のみ)はこの Schema に従って
//! `agent-verdicts/<tool>.jsonl` に追記する。未知のフィールドは無視する
//! (将来の書き手が拡張フィールドを足しても読み手は壊れない)。

use serde::{Deserialize, Serialize};

/// `verdict` フィールド。`pass` は書かれない(分母が要る集計は将来の拡張)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Deny,
    Ask,
}

/// `reason_id` フィールド。閉語彙 — 未知の値を書く書き手が現れたら、この
/// enum を拡張してから Schema を再生成する(ADR-478 D3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReasonId {
    /// org/repo 形式、または denylist 登録済みリポジトリ名の裸の単語一致。
    RepoRef,
    /// denylist 登録済み org 名の裸の単体出現。
    OrgBare,
    /// 判定に必要な設定ファイル(例: orgs.txt)が存在しない。
    Unconfigured,
    /// 設定ディレクトリの移行が未了。
    LegacyConfig,
    /// コマンド文字列の字句解析に失敗した。
    LexFailed,
    /// push 差分の計算に失敗した。
    PushDiffFailed,
    /// 判定エンジン自体が起動できない、または入力を読めない。
    EngineUnavailable,
}

/// `match_class` フィールド。マッチしなかった判定(fail-loud な ask 等)は
/// `none`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MatchClass {
    /// スラッシュを含む固定文字列(org/repo 形式)の一致。
    Plain,
    /// 裸のリポジトリ名の単語境界一致。
    WordHard,
    /// 裸の org 名の単語境界一致。
    WordWarn,
    #[serde(rename = "none")]
    NoneMatchClass,
}

/// `host` フィールド。判定を呼び出したホスト。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Host {
    Claude,
    Codex,
    Copilot,
    GitPrePush,
    Cli,
}

/// 判定レッジャーの 1 レコード。フィールド順は書き手(bleep)の出力に合わせる
/// (可読性のためだけで、契約上の意味はない)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VerdictRecord {
    /// レコード形式のバージョン。現在は 1 固定。
    pub v: u32,
    /// ISO 8601(UTC、`Z` 終端)。
    pub ts: String,
    /// 書き手のツール名(例: `"bleep"`)。`agent-verdicts/<tool>.jsonl` の
    /// `<tool>` と一致する。
    pub tool: String,
    pub tool_version: String,
    /// 起票候補になったときの既定の起票先(`"owner/repo"`)。
    pub repo: String,
    pub host: Host,
    /// hook を呼んだセッション ID。ホストが持たない場合は `null`。
    pub session_id: Option<String>,
    pub verdict: Verdict,
    pub reason_id: ReasonId,
    pub match_class: MatchClass,
    /// マッチした語の HMAC ハッシュ(hex64)。マッチが無い判定では `null`。
    pub term_hash: Option<String>,
    /// 呼ばれたツール名(例: `"Bash"`、MCP ツール名)。git pre-push など
    /// ツール呼び出しを経ない経路では空文字列。
    pub tool_name: String,
}
