#!/usr/bin/env bash
# wrapup-session-start.sh — wrap-up inbox の収集指示を注入する SessionStart hook。
#
# 設計と根拠: docs/claude/wrapup-inbox.md(このリポジトリ内)
#
# グローバル CLAUDE.md を home-manager の store symlink にすると Claude Code の
# `#` メモリ追記が書き込み失敗で壊れるため、常時指示は additionalContext 注入で届ける。
# 注入内容:
#   - スコープ外の気づきは wrapup-stop-gate.sh --add で inbox(JSONL)に追記せよ
#   - inbox に未処理行が残っていれば「未処理 N 件」を掲示(遅延フラッシュ)
#
# inbox のパス計算(repo_slug/自己修復マージ)は wrapup-stop-gate.sh に一本化
# されている(--inbox-path / --migrate サブコマンド)。ここでは source せず
# 呼び出すだけに留める — gate を source すると hook 本体まで走ってしまうため。
# 起票可否(gh・git repo・GitHub remote)の判定は Stop 側の縮退ゲートに任せ、
# ここでは常に注入する。jq 不在なら黙って exit 0(fail-open)。
set -euo pipefail

command -v jq >/dev/null 2>&1 || exit 0
input="$(cat)"

hooks_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gate="$hooks_dir/wrapup-stop-gate.sh"

project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input")}"
[[ -n "$project" ]] || exit 0

bash "$gate" --migrate "$project" 2>/dev/null || true
inbox="$(bash "$gate" --inbox-path "$project")"

# #328: フィードバックの Issue 化検査(wrapup-stop-gate.sh)が使う「今
# セッション」境界の基準点を touch する。失敗しても fail-open(検査側が
# 判定不能として何もしない側に倒れる)。
session_id="$(jq -r '.session_id // "unknown"' <<<"$input" 2>/dev/null)" || session_id="unknown"
bash "$gate" --stamp-feedback-session "$session_id" 2>/dev/null || true

pending=0
[[ -s "$inbox" ]] && pending="$(wc -l <"$inbox")"

ctx="[wrapup-inbox] このプロジェクトの wrap-up inbox: ${inbox}
今回のタスクのスコープ外だが Issue 起票の価値がある気づき(バグの兆候、負債、改善案など)が
出たら、その時点で次のコマンドで 1 気づき = 1 行を追記すること:
  bash '${gate}' --add '${inbox}' '{\"ts\": \"<ISO8601>\", \"title\": \"<Issue タイトル>\", \"detail\": \"<内容と文脈>\"}'
inbox を直接編集してはいけない(必ず --add 経由)。追記した項目はターン終了時の
Stop hook が起票を案内する。"

if [[ "$pending" -gt 0 ]]; then
  ctx+="
現在この inbox には未処理 ${pending} 件が残っている(過去セッションの残骸を含む)。"
fi

jq -n --arg ctx "$ctx" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
