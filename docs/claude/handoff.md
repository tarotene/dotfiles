# handoff — 中断ハンドオフを Human / AI 双方に起票する仕組みの設計記録

作業を途中で打ち切りたいとき、残タスクを揮発させずに再開可能な状態へ
落とし込む正規の経路が無かった。`docs/claude/scope-inventory.md` は
「実行段階の途中打ち切り」を明示的にスコープ外にしており、
`pr-gate.sh` は途中状態の PR でも `G_link`(closing keyword 必須)・
`G_CI`(CI green 必須)で Stop を block し続け、抜け道は全ゲートを
外す skip ファイルしか無かった。実装は `config/claude/skills/handoff/`
(手順は `SKILL.md`、決定論的サブコマンドは `scripts/handoff.sh`)、
機構側の変更は `config/claude/hooks/pr-gate.sh`(中断ハンドオフ節、
`docs/claude/pr-gate.md` 参照)と `crates/issue-index`
(着手可能な `handoff:ai` 節、`docs/claude/issue-index.md` 参照)。

## 決定表(/grill-me で確定)

| 論点 | 決定 |
|---|---|
| 場面 | 中断 → 後で再開するためのハンドオフ(棚上げ記録ではない) |
| 発動 | ユーザーの指示だけ。AI からは提案しない |
| WIP | commit → push → Draft PR。途中状態のスナップショットは Draft PR 本文 |
| pr-gate | Draft + 本文に `Handoff: #N`(N が open Issue)のときだけ G_CI を advisory に、G_link の充足とみなす |
| Issue 構成 | 1 タスク = 1 Issue。親は元 Issue、無ければ新設(tracking-issue 規約) |
| 担当の表現 | 閉語彙ラベル `handoff:human` / `handoff:ai`。Human 側は @me に assign |
| 順序 | GitHub ネイティブの Issue dependencies(blocked_by) |
| Human 本文 | 最小固定節: やること / なぜ人手か / 手順 / 完了条件 |
| 再開の入口 | issue-index に「着手可能な handoff:ai」行 |
| 起票前確認 | なし(ラベル新設の初回確認だけ例外) |
| 適用範囲 | 全リポジトリ |

## 先行例との対比

- D1: 残タスクと再開地点の正本を、リポジトリ内の進捗ファイルではなく GitHub の Issue + Draft PR に置く
  先行例: Justin Young (Anthropic), "Effective harnesses for long-running agents", 2025-11-26 https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents (取得 2026-09-27)
  差分: 異なる — 先行例は claude-progress.txt + git commit + 機能リスト JSON で「AI だけが次のセッションで読む」前提。今回は Human も読み、通知・assign が要り、会社リポジトリに未追跡ファイルを生やさない方針(`docs/claude/wrapup-inbox.md`)もあるため、GitHub 上に置く。「git + 残作業リスト + 状態スナップショット」という三つ組の構造は踏襲する
  軸: 還元 — 通知・担当・依存を GitHub の既存機能で担い、独自ファイル形式を新設しない
- D2: WIP は Draft PR として出す
  先行例: GitHub Docs, "Pull requests"(Draft pull requests 節)https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/about-pull-requests (取得 2026-09-27)
  差分: 一致 — Draft は merge 不可で、`gh pr ready` で解除する
  軸: 表現不可能 — 未完成の PR が merge される状態を GitHub 側が宣言的に禁じる
- D3: pr-gate は「Draft かつ `Handoff: #N`(N が open Issue)」のときだけ G_CI を advisory に落とし、G_link の充足とみなす(Closes は再開後、その PR で完了する Issue にだけ書く)
  先行例: `docs/claude/pr-gate.md`(`No-Issue:` / `No-Visual:` の閉じたタグによる escape hatch)(取得 2026-09-27)
  差分: 異なる — 既存のタグは本文の文字列だけで成立する。こちらは Draft 状態と参照先 Issue の実在という 2 つの外部状態を併せて検証し、「Draft にすれば CI を素通り」という抜け道を塞ぐ。判定不能なら緩めない
  軸: 検出のみ — 本文は自由記述で、Stop 時の事後検査でしか扱えない
- D4: 担当は閉語彙ラベル `handoff:human` / `handoff:ai` で表し、Human 側は @me に assign する
  先行例: `docs/claude/wrapup-inbox.md`(存在しないラベルは `gh issue create` を落とすので、ラベルを使わない判断)(取得 2026-09-27)
  差分: 異なる — 事前にラベルの存在を確認し、無ければ新設するサブコマンド(`handoff.sh labels-missing` / `create-labels`)を持つことで、その失敗様式を回避したうえでラベルを使う。検索フッターではなくラベルで引けることが issue-index 行(D7)の前提になる
  軸: 表現不可能 — 閉語彙 > 自由記述(タイトル接頭辞案を退けた理由)
- D5: タスク間の順序は GitHub ネイティブの Issue dependencies(blocked_by)で張る
  先行例: GitHub Docs, "REST API endpoints for issue dependencies" https://docs.github.com/en/rest/issues/issue-dependencies (取得 2026-09-27)
  差分: 一致 — POST `.../dependencies/blocked_by` に `issue_id`(number ではない)を渡す。取り違えやすいので `handoff.sh block` に寄せた。上限件数は文書に記載なし
  軸: 表現不可能 — 宣言 > 手続き(本文の `Blocked by #N` をパースする案を退けた)
- D6: 1 タスク = 1 Issue とし、親(元 Issue、無ければ新設)に native sub-issue で紐付ける
  先行例: `config/claude/skills/tracking-issue/SKILL.md`(sub-issues を正本にし、チェックリストを禁止)(取得 2026-09-27)
  差分: 一致
  軸: 表現不可能 — 単一正本 > 複写+同期
- D7: 後続の AI の再開入口は、issue-index に「着手可能な handoff:ai」行を足すことで作る
  先行例: `docs/claude/issue-index.md`(静的な指示は CLAUDE.md、現在状態は hook で運ぶ)(取得 2026-09-27)
  差分: 一致 — ただし @me の枠とは独立に出す(@me が 1 件以上あると全体一覧が消える既存挙動のため)
  軸: 還元 — 新しい hook を足さず、既存の SessionStart hook に行を足す
- D8: 手順の器は skill にし、誤りやすい API 操作(ラベル確認・作成、blocked_by)だけを selftest 付きの script に寄せる
  先行例: `docs/claude/external-call-scheduling.md`(「器をスキルにした理由」: 判断を要し、発動条件がキーワードで言い切れる)と `docs/claude/wrapup-inbox.md`(リスクのある操作を決定論的サブコマンドに寄せる)(取得 2026-09-27)
  差分: 一致
  軸: 還元 — 担当の振り分けは判断なので hook では担えない。API 操作は script にして LLM の自由操作を減らす
- D9: 起票前の確認を挟まず一気に実行する(例外はラベル新設の初回確認だけ)
  先行例: `config/agents/AGENTS.md`「外部発信は既定で下書き止まりにする」(GitHub は対象外で、訂正はレビュープロセスに委ねる)(取得 2026-09-27)
  差分: 一致 — ラベルの新設だけは、共有リポジトリの他の人に見える設定変更なので確認する
  軸: 還元 — 確認ステップを足さない
- D10: Human 担当の判定を閉語彙(GUI・認証・物理・判断・外部連絡)で行う
  先行例なし: リポジトリ内の Issue ラベル・docs を needs-human / handoff / 人手 / 中断 / 残タスクで検索、#505 の本文、`docs/cutover-runbook.md` を確認した。機械実行と human-only を書き分ける共通書式は #505 で未決定。外部の一次情報は探していない
  軸: 表現不可能 — 閉語彙 > 自由記述。#505 で標準化されたら追従する

## `scope-inventory` との関係

`docs/claude/scope-inventory.md` の「対象外: 実行段階の途中打ち切り」は、
AI が自発的に手を止める失敗様式(見積り膨張・判断の丸投げ)を指す。
ユーザーの明示的な指示による中断はこの skill が担い、両者は別の失敗様式
なので区別する。

## `#505` との関係

`#505`(クラウド上の成果物命名規則と人手を要する runbook 書式の横断
標準化)は参考として引用しただけで、この機能の実装対象ではない
(`Reference-Only`)。Human 担当子 Issue の本文は、決着を待たずに使える
最小固定節(§SKILL.md 5 節)を先行させ、`#505` が標準化されたら追従する。

## 検証

- `bash config/claude/skills/handoff/scripts/handoff.sh --selftest`
- `bash config/claude/hooks/pr-gate.sh --selftest`(中断ハンドオフ節)
- `nix develop --command cargo test -p issue-index`(着手可能な handoff:ai 節)
