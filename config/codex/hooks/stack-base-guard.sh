#!/usr/bin/env bash
# stack-base-guard.sh (Codex CLI adapter) — セッション内の複数 PR を常時
# 単一チェーンに積むことを作成時に機械強制する PreToolUse hook
# (ADR-0027 Amendment #531)。
#
# 設計と根拠: docs/claude/stack-base-guard.md(このリポジトリ内)
#
# 判定ロジックは一切持たない: config/claude/hooks/stack-base-guard.sh の
# 判定エンジン(decide_stack/decide_mcp_stack/emit_deny 等、attribution-
# guard.sh から継承した emit_deny を含む)をそのまま `source` し、この
# adapter が持つのは Codex CLI の実際の PreToolUse I/O 形への変換だけ。
# pr-title-guard.sh の Codex adapter と同じ設計。state ディレクトリ
# (~/.claude/stack-base-guard/state)は Claude と共有する — session_id で
# 区切られるため衝突しない(リポジトリ側の方針であって、エージェント別に
# 持つ理由が無い)。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由
# (matcher "Bash|mcp__.*")。MCP tool 名の命名規則が Codex 側で未確認
# (#161)なため、attribution-guard の Codex adapter と同様 Bash 経由のみを
# 対象にする(decide_mcp_stack は呼ばない)。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   stack-base-guard.sh --selftest   回帰テスト。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_STACK_BASE_GUARD="$SELF_DIR/../../claude/hooks/stack-base-guard.sh"
# shellcheck source=../../claude/hooks/stack-base-guard.sh
source "$CLAUDE_STACK_BASE_GUARD"

main_codex() {
  have jq || exit 0

  if [[ -e "$STACK_BASE_GUARD_DIR/skip" || "${SKIP_STACK_BASE_GUARD:-0}" == "1" ]]; then
    exit 0
  fi

  local input tool project reason cmd
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ $tool == Bash ]] || exit 0
  SESSION_ID="$(jq -r '.session_id // "unknown"' <<< "$input" 2> /dev/null)" || SESSION_ID="unknown"
  project="$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

  cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0
  reason="$(decide_stack "$cmd" "$project")" || exit 0

  emit_deny "$reason"
  exit 0
}

selftest_codex() {
  # decide_stack() 自体の網羅的な検査は config/claude/hooks/stack-base-
  # guard.sh の selftest(15 ケース)が担う。この adapter の selftest は
  # 「Codex の PreToolUse I/O 形から main_codex() 経由で decide_stack() まで
  # 正しく橋渡しできるか」だけを、deny/pass 各1ケースで確認する
  # (pr-title-guard.sh の Codex adapter と同じ最小主義)。
  local fails=0 self tmp repo stub_path out decision
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  ADAPTER_SELFTEST_TMP="$(mktemp -d)"
  trap 'rm -rf "$ADAPTER_SELFTEST_TMP"' RETURN
  tmp="$ADAPTER_SELFTEST_TMP"

  mkdir -p "$tmp/bin"
  cat > "$tmp/bin/gh" << 'STUB'
#!/usr/bin/env bash
jqbin="$(command -v jq)"
case "$1" in
  pr)
    case "$2" in
      list)
        if [[ -n "${STACK_STUB_PR_LIST_FILE:-}" && -f "${STACK_STUB_PR_LIST_FILE:-}" ]]; then
          cat "${STACK_STUB_PR_LIST_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  repo)
    case "$2" in
      view) exit 1 ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$tmp/bin/gh"
  stub_path="$tmp/bin:$PATH"

  repo="$tmp/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m base
  git -C "$repo" remote add origin https://github.com/tarotene/dotfiles.git
  git -C "$repo" update-ref refs/remotes/origin/main "$(git -C "$repo" rev-parse HEAD)"
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main

  git -C "$repo" switch -c stage1 -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m c1
  stage1_sha="$(git -C "$repo" rev-parse HEAD)"

  git -C "$repo" switch -c stage2 -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m c2

  printf '[{"number":1,"headRefName":"stage1","headRefOid":"%s","baseRefName":"main"}]\n' \
    "$stage1_sha" > "$tmp/prs-stage1-only.json"
  printf '[]\n' > "$tmp/prs-empty.json"

  # 1: stage2 から祖先 PR(#1 stage1)ありのまま base:main で create -> deny
  out="$(PATH="$stub_path" STACK_BASE_GUARD_DIR="$tmp/state1" STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    jq -nc --arg cwd "$repo" '{tool_name:"Bash",cwd:$cwd,session_id:"codex-selftest-1",tool_input:{command:"gh pr create --base main --title t --body b"}}' \
    | PATH="$stub_path" STACK_BASE_GUARD_DIR="$tmp/state1" STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" bash "$self")"
  decision="$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<< "$out" 2> /dev/null)"
  if [[ $decision != "deny" ]]; then
    echo "FAIL(deny 期待): 出力=$out" >&2
    fails=$((fails + 1))
  fi

  # 2: 祖先 PR なし(初回 PR)-> pass
  out="$(PATH="$stub_path" STACK_BASE_GUARD_DIR="$tmp/state2" STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    jq -nc --arg cwd "$repo" '{tool_name:"Bash",cwd:$cwd,session_id:"codex-selftest-2",tool_input:{command:"gh pr create --base main --title t --body b"}}' \
    | PATH="$stub_path" STACK_BASE_GUARD_DIR="$tmp/state2" STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" bash "$self")"
  if [[ -n $out ]]; then
    echo "FAIL(pass 期待、出力あり): 出力=$out" >&2
    fails=$((fails + 1))
  fi

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
