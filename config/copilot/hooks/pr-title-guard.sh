#!/usr/bin/env bash
# pr-title-guard.sh (Copilot CLI adapter) — PR タイトルを commit-message
# 契約として作成時に機械強制する preToolUse hook(ADR-0031)。
#
# 設計と根拠: docs/claude/pr-title-contract.md(このリポジトリ内)
#
# 判定ロジックは一切持たない: config/claude/hooks/pr-title-guard.sh の
# 判定エンジン(decide_pr_title 等)をそのまま `source` し、この adapter が
# 持つのは Copilot CLI の実際の preToolUse I/O 形への変換だけ。
# attribution-guard.sh の Copilot adapter と同じ設計。
#
# Copilot の preToolUse は Claude/Codex と入出力の形が違う
# (config/copilot/hooks/attribution-guard.sh の実測記録参照):
#   入力: {"cwd":"...","toolName":"bash","toolArgs":{"command":"..."}}
#   出力: hookSpecificOutput でラップしない直下の JSON
#   matcher が無いため toolName=="bash" のみをこの adapter 内部で絞り込む。
#
# hooks.json(~/.copilot/settings.json)の登録は home/modules/claude.nix の
# registerCopilotHooks 経由。
#
# 使い方: 通常は Copilot CLI から stdin JSON で呼ばれる(引数無し)。
#   pr-title-guard.sh --selftest   回帰テスト。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_PR_TITLE_GUARD="$SELF_DIR/../../claude/hooks/pr-title-guard.sh"
# shellcheck source=../../claude/hooks/pr-title-guard.sh
source "$CLAUDE_PR_TITLE_GUARD"

# Copilot は hookSpecificOutput でラップしない直下の JSON を読む(attribution-
# guard.sh の Copilot adapter と同じ)。
emit_deny_copilot() {
  jq -n --arg reason "$1" '{
    permissionDecision: "deny",
    permissionDecisionReason: $reason
  }'
}

main_copilot() {
  command -v jq > /dev/null 2>&1 || exit 0
  local input tool cmd project reason
  input="$(cat)" || exit 0
  tool="$(jq -r '.toolName // empty' <<< "$input" 2> /dev/null)" || exit 0
  project="$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0
  case "$tool" in
    bash)
      cmd="$(jq -r '.toolArgs.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide_pr_title "$cmd" "$project")" || exit 0
      ;;
    *) exit 0 ;;
  esac
  emit_deny_copilot "$reason"
  exit 0
}

ADAPTER_SELFTEST_TMP=""
cleanup_adapter_selftest() { [[ -n ${ADAPTER_SELFTEST_TMP:-} ]] && rm -rf "$ADAPTER_SELFTEST_TMP"; }

selftest_copilot() {
  local fails=0 self out decision repo tmp
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

  ADAPTER_SELFTEST_TMP="$(mktemp -d)"
  trap cleanup_adapter_selftest EXIT
  tmp="$ADAPTER_SELFTEST_TMP"
  repo="$tmp/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" remote add origin https://github.com/tarotene/dotfiles.git

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

  assert_deny "$(jq -nc --arg cwd "$repo" --arg cmd "gh pr create --title 'PR タイトルを直す' --body b" \
    '{sessionId:"s",cwd:$cwd,toolName:"bash",toolArgs:{command:$cmd}}')"
  assert_pass "$(jq -nc --arg cwd "$repo" '{sessionId:"s",cwd:$cwd,toolName:"bash",toolArgs:{command:"git status"}}')"
  assert_pass "$(jq -nc --arg cwd "$repo" '{sessionId:"s",cwd:$cwd,toolName:"str_replace_editor",toolArgs:{new_str:"x"}}')"

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
