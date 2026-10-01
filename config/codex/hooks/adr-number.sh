#!/usr/bin/env bash
# adr-number.sh (Codex CLI adapter) — `gh pr create` 直後に ADR-0000 を
# PR 番号へ自動改番する PostToolUse hook(ADR-380 Amendment #531)。
#
# 設計と根拠: docs/claude/adr-numbering.md(このリポジトリ内)
#
# 判定ロジックは一切持たない: config/claude/hooks/adr-number.sh の関数群
# (has_draft/command_ran_pr_create/resolve_adr_number_check/emit_context 等)
# をそのまま `source` し、この adapter が持つのは Codex CLI の実際の
# PostToolUse I/O 形への変換だけ。attribution-guard.sh の Codex adapter と
# 同じ設計。段3(利便性層)のみ — deny は一切しない。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   adr-number.sh --selftest   回帰テスト。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 配備先(~/.codex/hooks, ~/.copilot/hooks)には ../../claude/hooks という
# 兄弟ディレクトリが無い(#602)ので、CLAUDE_HOOKS_DIR → ソースツリー相対 →
# $HOME/.claude/hooks の順に、最初に存在したものを使う。
CLAUDE_ADR_NUMBER=""
for _d in "${CLAUDE_HOOKS_DIR:-}" "$SELF_DIR/../../claude/hooks" "$HOME/.claude/hooks"; do
  if [[ -n $_d && -f $_d/adr-number.sh ]]; then
    CLAUDE_ADR_NUMBER="$_d/adr-number.sh"
    break
  fi
done
# shellcheck source=../../claude/hooks/adr-number.sh
source "$CLAUDE_ADR_NUMBER"

main_codex() {
  local early_project="${CODEX_PROJECT_DIR:-}"
  if [[ -n $early_project ]] && ! has_draft "$early_project"; then
    exit 0
  fi

  have jq || exit 0

  local input tool cmd project
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ $tool == Bash ]] || exit 0

  cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  project="${CODEX_PROJECT_DIR:-$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)}" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

  has_draft "$project" || exit 0
  command_ran_pr_create "$cmd" || exit 0

  local adr_check
  adr_check="$(resolve_adr_number_check)"
  [[ -n $adr_check && -x $adr_check ]] || exit 0

  have gh || exit 0
  # PR 番号と base ブランチ名(差分内の ADR-0000 参照を書き換える範囲、#644)。
  local pr_info pr_number pr_base fix_args
  pr_info="$(cd "$project" && gh pr view --json number,baseRefName --jq '"\(.number) \(.baseRefName)"' 2> /dev/null)" || exit 0
  read -r pr_number pr_base <<< "$pr_info"
  [[ $pr_number =~ ^[0-9]+$ ]] || exit 0
  fix_args=(--fix "$pr_number")
  if [[ -n ${pr_base:-} ]]; then
    fix_args+=(--base "origin/$pr_base")
  fi

  local fix_out
  fix_out="$(cd "$project" && "$adr_check" "${fix_args[@]}" 2>&1)" || exit 0

  emit_context "$pr_number" "$fix_out"
  exit 0
}

selftest_codex() {
  local fails=0 self repo out ctx
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  ADR_NUMBER_ADAPTER_SELFTEST_TMP="$(mktemp -d)"
  trap 'rm -rf "$ADR_NUMBER_ADAPTER_SELFTEST_TMP"' RETURN
  local tmp="$ADR_NUMBER_ADAPTER_SELFTEST_TMP"
  repo="$tmp/repo"
  mkdir -p "$repo/docs/adr"
  git -C "$repo" init -q
  cat > "$repo/docs/adr/0000-draft.md" << 'EOF'
# ADR-0000 — draft
EOF
  cat > "$tmp/adr-number-check" << 'SCRIPT'
#!/usr/bin/env bash
echo "renamed to $2"
SCRIPT
  chmod +x "$tmp/adr-number-check"
  cat > "$tmp/gh" << 'SCRIPT'
#!/usr/bin/env bash
echo 999
SCRIPT
  chmod +x "$tmp/gh"

  local input
  input="$(jq -nc --arg cwd "$repo" '{tool_name:"Bash",cwd:$cwd,tool_input:{command:"gh pr create --title x --body y"}}')"
  out="$(PATH="$tmp:$PATH" ADR_NUMBER_CHECK_BIN="$tmp/adr-number-check" bash "$self" <<< "$input")"
  ctx="$(jq -r '.hookSpecificOutput.additionalContext // empty' <<< "$out" 2> /dev/null)"
  if [[ -z $ctx ]]; then
    echo "FAIL(additionalContext 期待): 出力=$out" >&2
    fails=$((fails + 1))
  fi

  local input_other out_other
  input_other="$(jq -nc --arg cwd "$repo" '{tool_name:"Bash",cwd:$cwd,tool_input:{command:"git status"}}')"
  out_other="$(PATH="$tmp:$PATH" bash "$self" <<< "$input_other")"
  if [[ -n $out_other ]]; then
    echo "FAIL(pass 期待、出力あり): 出力=$out_other" >&2
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
