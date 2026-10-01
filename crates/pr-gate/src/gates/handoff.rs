//! 中断ハンドオフ(`Handoff: #N`): G_link / G_CI の緩和条件。
//!
//! handoff skill(docs/claude/handoff.md)が作る WIP の Draft PR は、まだ
//! closing keyword を書けない(親 Issue に `Closes` を書くと、再開後に子の
//! 一部だけ終えてマージしたときに親まで閉じてしまう — tracking-issue の
//! 「全 sub-issue closed で親を閉じる」条件に反する)。かつ CI が赤/pending
//! のまま中断することが普通にある。
//!
//! `isDraft == true` かつ本文に `Handoff: #N`(N が open な Issue)があるとき
//! だけ「Handoff 成立」とみなし、G_link は LINKED 相当・G_CI の block は
//! advisory に緩める。判定に使う外部状態(isDraft・Issue の open/closed)が
//! 取得できないときは、緩めずに従来どおりの判定に落とす(fail-closed —
//! 全ゲートを外す skip ファイルの代替として使われないため)。

use crate::{body, gh};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// 成立。Issue 番号を持つ。
    Ok(String),
    /// Handoff 行が無い。
    None,
    /// Handoff 行はあるが、Issue が open と確認できない(closed・API 失敗)。
    Unverified,
}

pub fn judge(body_text: &str, nwo: &str) -> Handoff {
    let Some(num) = body::handoff_issue(body_text) else {
        return Handoff::None;
    };
    match gh::issue_state(&num, nwo) {
        Some(s) if s == "OPEN" => Handoff::Ok(num),
        _ => Handoff::Unverified,
    }
}
