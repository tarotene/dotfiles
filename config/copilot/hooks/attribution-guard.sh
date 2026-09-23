#!/usr/bin/env bash
# attribution-guard.sh (Copilot CLI adapter) — Copilot CLI が GitHub に書く
# 外向きテキストに attribution フッターが載っていることを保証する
# preToolUse hook。
#
# 設計と根拠: docs/claude/attribution-guard.md(このリポジトリ内、#192)
#
# 判定ロジックは一切持たない: config/claude/hooks/attribution-guard.sh の
# 判定エンジン(decide/has_marker 等)をそのまま `source` し、この adapter が
# 持つのは Copilot CLI の実際の preToolUse I/O 形への変換だけ。tarotene/
# bleep(当時 publish-guard)の adapters/copilot-adapter.sh(claude-
# adapter.sh を薄く包む既存の型 — #25-28 で単一 shim hooks/bleep.sh
# --host=<name> に統合済み、ここでの記述は統合前の実測記録)と同じ設計。
#
# Copilot の preToolUse は Claude/Codex と入出力の形が違う(tarotene/bleep
# (当時 publish-guard)の copilot-adapter.sh が 2026-09-10 に `copilot -p ...
# --allow-all-tools` で実測済み):
#   実測入力: {"sessionId":"...","timestamp":..,"cwd":"...",
#              "toolName":"bash","toolArgs":{"command":"..."}}
#     (toolName は小文字 "bash"、Bash/Shell ではない)
#   出力: hookSpecificOutput でラップしない直下の JSON
#     {"permissionDecision":"deny","permissionDecisionReason":"..."}
#   を返すと実際にブロックされることも実測済み(publish-guard 時代の記録)。
#   Copilot の preToolUse には matcher が無く全 tool call で無条件発火する
#   ため、tool 種別の絞り込みはこの adapter 内部(toolName=="bash" のみ
#   対象)で行う。
#
# hooks.json(~/.copilot/settings.json)の登録は home/modules/claude.nix の
# registerCopilotHooks 経由(preToolUse、matcher フィールドは無視されるため
# 書かない)。MCP tool 名の命名規則が Copilot 側で未確認(#161)なため、
# この adapter は toolName=="bash" のコマンドのみを対象にする。
#
# 使い方: 通常は Copilot CLI から stdin JSON で呼ばれる(引数無し)。
#   attribution-guard.sh --selftest   回帰テスト。
set -uo pipefail

ATTRIBUTION_AGENT_NAME="GitHub Copilot CLI"
ATTRIBUTION_AGENT_URL="https://docs.github.com/en/copilot/how-tos/copilot-cli"

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_ATTRIBUTION_GUARD="$SELF_DIR/../../claude/hooks/attribution-guard.sh"
# shellcheck source=../../claude/hooks/attribution-guard.sh
source "$CLAUDE_ATTRIBUTION_GUARD"

# Copilot は hookSpecificOutput でラップしない直下の JSON を読む(Claude/
# Codex とはここだけ違う — publish-guard 時代の copilot-adapter.sh で実測済み)。
emit_deny_copilot() {
  jq -n --arg reason "$1" '{
    permissionDecision: "deny",
    permissionDecisionReason: $reason
  }'
}

main_copilot() {
  command -v jq > /dev/null 2>&1 || exit 0
  local input tool cmd reason
  input="$(cat)" || exit 0
  tool="$(jq -r '.toolName // empty' <<< "$input" 2> /dev/null)" || exit 0
  case "$tool" in
    bash)
      cmd="$(jq -r '.toolArgs.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide "$cmd")" || exit 0
      ;;
    *) exit 0 ;;
  esac
  emit_deny_copilot "$reason"
  exit 0
}

selftest_copilot() {
  local fails=0 self out decision footer
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  footer="🤖 Generated with [GitHub Copilot CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli)"

  assert_deny() { # $1=入力JSON
    out="$(bash "$self" <<< "$1")"
    decision="$(jq -r '.permissionDecision // empty' <<< "$out" 2> /dev/null)"
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

  assert_deny "$(jq -nc --arg cmd "gh pr comment 1 --body '確認しました。'" '{sessionId:"s",cwd:"/x",toolName:"bash",toolArgs:{command:$cmd}}')"
  assert_pass "$(jq -nc --arg cmd "gh pr comment 1 --body '確認しました。$footer'" '{sessionId:"s",cwd:"/x",toolName:"bash",toolArgs:{command:$cmd}}')"
  assert_pass "$(jq -nc '{sessionId:"s",cwd:"/x",toolName:"bash",toolArgs:{command:"git status"}}')"
  # matcher が無いため bash 以外の tool でも無条件発火するが、この adapter
  # は toolName!="bash" を無視する(#161 未確認の MCP と同じ理由で対象外)。
  assert_pass "$(jq -nc '{sessionId:"s",cwd:"/x",toolName:"str_replace_editor",toolArgs:{new_str:"no footer here"}}')"

  if [[ $fails -gt 0 ]]; then
    echo "selftest(copilot adapter): ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest(copilot adapter): OK"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest) selftest_copilot ;;
    *) main_copilot ;;
  esac
fi
