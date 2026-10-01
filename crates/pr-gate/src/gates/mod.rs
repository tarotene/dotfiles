//! 外部状態(gh / git / new-tool-guard)を引く判定。本文だけで決まる判定は
//! [`crate::body`] にある。

pub mod ci;
pub mod handoff;
pub mod prior;
pub mod stack;
