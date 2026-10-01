//! decision-colocation(ADR-396、ADR-0024 Stage 4a #415): 決定成果物と
//! 執行点を同じ PR に強制する。
//!
//! - [`check`]: 判定エンジン(bash 版 `scripts/decision-colocation-check`)。
//!   CI required check(`decision-colocation-check` bin)と client guard が
//!   共有する単一ソース。
//! - [`guard`]: PreToolUse hook の判定(bash 版
//!   `config/claude/hooks/decision-colocation-guard.sh`)。コマンド解析は
//!   `guard-core`。

pub mod check;
pub mod guard;
