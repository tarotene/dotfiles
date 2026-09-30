#!/usr/bin/env bash
# wrapup-session-start.sh — wrap-up inbox の収集指示を注入する SessionStart hook。
#
# 設計と根拠: docs/claude/wrapup-inbox.md(このリポジトリ内)
#
# グローバル CLAUDE.md を home-manager の store symlink にすると Claude Code の
# `#` メモリ追記が書き込み失敗で壊れるため、常時指示は additionalContext 注入で届ける。
# 注入内容(LLM 向け hook 出力の書式は ADR-0000: <hook-directive> 外枠 + 英語本文):
#   - スコープ外の気づきは wrapup-stop-gate.sh --add で inbox(JSONL)に追記せよ
#   - inbox に未処理行が残っていれば未処理件数を掲示(遅延フラッシュ)
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

ctx="<hook-directive source=\"wrapup-session-start\" event=\"SessionStart\">
wrap-up inbox for this project: ${inbox}
When something outside the current task's scope is worth an Issue (a sign of a
bug, debt, an improvement idea), append it right then as one line per finding:
  bash '${gate}' --add '${inbox}' '{\"ts\": \"<ISO8601>\", \"title\": \"<Issue title>\", \"detail\": \"<what and why>\"}'
Do not edit the inbox directly (always go through --add). The Stop hook at the
end of the turn points to the filing procedure for appended items."

if [[ "$pending" -gt 0 ]]; then
  ctx+="
The inbox currently holds ${pending} unprocessed item(s) (including leftovers from past sessions)."
fi
ctx+="
</hook-directive>"

jq -n --arg ctx "$ctx" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
