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
5. grilling セッションや Plan モード中にスコープ外だが価値のある発見が
   あったとき、選択肢空間に「stack の一段として受ける」が無いため、
   「wrap-up inbox 送り」か「同一 PR に混ぜる」かという極端な二択を
   `AskUserQuestion` で迫ってしまう(2026-09-19 の grilling セッションで
   観測)

PR 同士の依存関係は Issue 同士の依存関係とは別問題 — 1 つの Issue が複数段の
stack になることもあり、複数の独立 Issue が 1 つの線形 stack になることもある。
この区別を規律の前提として明記した。

## スコープ外発見を stack の一段として受ける入口(2026-09-19)

上記 5 の観測を受けて、判定条件(§1)の対象を「依頼された変更同士」から
「現在進行中の変更と、計画・グリル・実装中に見つかったスコープ外項目」に
広げた。決定ルールは単純な条件分岐にした:現在の変更との間で判定条件
(a)/(b) を満たせば stacked PR の追加提案段として受け、満たさなければ
wrap-up inbox へ流す(global CLAUDE.md「複数項目の依頼は要求インベントリで
受ける」節)。この分岐自体はモデルに聞かず機械的に決める — 聞くのは
「今のセッションでやるか」(Plan 中は分割案への追加提案として一括承認、
実装中の発見のみ `AskUserQuestion` で stack か inbox かの 2 択)だけに絞る。

「同一 PR に混ぜる」は選択肢から落とすが、レビュー負荷が事実上ゼロの微小
修正(通りかかったファイルの typo・dead link 修正等、数行)に限り例外として
残した。Google の "Small CLs" ガイド(eng-practices,
https://google.github.io/eng-practices/review/developer/small-cls.html、
取得 2026-09-19)も同旨で、"It's usually best to do refactorings in a
separate CL from feature changes or bug fixes." としつつ "Small cleanups
such as fixing a local variable name can be included inside of a feature
change or bug fix CL, though." と裁量の余地を残している。ただし同文書は
同梱を裁量に委ねるだけなのに対し、ここでは同梱時に PR 本文で一言断る義務を
追加した — 後からレビュー・監査する側が「意図した同梱」と「スコープの
なし崩し的な混入」を区別できるようにするため。

機械 gate(`plan-scope-gate.sh` / `pr-gate.sh` への検査追加)は今回作らない。
`AskUserQuestion` の選択肢空間は機械検査に向かないうえ、§8「なぜ
pr-gate.sh を触らないか」の既存裁定(判定できる場合だけ踏み込む、実測が
出てから block 化を検討する)にそのまま従う。

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

## なぜ pr-gate.sh を触らなかったか(2026-09-21 に ADR-0027 が上書き)

**この節の裁定は ADR-0027 によって上書きされた。** 以下は当時の記録として
残すが、現在の規律は「保留条項の発火と ADR-0027」節を参照。

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

## 保留条項の発火と ADR-0027(2026-09-21)

上記の保留条項がまさに発火した。ある private リポジトリでの開発セッション
(どのリポジトリかは ADR-0014 の方針により記さない — private/company
リポジトリ名は dotfiles の成果物に残さない)で 1 セッション約 10 PR を
作成した際、ブランチは物理的に直列に積まれていたのに base 宣言が
不整合になった:

- ある PR は先行 PR の head を base にすべきところ default branch の
  ままで、他の複数 open PR のコミットを含む汚染 diff になっていた。
- `gh stack link` が未実行のまま Web UI で手動 stack を試み、束ねきれない
  orphan PR が発生した。
- 並列 2 チェーン + 独立 PR 1 本に分裂した。

判定条件 (a)/(b) に基づく依存予測は LLM 判断に委ねられており、セッション中
に系統的に外れた。`ADR-0027`(uncertainty-first stacking)は、この予測を
「積むか否か」の判定からは廃止し、セッション内の複数 PR は常に作成順の
単一チェーンに積むことを、作成時 PreToolUse hook(`stack-base-guard.sh`)と
完了時 Stop judgement(`G_stack`、`pr-gate.sh`)の両端で機械強制する決定を
下した。詳細な設計根拠は ADR-0027 本文および `docs/claude/
stack-base-guard.md` / `docs/claude/pr-gate.md` を参照。

判定条件 (a)/(b) 自体は「段の切り方(何を 1 段にまとめるか)」の設計原則
としては §2 に存続する。廃止したのは「積むかどうか」を予測で分岐する
判断だけである。当該 private リポジトリの現行 PR 群の修復は本ドキュメントの
スコープ外(private リポジトリ側の運用として個別に対応する)。

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
