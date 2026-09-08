# stacked-pr スキル

`config/claude/skills/stacked-pr/SKILL.md` — PR 同士に依存関係があるとき
(先行 PR の成果物を後続が参照する、または同一ファイルの同じ節を逐次編集する)、
main 起点で並行させず base を親ブランチにした stacked PR として積む手順を持つ
スキル。global `CLAUDE.md` に短い引き金の定義を持ち、詳細はこのスキルに委ねる
分担(ADR-0008 のルール 3 に従う: hook/skill の living な設計根拠は
`docs/claude/<name>.md`)。

腐る事実(GitHub ネイティブ stack 機能の preview ステータス、`gh-stack` 拡張の
open issue、バージョン番号)は `docs/stacked-pr-github-native.md` に切り出した
(ADR-0008 のルール 1)。このファイルには**裁定とその理由だけ**を書く。

## 動機

「commit → push → `gh pr create` を問答無用で一続きに実行する」という既存の
完了定義(global `CLAUDE.md`)は、PR 同士に依存関係が無い前提で書かれていた。
実際には次の 4 つが同時に起きていた:

1. 依存を無視して main 起点で並行し、同一ファイル(ADR、単一設定ファイル)を
   触る PR が衝突・上書きする
2. 親 PR のマージ待ちで子作業が始められず、セッションが空転する
3. 分けられないので ADR + 実装 + docs を 1 PR に詰め、レビュー不能な塊になる
4. 「これは分けるべきか」の判断がセッションごとにブレる

PR 同士の依存関係は Issue 同士の依存関係とは別問題 — 1 つの Issue が複数段の
stack になることもあり、複数の独立 Issue が 1 つの線形 stack になることもある。
この区別を規律の前提として明記した。

## なぜ素の `--base` + `gh stack link` を選び、`init/submit/sync` を避けたか

2026-07-30 に GitHub 本体へ public preview で入った Stacked pull requests
機能には、ローカルのブランチ構造を丸ごと管理する `gh stack init/add/submit/
sync` 系コマンドと、既存 PR を GitHub 上でリンクするだけの `gh stack link`
がある。前者を採用すると、`gh-stack` 拡張の既知のバグ(既存ブランチを trunk
から再作成して force-push で履歴を破壊する、sync のたびに force-push して
レビュー履歴を壊す)を踏むリスクを、このリポジトリの通常の PR フローに
持ち込むことになる。

`gh stack link` はローカルの追跡状態を作らず、GitHub 上で既存 PR 同士を
リンクするだけなので、このリスクを踏まずに stack map・all-or-nothing マージ・
CI/branch protection の継承といった public preview の利点だけを得られる。
ブランチの作成・rebase・push は、このリポジトリが既に持っている通常の git
操作(`git switch -c`、`rebase.updateRefs`、`--force-with-lease`)で行う。

preview が GA したときにこの裁定を見直すかどうかは、
`docs/stacked-pr-github-native.md` の再検証すべきこと節で追跡する。

## なぜトリガー (b) を維持し、「同じ行を触らない」で補ったか

このリポジトリは squash-merge のみ許可しており、`allow_merge_commit` /
`allow_rebase_merge` はいずれも無効。git-town は「複数の stack 内ブランチが
同じ行を触り、かつ squash-merge を使うと、実際には衝突していないのに衝突が
報告される(phantom conflict)」と明示的に警告しており、この条件をマージ
方式の変更で回避することはできない。

トリガー (b)(同一ファイルの同じ節を逐次編集する)自体を落とす選択肢も
検討したが、これは今回そもそも stacked PR 規律を必要とした具体的な事故
(同一 ADR / 単一設定ファイルへの同時編集による衝突)を正面から扱う条件
なので落とせない。代わりに「同じ行を 2 段以上が書き換える切り方はしない
(追記なら可)」という切り方の規律を追加し、既に有効な `rerere.enabled =
true` で残りを吸収する形にした。

## なぜ pr-gate.sh を触らなかったか

`pr-gate.sh` は既に PR の `baseRefName` を見て動作しており、`G_link` に
stacked 用の advisory(base が default branch でないときは closing keyword
が発火しない旨を注記)、`G_CI` に quiesce フォールバック(stacked PR で
required check が 0 件のときの縮退経路)を持つ。つまり stacked PR で完全に
沈黙するわけではない。

「親のコミットを含むのに base が default branch」という取り違えを機械的に
block する案(`G_stack`)も検討したが、`pr-gate.sh` の既存の設計原則
(「判定できる場合だけ踏み込む」、`docs/claude/pr-gate.md` 参照)に倣い、
今回は指示文とスキルの運用を先に確立し、実際に取り違えが起きてから block
化を検討することにした。誘発の実測が無いまま `MAX_BLOCKS` を引き上げて
PR を大きくする判断はしない。

## `Stack:` 行を `pr-description` のスケルトンに追加した理由

stack の位置(第 k/N 段、親は #M)は GitHub の PR 画面が base branch として
表示するので、本来は省略できる。しかし親 PR がマージされて auto-retarget
された後は、UI からも stack だった履歴が消える。1 行のコストで
`Stack: <段番号>/<総段数> (base: #<親PR番号>)` を残すことで、レビュー時と
マージ後の両方で stack の全体像が本文だけから読み取れるようにした。
`pr-gate.sh` はこの行を検査しない(人間とレビュアーのための注記であり、
機械強制の対象ではない)。

## `stacked-pr` と `pr-description` / ADR-0008 との関係

`pr-description` は PR を出す**瞬間**の本文の型と証跡要件に特化し、
`stacked-pr` は複数の PR を**どう分割し、どう順序立てて出すか**に特化する。
接続点は `Stack:` 行(§7)と Issue リンクの書き分け(§6)の 2 箇所のみで、
それ以外の本文要素(課題・解決策・Before/After・検証・要確認)は
`pr-description` の規律がそのまま適用される。

ADR-0008(記録の器の選択規約)はこのスキル自身の執筆にも適用した —
腐る事実を `docs/stacked-pr-github-native.md` に追い出し、この文書には
裁定とその理由だけを残した。
