#!/usr/bin/env bash
# git-stash-guard.sh (Codex CLI adapter) — 素の `git stash` を弾く
# PreToolUse hook(ADR-0027 Amendment #531)。
#
# 設計と根拠: docs/claude/git-stash-guard.md(このリポジトリ内)
#
# 判定ロジックは一切持たない: config/claude/hooks/git-stash-guard.sh の
# 判定エンジン(decide/emit_deny 等)をそのまま `source` し、この adapter が
# 持つのは Codex CLI の実際の PreToolUse I/O 形への変換だけ。
# attribution-guard.sh の Codex adapter と同じ設計。
#
# Codex の PreToolUse は Claude と入出力の形が同じ(attribution-guard.sh の
# 実測記録参照)。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由
# (matcher "Bash")。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   git-stash-guard.sh --selftest   回帰テスト。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_GIT_STASH_GUARD="$SELF_DIR/../../claude/hooks/git-stash-guard.sh"
# shellcheck source=../../claude/hooks/git-stash-guard.sh
source "$CLAUDE_GIT_STASH_GUARD"

main_codex() {
  command -v jq > /dev/null 2>&1 || exit 0
  local input cmd reason
  input="$(cat)" || exit 0
  grep -qw stash <<< "$input" || exit 0

  cmd="$(jq -r 'select(.tool_name == "Bash") | .tool_input.command // empty' \
    <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  if reason="$(decide "$cmd")"; then
    jq -n --arg reason "$reason" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $reason
      }
    }'
  fi
  exit 0
}

selftest_codex() {
  local fails=0 self out decision

  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

  expect_deny() {
    out="$(bash "$self" <<< "$(jq -nc --arg cmd "$1" '{tool_name:"Bash",tool_input:{command:$cmd}}')")"
    decision="$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<< "$out" 2> /dev/null)"
    if [[ $decision != "deny" ]]; then
      echo "FAIL(deny 期待): コマンド=$1 出力=$out" >&2
      fails=$((fails + 1))
    fi
  }
  expect_pass() {
    out="$(bash "$self" <<< "$(jq -nc --arg cmd "$1" '{tool_name:"Bash",tool_input:{command:$cmd}}')")"
    if [[ -n $out ]]; then
      echo "FAIL(pass 期待、出力あり): コマンド=$1 出力=$out" >&2
      fails=$((fails + 1))
    fi
  }

  expect_deny "git stash pop"
  expect_deny "git stash"
  expect_pass "git stash list"
  expect_pass "git status"
  expect_pass "git stash push -u -m unique-tag"

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
