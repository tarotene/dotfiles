//! ja-polish — 日本語の文書を、`llm` 経由で Gemini に書き直させる。通した段落は記録に残し、
//! 関所(投稿の PreToolUse、文書ファイルの Stop)が「通していない日本語の段落」を拾う。
//!
//! 設計と手順: docs/agents/ja-polish.md。
//!
//! - `blocks`: 文書を段落に分け、日本語かどうかとハッシュを出す
//! - `mask`: コード・URL・参照・見出しなどを伏せ字にし、戻りを照合する
//! - `llm`: `llm` コマンドの呼び出しと、指示文・依頼文・応答の読み取り
//! - `polish`: 書き直しの本体と、未通過の段落の判定(`polish::pending`)
//! - `records`: 通した段落の記録(形式の正本)
//! - `config`: 設定ファイル。あれば有効な機体
//! - `baseline`・`style`: 基点の内容と、書き直しの基準(文体規約)の取得
//! - `app`: 1 回分の流れと報告

pub mod app;
pub mod baseline;
pub mod blocks;
pub mod config;
pub mod llm;
pub mod mask;
pub mod polish;
pub mod records;
pub mod style;
