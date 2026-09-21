#!/usr/bin/env bash
# attribution-guard.sh (Codex CLI adapter) — Codex CLI が GitHub に書く外向き
# テキストに attribution フッターが載っていることを保証する PreToolUse hook。
#
# 設計と根拠: docs/claude/attribution-guard.md(このリポジトリ内、#192)
#
# 判定ロジックは一切持たない: config/claude/hooks/attribution-guard.sh の
# 判定エンジン(decide/emit_deny 等)をそのまま `source` し、この adapter が
# 持つのは Codex CLI の実際の PreToolUse I/O 形への変換だけ。tarotene/
# publish-guard の adapters/codex-adapter.sh(claude-adapter.sh を薄く包む
# 既存の型)と同じ設計。
#
# Codex の PreToolUse は Claude と入出力の形が同じ(tarotene/publish-guard の
# codex-adapter.sh が 2026-09-10 に `codex exec
# --dangerously-bypass-hook-trust` で実測済み):
#   実測入力: {"tool_name":"Bash","tool_input":{"command":"..."}, ...}
#   出力: hookSpecificOutput でラップした
#         {"hookSpecificOutput":{"hookEventName":"PreToolUse",
#           "permissionDecision":"deny","permissionDecisionReason":"..."}}
#   を返すと実際にブロックされることも実測済み(publish-guard 側の記録)。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由
# (matcher "Bash|mcp__.*")。MCP tool 名の命名規則が Codex 側で未確認
# (#161)なため、この adapter は Bash 経由の `gh` コマンドのみを対象にする
# (decide_mcp は呼ばない)。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   attribution-guard.sh --selftest   回帰テスト。
set -uo pipefail

ATTRIBUTION_AGENT_NAME="Codex CLI"
ATTRIBUTION_AGENT_URL="https://learn.chatgpt.com/docs/codex/cli"

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_ATTRIBUTION_GUARD="$SELF_DIR/../../claude/hooks/attribution-guard.sh"
# shellcheck source=../../claude/hooks/attribution-guard.sh
source "$CLAUDE_ATTRIBUTION_GUARD"

main_codex() {
  command -v jq > /dev/null 2>&1 || exit 0
  local input tool cmd reason
  input="$(cat)" || exit 0
  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  case "$tool" in
    Bash)
      cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide "$cmd")" || exit 0
      ;;
    *) exit 0 ;;
  esac
  emit_deny "$reason"
  exit 0
}

selftest_codex() {
  local fails=0 self out decision footer
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  footer="🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)"

  assert_deny() { # $1=入力JSON
    out="$(bash "$self" <<< "$1")"
    decision="$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<< "$out" 2> /dev/null)"
    if [[ $decision != "deny" ]]; then
      echo "FAIL(deny 期待): 入力=$1 出力=$out" >&2
      fails=$((fails + 1))
    fi
  }
  assert_pass() { # $1=入力JSON
    out="$(bash "$self" <<< "$1")"
    if [[ -n $out ]]; then
      echo "FAIL(pass 期待、出力あり): 入力=$1 出力=$out" >&2
      fails=$((fails + 1))
    fi
  }

  assert_deny "$(jq -nc --arg cmd "gh pr comment 1 --body '確認しました。'" '{tool_name:"Bash",tool_input:{command:$cmd}}')"
  assert_pass "$(jq -nc --arg cmd "gh pr comment 1 --body '確認しました。$footer'" '{tool_name:"Bash",tool_input:{command:$cmd}}')"
  assert_pass "$(jq -nc '{tool_name:"Bash",tool_input:{command:"git status"}}')"
  assert_pass "$(jq -nc '{tool_name:"mcp__github__create_issue",tool_input:{body:"no footer"}}')"

  if [[ $fails -gt 0 ]]; then
    echo "selftest(codex adapter): ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest(codex adapter): OK"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest) selftest_codex ;;
    *) main_codex ;;
  esac
fi
