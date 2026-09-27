---
name: handoff
description: ユーザーの指示で作業を途中で打ち切るとき、残タスクを Human / AI 双方に振り分けて Issue に起票し、後で再開できる状態にする手順。中断・打ち切り・ここまでで止める・続きは後で・引き継ぎ・handoff・今日はここまで、といった文脈で使う。stop here and resume later, hand off remaining work, pause this task, wrap up for now、といった英語の文脈でも使う。セッション長やコンテキスト残量を理由に AI が自発的に提案することはない(scope-inventory スキルの「実行段階の途中打ち切り」と同じ理由)— ユーザーの明示的な指示があるときだけ発火する。
---

作業を途中で打ち切るとき、残タスクを揮発させず、Human / AI 双方が後で
拾える形で GitHub に残す。設計判断の全体像(なぜ Draft PR か・なぜ
GitHub ネイティブの機能に寄せるか)は `docs/claude/handoff.md` を参照。

確認は挟まない。棚卸しから起票まで一気に実行し、最後に一覧を報告する
(GitHub への投稿は下書き止まりの対象外 — 共有 AGENTS.md「外部発信は
既定で下書き止まりにする」の GitHub 除外)。例外はラベルの新設(§3)だけ。

## 1. 棚卸し

現在の状態を確認する:

- 現ブランチ、既存の open PR(あれば)、元になった Issue(あれば #M)、
  stacked PR チェーンでの位置。
- 残タスクを列挙し、それぞれの担当を決める。**なぜ人手かの閉語彙**
  (GUI 操作・認証・物理作業・判断・外部連絡)のどれかに当たれば
  Human、それ以外は AI。曖昧なら Human 側に倒す(AI が「たぶんできる」
  と過信して着手不能なタスクを AI 側に残すより安全)。

## 2. WIP を Draft PR にする

差分または未 push の commit があれば:

```
git add -A && git commit -m "..."
git push
```

既存 PR が無ければ Draft PR を作る(既存 PR があれば `gh pr ready --undo`
で Draft に戻す)。stack の途中なら **最上段だけ**を対象にする。

本文は `pr-description` スキルの 5 節スケルトン + 次の 2 点:

- `Closes #P` は書かない(親 Issue はこの PR では閉じない。書くと、
  再開後に子タスクの一部だけ終えてマージしたとき親まで閉じてしまう)。
- 1 行目に `Handoff: #P`(P は §4 で作る親 Issue の番号)を書く。
  `pr-gate.sh` はこの行と Draft 状態が揃っているときだけ、closing
  keyword 省略と CI 未完了を advisory に緩める(詳細:
  `docs/claude/pr-gate.md`「中断ハンドオフ」節)。

差分が無い(調査だけのセッション)なら、この節は丸ごとスキップして
Issue の起票だけ行う。

## 3. ラベルの確認

```
bash config/claude/skills/handoff/scripts/handoff.sh labels-missing <owner/repo>
```

`handoff:human` / `handoff:ai` のうち欠けているものが出力される。1 つでも
出力されたら、そのリポジトリで初回だけ `AskUserQuestion` でラベル新設を
確認する(共有リポジトリの他の人にも見える設定変更のため、ここだけは
確認を挟む)。承認されたら:

```
bash config/claude/skills/handoff/scripts/handoff.sh create-labels <owner/repo> handoff:human handoff:ai
```

拒否されたら、その旨を報告してこのリポジトリでの中断ハンドオフを打ち切る
(ラベル無しで `gh issue create --label` すると失敗するため、黙って
ラベル無しで進めない)。

## 4. 親 Issue

元になった Issue #M があればそれを親として使う。無ければ `tracking-issue`
スキルの書式で新設する(子が 2 件以上あれば `tracking` ラベルも付ける)。

## 5. 子 Issue

残タスク 1 件 = 子 Issue 1 件。`gh issue create --parent <親番号>` で
起票する(親が `tracking-issue` の子として native sub-issue を使う理由と
同じ)。

**Human 担当**(`handoff:human` ラベル、assignee は `@me`):

```
## やること
<1〜2文>

## なぜ人手か
GUI 操作 | 認証 | 物理作業 | 判断 | 外部連絡 のいずれか + 一言

## 手順
<箇条書き>

## 完了条件
<この Issue を close してよい条件>
```

将来 #505(人手を要する runbook 書式の横断標準化)が決着したら、この
最小テンプレを追従させる。

**AI 担当**(`handoff:ai` ラベル):

```
## 目的
<1〜2文>

## 再開地点
Draft PR: #<番号>(ブランチ: <branch名>)

## 次の一手
<具体的な次の 1 アクション>

## 完了条件
<この Issue を close してよい条件>
```

どちらも生成元フッター(`config/claude/CLAUDE.md`「GitHub 投稿の生成元
明示」)を付ける。

## 6. 依存を張る

ある子が別の子(または Human の判断)を待つ場合、GitHub ネイティブの
Issue dependencies で表す:

```
bash config/claude/skills/handoff/scripts/handoff.sh block <owner/repo> <blocked番号> <blocker番号>
```

典型例: 「AI タスクは Human タスクの完了(close)を待つ」なら、AI 側の
Issue 番号を `<blocked番号>`、Human 側を `<blocker番号>` にする。

## 7. 時間帯制約のある Human タスク

電話・窓口対応など営業時間の制約があるものは、Issue を起票するだけで
終わらせず `external-call-scheduling` スキルにも回し、カレンダーへの
反映まで行う。

## 8. 報告

担当(Human/AI)・依存関係・Issue と Draft PR の URL 一覧を最後にまとめて
報告する。

## 再開

`handoff:ai` の子 Issue を着手するとき(issue-index の SessionStart 注入
「着手可能な handoff:ai」節、または `#N を再開して` という指示から):

1. 親 Issue と Draft PR の本文を読み、次の一手を確認する。
2. `gh pr checkout <番号>` で Draft PR のブランチに乗る。
3. PR 本文の `Handoff: #P` 行を、**この PR で完了する子 Issue だけ**の
   `Closes #…` 行に置き換える(fenced code block やインラインコードの
   中に書かない — `pr-gate.sh` の `G_link` は GitHub の解釈と同じ判定を
   する)。親 #P への `Closes` は、この merge で親の全 sub-issue が
   closed になり、かつ親の完了定義も満たすときだけ追加で書く
   (`tracking-issue` スキルのクローズ条件)。
4. 作業を続け、CI を通す。
5. `gh pr ready` で Draft を解除する。以降は通常どおり `pr-gate.sh` の
   全ゲートが効く。
