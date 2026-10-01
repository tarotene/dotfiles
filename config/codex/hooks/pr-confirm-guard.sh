#!/usr/bin/env bash
# pr-confirm-guard.sh (Codex CLI adapter) — PR 本文に未チェックの task list
# を残さない・`## 要確認` の各項目に Issue 参照を持たせる原則を作成時に
# 機械強制する PreToolUse hook。
#
# 判定ロジックは一切持たない: config/claude/hooks/pr-confirm-guard.sh の
# 判定エンジン(decide_pr_confirm/emit_deny 等)をそのまま `source` し、この
# adapter が持つのは Codex CLI の実際の PreToolUse I/O 形への変換だけ。
# pr-title-guard.sh (Codex adapter) と同じ設計(既存手段: config/codex/
# hooks/pr-title-guard.sh — 拡張)。
#
# Codex の PreToolUse は Claude と入出力の形が同じ
# (config/codex/hooks/attribution-guard.sh の実測記録参照)。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由
# (matcher "Bash|mcp__.*")。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   pr-confirm-guard.sh --selftest   回帰テスト。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 配備先(~/.codex/hooks, ~/.copilot/hooks)には ../../claude/hooks という
# 兄弟ディレクトリが無い(#602)ので、CLAUDE_HOOKS_DIR → ソースツリー相対 →
# $HOME/.claude/hooks の順に、最初に存在したものを使う。
CLAUDE_PR_CONFIRM_GUARD=""
for _d in "${CLAUDE_HOOKS_DIR:-}" "$SELF_DIR/../../claude/hooks" "$HOME/.claude/hooks"; do
  if [[ -n $_d && -f $_d/pr-confirm-guard.sh ]]; then
    CLAUDE_PR_CONFIRM_GUARD="$_d/pr-confirm-guard.sh"
    break
  fi
done
# shellcheck source=../../claude/hooks/pr-confirm-guard.sh
source "$CLAUDE_PR_CONFIRM_GUARD"

main_codex() {
  command -v jq > /dev/null 2>&1 || exit 0
  local input tool cmd project reason
  input="$(cat)" || exit 0
  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  project="$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0
  case "$tool" in
    Bash)
      cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide_pr_confirm "$cmd" "$project")" || exit 0
      ;;
    *) exit 0 ;;
  esac
  emit_deny "$reason"
  exit 0
}

ADAPTER_SELFTEST_TMP=""
cleanup_adapter_selftest() { [[ -n ${ADAPTER_SELFTEST_TMP:-} ]] && rm -rf "$ADAPTER_SELFTEST_TMP"; }

selftest_codex() {
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

  assert_deny "$(jq -nc --arg cwd "$repo" --arg cmd "gh pr create --body 'Closes #1

## 検証
- [ ] 実機で確認'" '{tool_name:"Bash",cwd:$cwd,tool_input:{command:$cmd}}')"
  assert_pass "$(jq -nc --arg cwd "$repo" --arg cmd "gh pr create --body 'Closes #1

## 要確認
- #42 — 実機での確認'" '{tool_name:"Bash",cwd:$cwd,tool_input:{command:$cmd}}')"
  assert_pass "$(jq -nc --arg cwd "$repo" '{tool_name:"Bash",cwd:$cwd,tool_input:{command:"git status"}}')"

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
