//! pr-gate — PR の完了を待つ Stop hook(+ 現在状態を運ぶ SessionStart hook)。
//! 旧 `config/claude/hooks/pr-gate.sh` の移植(ADR-0024 Stage 4a、#415)。
//!
//! 設計と根拠: docs/claude/pr-gate.md(このリポジトリ内)
//!
//! 「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」「PR を Issue に
//! 繋がないまま終わる」「見た目の変更なのに視覚証跡が無いまま終わる」「push 済み
//! なのに PR を作らないまま終わる」という 5 種の事故を、Stop の 1 点だけで
//! hard gate する。base 鮮度・未コミット変更は advisory(block するときだけ
//! 相乗りで伝える。それ単独では終了を止めない)。
//!
//! ```text
//!   G_push     : ローカル HEAD == PR の headRefOid              → block
//!   G_unpushed : PR が無いときの「push すべきコミットが 0 件か」→ block
//!   G_pr       : push 済み・PR 無し・ahead>0 のブランチ         → block
//!   G_link     : PR 本文に closing keyword または No-Issue:     → block
//!   G_visual   : PR 本文に Before/After の視覚証跡 or No-Visual: → block
//!   G_stack    : stacked PR チェーンが GitHub 上の stack に未リンク → block
//!   G_prior    : 新設した「新しい道具・単位」に既存手段: 記載が無い → block
//!   G_CI       : 期待される check がすべて pass/skipping        → block
//!   G_base     : origin/<base> に対する ahead/behind            → advisory
//!   G_wt       : 未コミット件数                                 → advisory
//! ```
//!
//! 中断ハンドオフ(docs/claude/pr-gate.md「中断ハンドオフ」節、handoff skill):
//! Draft PR かつ本文に `Handoff: #N`(N が open Issue)があるときだけ、G_link の
//! MISSING と G_CI の非 PASS を block ではなく advisory に落とす
//! ([`gates::handoff`])。判定不能なら緩めない(fail-closed)。
//!
//! G_unpushed は G_push が届かない領域を塞ぐ: G_push は比較対象の headRefOid を
//! open PR から取るので、PR がまだ無いセッションでは何も見ずに完全沈黙していた
//! (実測: コミットを 3 つ積んで push せずに終わっても pr-gate は無言だった)。
//! push の事実だけを見て、その先の「PR を作るべきか」は G_pr が判定する。
//!
//! G_pr は G_unpushed のさらに先を塞ぐ: push 済みなら G_unpushed は何も言わず、
//! 以前はここで完全沈黙していた(実測: push だけして PR を作らずに終わっても
//! 無言だった)。判定できる場合だけ block し、判定できない場合(upstream 不明、
//! origin/HEAD 未設定、default branch 上、ahead 不明、merged/closed PR が既に
//! ある)は断定に変えず素通す。
//!
//! SessionStart は「PR が無ければ完全沈黙」だったため、base が何コミット遅れて
//! いても `[gone]` ブランチが何本溜まっていても一切表示されなかった。PR の有無を
//! 問わず hygiene 行([`repo::stale_base_line`] ほか)を計算し、材料があれば単独で
//! advisory を出す(SessionStart 自体には block/pass の概念が無いので、単独発火の
//! コストは無い)。
//!
//! 発火範囲: allowlist ファイル(既定 ~/.claude/pr-gate-repos)に nwo が無ければ
//! 完全沈黙。全ホストに無条件配備する前提(会社リポジトリでは既定で沈黙する)。
//!
//! 縮退:
//! - 完全沈黙(exit 0, 出力なし) → allowlist 外 / git repo でない / GitHub remote
//!   でない / gh・git 不在 / skip / (PR が無く、かつ hygiene の材料も無い)
//!   / G_pr の判定不能条件 / G_stack の chain size が 1(stacked PR ではない)
//! - 警告 1 行 + fail-open → gh 未ログイン・API 失敗・fetch 失敗・
//!   G_pr の merged/closed 検査 API 失敗・G_stack の gh-stack 拡張不在
//!   または stacks API 取得不能(advisory 降格)
//! - hard block(exit 2) → G_push 不一致 / G_unpushed / G_pr / G_link 欠落 /
//!   G_visual 欠落 / G_stack(chain 2 以上・未リンク・拡張/API 利用可能) /
//!   G_prior / G_CI が揃わない・失敗・pending(ただし中断ハンドオフ成立時は
//!   G_link 欠落 / G_CI 不揃いを advisory に降格)
//!
//! `stop_hook_active` は見ない。wrapup-stop-gate と同じ即 exit 0 にすると、
//! G_push で 1 回 block した直後の再呼び出しが CI 判定に到達しない
//! (docs/claude/copilot-plan-review.md の「第二次の非収束」と同型)。上限は独自
//! カウンタ([`state`])。
//!
//! bash 版は jq で JSON を読み書きしていた。Rust 版は jq に依存しない
//! (serde_json で読み、SessionStart の出力は [`hook_io::jqfmt`] で jq の整形出力と
//! バイト一致させる)。
//!
//! エスケープハッチ: `touch ~/.claude/pr-gate/skip` または `SKIP_PR_GATE=1`。

pub mod body;
pub mod config;
pub mod gates;
pub mod gh;
pub mod jqv;
pub mod repo;
pub mod session_start;
pub mod state;
pub mod stop;

/// bash の `$(...)` と同じく、末尾の改行をすべて落とす。
pub fn trim_nl(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// 改行区切りで連結する(空文字の要素は無視)。bash の `join_hygiene_lines`。
pub fn join_lines<S: AsRef<str>>(lines: &[S]) -> String {
    lines
        .iter()
        .map(AsRef::as_ref)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
