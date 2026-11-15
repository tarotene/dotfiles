//! reply-coverage — 相手のいる会話への返信が、相手の未決の依頼すべてに応答しているかを検査する。
//!
//! 設計と手順: docs/agents/reply-coverage.md。
//!
//! - `text`: 正規化・逐語照合・ハッシュ(記録の `sha` と引用の照合の単一の定義)
//! - `store`: 機体ローカルの状態(会話の全文・検査の記録。有効期限で掃く)
//! - `detect`: 検出の指示文・出力スキーマ・照合(`judge`)
//! - `backend`: 検出の送り先(文脈を切った `claude -p`)
//! - `check`: 1 回の検査(検出 → 照合 → 記録)と報告
//! - `verify`: 照会(記録を引くだけ。関所と裁定ページの生成が呼ぶ)
//! - `bench`: 見本でモデルごとの質と時間を測る

pub mod backend;
pub mod bench;
pub mod check;
pub mod detect;
pub mod store;
pub mod text;
pub mod verify;
