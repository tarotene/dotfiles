---
description: 執筆中のプランを Copilot に中間レビューさせる（advisory・ゲートなし）
allowed-tools: Bash(bash /home/tarotene/.claude/hooks/copilot-plan-review.sh --advisory *)
---

執筆中のプランに対する Copilot の中間レビュー（advisory）を実行する。

観点の異なる 2 つの critic（要件・スコープ / 実装可能性・検証戦略）が並列で走り、
指摘は `BLOCKER` / `MAJOR` / `MINOR` / `NIT` に分類されて返る。

手順:

1. 現在のセッションのプランファイル（plan mode の system message に記載されたパス、通常 `~/.claude/plans/*.md`）を特定する。プランファイルがまだ存在しない・空の場合は、先にここまでの内容をプランファイルに書き出してから進む。
2. 次を実行する（数分かかる。タイムアウトは 300000 ms を指定）:
   ```
   bash /home/tarotene/.claude/hooks/copilot-plan-review.sh --advisory <プランファイルの絶対パス> <プロジェクトの絶対パス>
   ```
3. レビュー結果をユーザーに要約して報告する。**対応対象は `BLOCKER` と `MAJOR` だけ**である:
   - `BLOCKER` / `MAJOR` かつ `[TECHNICAL]`: リポジトリ等の証拠で検証し、妥当なら反映、誤りなら反証の根拠を添えて報告する。**反証できた指摘を却下するのは正当な帰結**であり、無理に反映しなくてよい。
   - `BLOCKER` / `MAJOR` かつ `[NEEDS_DECISION]`: 勝手に採否を判断せず、AskUserQuestion で論点と選択肢（推奨付き）をユーザーに提示し、回答を得てから反映する。
   - `MINOR` / `NIT`: いま直さない。backlog として扱い、要約で件数だけ伝える。ユーザーが明示的に求めた場合のみ対応する。
4. `## GATE: PASS`（= BLOCKER / MAJOR が 0 件）は正常かつ望ましい結果である。指摘を捻り出してプランを変える必要はない。`readiness` 行（`R= S= I= T=`）に `NO` があれば、その軸が未充足であることだけ報告する。
5. これは advisory であり Approve を妨げない。ExitPlanMode 時の自動レビューのラウンド数も消費しない。

$ARGUMENTS
