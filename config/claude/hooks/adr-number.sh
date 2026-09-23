#!/usr/bin/env bash
# adr-number.sh — `gh pr create` の直後に ADR-0000(起草中)を PR 番号へ
# 自動改番する PostToolUse hook(ADR-380, docs/claude/adr-numbering.md)。
#
# 段2(scripts/adr-number-check + CI required check)だけで採番衝突は既に
# 構造的に不可能になっている。この hook は段3の利便性層に過ぎず、
# 何も deny しない — commit を跨いだフォローアップ push が要る事実だけを
# additionalContext で伝え、忘れても CI が red になって気づける(段2で
# 担保済み)。単体で revert しても安全。
#
# 早期 exit: docs/adr/0000-*.md が無ければ、stdin の JSON すら読まずに
# 即 exit 0 にする(常時コストほぼゼロ)。$CLAUDE_PROJECT_DIR が未設定の
# 場合だけ stdin 側の解決結果で改めて判定する。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / CMD_SEPS)は
# attribution-guard.sh を source して再利用する(pr-title-guard.sh /
# stack-base-guard.sh と同じ「1つの判定エンジンを source する」型)。
# `is_target_at` はこのファイルで `gh pr create` 検出専用に上書きする。
#
# PR 番号は tool_response(gh pr create の出力)を読み取らず
# `gh pr view --json number` で解決する(先行例なし: config/claude/hooks/
# 全 19 本・config/codex/hooks/・config/copilot/hooks/・
# home/modules/claude.nix のいずれも tool_response を読んでいない。
# スキーマ非依存にすることで `--web` 作成・ブラウザ作成でも効く)。
#
# 使い方:
#   hook として: settings.json の PostToolUse(matcher: Bash)から
#                stdin JSON で呼ばれる
#   自己検査:   adr-number.sh --selftest(ネットワーク不使用、gh をスタブ)
set -euo pipefail

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

have() { command -v "$1" > /dev/null 2>&1; }

# 呼び出しのたびに解決する(--selftest の ADR_NUMBER_CHECK_BIN 差し替えが
# 効くように)。source tree からの相対パスを先に試し、無ければ PATH 上の
# 配備済み `adr-number-check`(~/.local/bin)にフォールバックする。
resolve_adr_number_check() {
  local candidate="${ADR_NUMBER_CHECK_BIN:-$GUARD_SELF_DIR/../../../scripts/adr-number-check}"
  if [[ -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  command -v adr-number-check 2> /dev/null
}

has_draft() {
  local project="$1"
  compgen -G "$project/docs/adr/0000-*.md" > /dev/null 2>&1
}

# ---------------------------------------------------------------------------
# コマンド位置判定(attribution-guard.sh の同名関数を上書きする)。
# `gh pr create`(kind 抽出は不要、存在検出のみ)。
# ---------------------------------------------------------------------------

is_target_at() {
  local i=$1 n=${#TOK[@]} base
  ((i + 2 < n)) || return 1
  base="${TOK[i]##*/}"
  [[ $base == gh ]] || return 1
  [[ ${TOK[i + 1]} == pr ]] || return 1
  [[ ${TOK[i + 2]} == create ]] || return 1
  return 0
}

# $1=コマンド文字列全体。`gh pr create` がコマンド位置に 1 つでもあれば 0。
command_ran_pr_create() {
  local cmd="$1"
  split_heredoc "$cmd"

  TOK=()
  local t
  while IFS= read -r -d '' t; do TOK+=("$t"); done < <(tokenize "$CMD_NOHD") || true
  ((${#TOK[@]} > 0)) || return 1

  local n=${#TOK[@]} i at_cmd_pos=1
  for ((i = 0; i < n; i++)); do
    if ((at_cmd_pos)) && is_target_at "$i"; then
      return 0
    fi
    if is_sep "${TOK[i]}"; then at_cmd_pos=1; else at_cmd_pos=0; fi
  done
  return 1
}

# ---------------------------------------------------------------------------
# hook 入出力
# ---------------------------------------------------------------------------

emit_context() {
  local msg
  msg="ADR-0000 を PR #${1} の番号へ改番しました($2)。commit して push してください。"
  jq -n --arg ctx "$msg" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
}

main() {
  # 最安の早期 exit: $CLAUDE_PROJECT_DIR が分かっていれば stdin を読む前に
  # draft の有無だけ見て終わる。
  local early_project="${CLAUDE_PROJECT_DIR:-}"
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

  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)}" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

  has_draft "$project" || exit 0
  command_ran_pr_create "$cmd" || exit 0

  local adr_check
  adr_check="$(resolve_adr_number_check)"
  [[ -n $adr_check && -x $adr_check ]] || exit 0

  have gh || exit 0
  local pr_number
  pr_number="$(cd "$project" && gh pr view --json number --jq '.number' 2> /dev/null)" || exit 0
  [[ $pr_number =~ ^[0-9]+$ ]] || exit 0

  local fix_out
  fix_out="$(cd "$project" && "$adr_check" --fix "$pr_number" 2>&1)" || exit 0

  emit_context "$pr_number" "$fix_out"
  exit 0
}

# ---------------------------------------------------------------------------
# selftest
# ---------------------------------------------------------------------------

SELFTEST_TMP=""
cleanup_selftest() { [[ -n ${SELFTEST_TMP:-} ]] && rm -rf "$SELFTEST_TMP"; }

selftest() {
  local fails=0 tmp

  SELFTEST_TMP="$(mktemp -d)"
  trap cleanup_selftest EXIT
  tmp="$SELFTEST_TMP"

  check() { # $1=名前 $2=期待exit $3=実際exit
    if [[ $2 == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected rc=$2 got rc=$3)" >&2
      fails=$((fails + 1))
    fi
  }
  check_contains() {
    if grep -qF -- "$2" <<< "$3"; then
      echo "ok   $1"
    else
      echo "FAIL $1 (substring [$2] not found in [$3])" >&2
      fails=$((fails + 1))
    fi
  }

  # --- gh スタブ(pr view のみ応答。それ以外は失敗させる) ---
  mkdir -p "$tmp/bin"
  cat > "$tmp/bin/gh" << 'STUB'
#!/usr/bin/env bash
if [[ "$1 $2" == "pr view" ]]; then
  echo 999
  exit 0
fi
exit 1
STUB
  chmod +x "$tmp/bin/gh"

  cat > "$tmp/bin/gh-noop" << 'STUB'
#!/usr/bin/env bash
exit 1
STUB
  chmod +x "$tmp/bin/gh-noop"

  # --- adr-number-check スタブ(呼び出し引数をファイルに記録する) ---
  cat > "$tmp/bin/adr-number-check" << 'STUB'
#!/usr/bin/env bash
echo "called with: $*" >> "$ADR_FIX_LOG"
echo "0000-x.md -> 999-x.md"
exit 0
STUB
  chmod +x "$tmp/bin/adr-number-check"
  export ADR_NUMBER_CHECK_BIN="$tmp/bin/adr-number-check"
  export ADR_FIX_LOG="$tmp/fix.log"

  # --- 実験用 git repo(docs/adr/ に draft あり) ---
  local repo="$tmp/repo"
  mkdir -p "$repo/docs/adr"
  git -C "$repo" init -q
  printf '# ADR-0000 — x\n' > "$repo/docs/adr/0000-x.md"

  run() { # $1=PATH の先頭に足すディレクトリ($tmp/bin か $tmp/binでない) $2=input json
    PATH="$1:$PATH" bash "$GUARD_SELF_DIR/adr-number.sh" <<< "$2"
  }

  local input_create input_other
  input_create="$(jq -n --arg cwd "$repo" '{tool_name:"Bash", tool_input:{command:"gh pr create --title x --body y"}, cwd:$cwd}')"
  input_other="$(jq -n --arg cwd "$repo" '{tool_name:"Bash", tool_input:{command:"git status"}, cwd:$cwd}')"

  echo "1: draft あり + gh pr create 検出 → additionalContext を出す"
  : > "$ADR_FIX_LOG"
  out="$(CLAUDE_PROJECT_DIR="$repo" run "$tmp/bin" "$input_create")"
  check_contains "1a additionalContext に PR番号を含む" "999" "$out"
  check_contains "1b --fix が呼ばれた" "--fix 999" "$(cat "$ADR_FIX_LOG")"

  echo "2: draft が無い → 早期 exit(gh も adr-number-check も呼ばれない)"
  local repo2="$tmp/repo2"
  mkdir -p "$repo2/docs/adr"
  git -C "$repo2" init -q
  : > "$ADR_FIX_LOG"
  out="$(CLAUDE_PROJECT_DIR="$repo2" run "$tmp/bin" "$(jq -n --arg cwd "$repo2" '{tool_name:"Bash", tool_input:{command:"gh pr create --title x"}, cwd:$cwd}')")"
  check "2a 出力なし" "" "$out"
  check "2b adr-number-check は呼ばれない" "" "$(cat "$ADR_FIX_LOG")"

  echo "3: draft はあるが gh pr create でないコマンド → 何もしない"
  : > "$ADR_FIX_LOG"
  out="$(CLAUDE_PROJECT_DIR="$repo" run "$tmp/bin" "$input_other")"
  check "3a 出力なし" "" "$out"
  check "3b adr-number-check は呼ばれない" "" "$(cat "$ADR_FIX_LOG")"

  echo "4: gh pr view が失敗(PR がまだ無い等) → クラッシュせず無出力"
  : > "$ADR_FIX_LOG"
  # gh 自体を差し替えた専用ディレクトリを PATH に通す(gh-noop)。
  local badbin="$tmp/badbin"
  mkdir -p "$badbin"
  ln -sf "$tmp/bin/gh-noop" "$badbin/gh"
  ln -sf "$tmp/bin/adr-number-check" "$badbin/adr-number-check"
  out="$(CLAUDE_PROJECT_DIR="$repo" run "$badbin" "$input_create")"
  check "4a 出力なし" "" "$out"
  check "4b adr-number-check は呼ばれない" "" "$(cat "$ADR_FIX_LOG")"

  echo "5: tool が Bash でない → 何もしない"
  out="$(CLAUDE_PROJECT_DIR="$repo" run "$tmp/bin" '{"tool_name":"Read","tool_input":{}}')"
  check "5a 出力なし" "" "$out"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

if [[ "${1-}" == "--selftest" ]]; then
  selftest
else
  main
fi
