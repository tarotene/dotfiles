---
description: 執筆中のプランを Codex に中間レビューさせる（advisory・ゲートなし）
allowed-tools: Bash(bash /home/tarotene/.claude/hooks/codex-plan-review.sh --advisory *)
---

執筆中のプランに対する Codex の中間レビュー（advisory）を実行する。

手順:

1. 現在のセッションのプランファイル（plan mode の system message に記載されたパス、通常 `~/.claude/plans/*.md`）を特定する。プランファイルがまだ存在しない・空の場合は、先にここまでの内容をプランファイルに書き出してから進む。
2. 次を実行する（数分かかる。タイムアウトは 300000 ms を指定）:
   ```
   bash /home/tarotene/.claude/hooks/codex-plan-review.sh --advisory <プランファイルの絶対パス> <プロジェクトの絶対パス>
   ```
3. レビュー結果をユーザーに要約して報告する。指摘の扱いは種別で分ける:
   - `[技術]` タグ: リポジトリ等の証拠で検証し、妥当なら反映、誤りなら反証の根拠を添えて報告する。
   - `[要判断]` タグ（およびタグなし）: 勝手に採否を判断せず、AskUserQuestion で論点と選択肢（推奨付き）をユーザーに提示し、回答を得てから反映する。
4. これは advisory であり Approve を妨げない。VERDICT 行は参考情報として扱う。

$ARGUMENTS
