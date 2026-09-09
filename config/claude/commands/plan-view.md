---
description: 執筆中のプランを Chrome の専用窓に HTML でレンダリングして表示する
allowed-tools: Bash(bash /home/tarotene/.claude/hooks/plan-view.sh *)
---

執筆中のプランを Markdown レンダリング済みの HTML にして、Chrome の専用窓に表示する。

ExitPlanMode 時には同じものが自動で開く。このコマンドは、**プランを書き終える前に
途中の状態を目で確認したいとき**のためのものである。

手順:

1. 現在のセッションのプランファイル（plan mode の system message に記載されたパス、
   通常 `~/.claude/plans/*.md`）を特定する。プランファイルがまだ存在しない・空の場合は、
   先にここまでの内容をプランファイルに書き出してから進む。
2. 次を実行する:
   ```
   bash /home/tarotene/.claude/hooks/plan-view.sh <プランファイルの絶対パス>
   ```
3. 標準出力に生成された HTML のパスが 1 行返る。**それを一言添えて報告するだけでよい。**
   プランの内容を要約し直したり、レンダリング結果について推測を述べたりしないこと
   （ユーザーは今その窓を自分で読んでいる）。

注意:

- これは表示するだけの操作であり、プランの内容には一切触れない。ユーザーから修正の
  指示があるまでプランを書き換えないこと。
- 失敗した場合（`pandoc` 不在・`DISPLAY` なし・ブラウザ不在）は非ゼロで理由が
  stderr に出る。リトライせず、その理由をそのまま報告すること。
- 無効化されているとき（`PLAN_VIEW_SKIP=1` または `~/.claude/plan-views/skip`）でも、
  この CLI 経路は動く。無効化は ExitPlanMode の自動発火だけを止める。

$ARGUMENTS
