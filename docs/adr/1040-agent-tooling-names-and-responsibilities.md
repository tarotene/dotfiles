# ADR-1040 — エージェント向けツールの命名・責務・共有配置

- Status: Accepted
- Date: 2026-10-26

## Context

共有ツールが Claude 固有のディレクトリに集まり、集約ライブラリと終了処理の名前が責務を説明しきれなくなった。
発火イベントで guard/gate を区別する ADR-0007 の規則も、複数イベントで同じ保護を担う実装と一致しない。

## Decision

名前は責務を端的に説明する。guard は禁止操作・状態からの保護、gate は条件成立までの進行停止、
check は適合性判定とする。これは本リポジトリの語彙とし、品詞を一律に揃えない。
[命名・配置規約](../agents/tooling-conventions.md)と[移行一覧](../agents/tooling-migration.md)を適用する。
共有正本は config/agents・docs/agents、共有配布宣言は home/modules/agents.nix に置く。
製品固有の設定と説明は製品別に残す。

hook-io と guard-core は責務別の七つのライブラリに分割する。
終了処理は issue-inbox・session-retrospective・feedback-trial と session-wrapup に分ける。
Git 操作は git-repository が所有し、各入口がブランチ・fetch 条件・通知・タイムアウト方針を保持する。
新しい実装言語や外部依存を導入しない。

互換 CLI は同じ実装へ転送し、互換 skill は正準の全文を読む入口にする。
状態パス・環境変数・記録形式・承認・マーカーを維持する。
旧入口は全ホストへの適用・旧セッション終了・現行参照ゼロ・互換なし検証の四条件が揃った別 PR で削除する。
規約・配布・実装・検査は一本の PR に揃える。

## 先行例との対比

- D1: 責務を短く説明する名前と七ライブラリを採る。出典: Sameer Ajmani, “Package names”, 2015-02-04, https://go.dev/blog/package-names ／ Google, “C++ Style Guide — Choosing Names”, 公開日記載なし, https://google.github.io/styleguide/cppguide.html#Choosing_Names （取得 2026-10-26）。差分: 考え方は一致。言語固有の綴り規則は移植しない。既存手段: 拡張 — 既存 Rust 実装を再配置する。軸: 還元。
- D2: guard/gate/check を責務で区別する。出典: dotfiles, ADR-0007 と main-checkout-guard の pre/stop 実装（取得 2026-10-26）。差分: 発火時点による分類を責務による分類へ変える。軸: 還元。
- D3: 共有正本と製品別配布を分ける。出典: dotfiles, ADR-0032・ADR-872（取得 2026-10-26）。差分: 既存の正本からの配布と登録検査を共有ディレクトリへ広げる。既存手段: 拡張 — Nix と pipeline-map。軸: 表現不可能。
- D4: 終了処理を分割し、順序と早期終了を維持する。出典: dotfiles, 5d499a9 の wrapup-stop-gate/src/lib.rs:869-1127 と retro.rs（取得 2026-10-26）。差分: 独立した責務を別 CLI として公開する。既存手段: 拡張 — 既存の逐次実装。軸: 還元。
- D5: 互換入口を残す。出典: Agent Skills, “Specification — name field”, 公開日記載なし, https://agentskills.io/specification#name-field ／ dotfiles の hook 所有記録（取得 2026-10-26）。差分: 同じ手順本文への転送を加える。既存手段: 拡張 — home.file と reconcile。軸: 還元。
- D6: 責務外の手順を共有参照へ移す。出典: dotfiles, browser-login-handoff・repo-governance-common（取得 2026-10-26）。差分: 必要な場面で必ず読む導線と、同一 source の配布を加える。既存手段: 拡張 — 共有参照の配布。軸: 還元。
- D7: 一本の PR に揃える。出典: dotfiles, stacked-pr §2 と pipeline-map.toml（取得 2026-10-26）。差分: 一致。同じ登録行の段を跨ぐ再編集を避ける。軸: 還元。
- D8: Git 操作と適用条件を分離する。出典: dotfiles, 5d499a9 の plan-fresh-gate・git-checkout-freshness・worktree-fresh-base（取得 2026-10-26）。差分: 既存の条件を保ち、操作を共有する。既存手段: 拡張 — 旧 hook-io/src/git.rs。軸: 還元。

内部出典の版: tarotene/dotfiles, 5d499a9a06d5c4c4e3c76ec870f5760be57164ab,
https://github.com/tarotene/dotfiles/tree/5d499a9a06d5c4c4e3c76ec870f5760be57164ab （取得 2026-10-26）。

## 執行点

- home/modules/agents.nix
- home/common.nix
- crates/pipeline-map/tests/coverage.rs

## Consequences

配布元と実装の責務を名前から追える。名前の意味はレビューで判断し、構造と参照の整合性は既存検査で強制する。
移行中の旧入口は残るが、実装・手順本文・永続状態は複製しない。

## Amendment (2026-11-09 — hook の逃げ道の環境変数は agent-session が名前と真偽を決める閉語彙にする, #1437)

Decision の「状態パス・環境変数・記録形式・承認・マーカーを維持する」のうち、**環境変数**の維持を改める。
逃げ道(guard・gate を一時的に止める手段)が 21 件あり、綴りが 3 種類と逆向き 1 件(`SKIP_X` が 10、`X_ALLOW` が 8、
`X_BYPASS` が 2、`X_SKIP` が 1)に割れ、読む場所(プロセス環境、コマンドの語、skip ファイル)、真の条件
(`1` ちょうどと「空でない」)、拒否文の案内も guard ごとに違った。実行名の改名に環境変数が追従していない例
(`pr-confirm-guard` の改名後も `PR_CONFIRM_GUARD_ALLOW`)もあった。

### D9: 逃げ道は、`agent_session::escape` の表が名前・スコープ・真偽・案内を決める

- スコープは 2 種類だけ。**呼び出し単位** `<STEM>_ALLOW=1` は、Bash の 1 回の呼び出しのコマンドにある引用外の語。
  **セッション単位** `SKIP_<STEM>=1` は、hook プロセスの環境変数(skip ファイルを持つ guard は、その存在も同じ意味)。
  `STEM` は実行名を大文字と `_` にしたもの。
- 真は `1` だけ。`BYPASS` の綴りは廃止する。
- どの guard がどのスコープを持つか、案内を出すかは、`escape::ALL` の表が唯一の正本。他の crate は表の項目を作れない。
  外部への送信など、エージェントが自分で外せてはならないものは、案内を出さない。
- 旧名は別名として残さない(参照元を同じ段で直した)。外向きの名前の変更を許す前提は、本人の裁定(2026-11-09)。
- 維持するもの(変えない): skip ファイルの置き場所、状態パス、記録形式、承認、マーカー、gate-event と directive の識別子。
  逃げ道でない環境変数(`*_DIR`、`WORKTREE_FRESH_BASE_FETCH_TTL`)の維持の凍結はやめ、変えるときは「参照元を同じ変更で直す」に従う。
- bash の git hook の `GIT_ALLOW_MAIN_COMMIT` は、この表を呼べない層なので範囲外(名前を変えない)。

手動モード(`--check`)の出力と終了コードは、`agent_session::cli` が 1 つにそろえる(`pass` / `deny: …`、終了コード 0・1・2)。

### 執行点

- crates/agent-session/src/escape.rs
- crates/agent-session/src/cli.rs
