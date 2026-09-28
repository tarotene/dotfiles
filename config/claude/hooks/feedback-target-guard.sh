#!/usr/bin/env bash
# feedback-target-guard.sh — `gh issue create --label feedback` に
# `Target:` 行(規範・skill・hook への実在ポインタ)を要求する PreToolUse
# (Bash) hook(ADR-543「既存手段の前倒し接地と、決定論への昇格導線」段3)。
#
# 動機: Q2(LLM/散文 → 決定論への昇格)の兆候の1つは「同じ規範・skillに
# 対する feedback Issue の再発」。再発を機械的に集計するには、各 feedback
# Issue がどの規範・skill・hook を指しているかを本文から決定論で読み取れる
# 必要がある。自由記述のまま起票させると集計不能になる(閉語彙 > 自由記述
# +事後 lint、ADR-0035 D1)ため、起票の瞬間に `Target:` を要求する。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / decide / TOK /
# CMD_SEPS)は attribution-guard.sh を source して再利用する(adr-number.sh
# / pr-title-guard.sh / stack-base-guard.sh と同じ「1つの判定エンジンを
# source する」型 — 既存手段: config/claude/hooks/attribution-guard.sh —
# 拡張: heredoc 分離・引用符解釈込みのトークナイザを自前で再実装しない)。
# `is_target_at` / `decide_tokens` / `selftest` をこのファイル専用に
# 上書きする(sourcing 後の関数再定義は decide() の呼び出し先を差し替える
# だけで済み、decide() 自体は書き換えない)。
#
# `Target:` の閉じた語彙(いずれか1つ、実在照合まで行う):
#   Target: skill/<name>          — config/claude/skills/<name>/SKILL.md
#                                    (配備先 ~/.claude/skills/<name>/SKILL.md)
#   Target: agents-md/<節見出し>   — ~/.agents/AGENTS.md または
#                                    ~/.claude/CLAUDE.md の実在する見出し文字列
#   Target: hook/<name>            — ~/.claude/hooks/<name>* の実在するファイル
#
# 過去の feedback Issue(#516 等、この hook の導入前に起票されたもの)は
# 遡及修正しない(ADR-0007 の grandfathering 前例、ADR-0035 D5 と同型)。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: Bash)から
#                stdin JSON で呼ばれる
#   自己検査:   feedback-target-guard.sh --selftest(ネットワーク不使用)
set -uo pipefail

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

FEEDBACK_LABEL="feedback"

# ---------------------------------------------------------------------------
# コマンド位置判定(attribution-guard.sh の同名関数を上書きする)。
# `gh issue create` のみを対象にする。
# ---------------------------------------------------------------------------
is_target_at() {
  local i=$1 n=${#TOK[@]} base
  ((i + 2 < n)) || return 1
  base="${TOK[i]##*/}"
  [[ $base == gh ]] || return 1
  [[ ${TOK[i + 1]} == issue ]] || return 1
  [[ ${TOK[i + 2]} == create ]] || return 1
  TARGET_KIND=cli
  return 0
}

# $1=種別/名前(例: "skill/precedent-grounding") ; 実在すれば 0。
validate_target() {
  local value="$1" kind name
  [[ $value == */* ]] || return 1
  kind="${value%%/*}"
  name="${value#*/}"
  [[ -n $name ]] || return 1
  case "$kind" in
    skill) [[ -f "$HOME/.claude/skills/$name/SKILL.md" ]] ;;
    hook) compgen -G "$HOME/.claude/hooks/${name}*" > /dev/null 2>&1 ;;
    agents-md)
      grep -qF -- "$name" "$HOME/.agents/AGENTS.md" 2> /dev/null \
        || grep -qF -- "$name" "$HOME/.claude/CLAUDE.md" 2> /dev/null
      ;;
    *) return 1 ;;
  esac
}

# $1=本文; 最初の `Target: <値>` 行の値だけを出力(無ければ何も出さない)。
find_target_value() {
  awk '
    match($0, /^[[:space:]]*Target:[[:space:]]*[^[:space:]]+/) {
      line = substr($0, RSTART, RLENGTH)
      sub(/^[[:space:]]*Target:[[:space:]]*/, "", line)
      print line
      exit
    }
  ' <<< "$1"
}

# $@=1コマンドぶんのトークン列; feedback ラベル(カンマ区切り複数可)が
# 含まれていれば 0。
has_feedback_label() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0 val p
  local -a parts
  while ((i < n)); do
    case "${tok[i]}" in
      --label | -l)
        if ((i + 1 < n)); then
          val="${tok[i + 1]}"
          IFS=',' read -r -a parts <<< "$val"
          for p in "${parts[@]}"; do [[ $p == "$FEEDBACK_LABEL" ]] && return 0; done
          i=$((i + 2))
          continue
        fi
        ;;
      --label=*)
        val="${tok[i]#--label=}"
        IFS=',' read -r -a parts <<< "$val"
        for p in "${parts[@]}"; do [[ $p == "$FEEDBACK_LABEL" ]] && return 0; done
        ;;
    esac
    i=$((i + 1))
  done
  return 1
}

deny_reason_feedback() {
  printf '%s' "feedback ラベル付き Issue の起票に問題があります: ${1}。

本文に次のいずれかの形式で1行追加してください(実在照合されます):

  Target: skill/<name>          — config/claude/skills/<name>/SKILL.md
  Target: agents-md/<節見出し>   — ~/.agents/AGENTS.md・~/.claude/CLAUDE.md の実在する見出し
  Target: hook/<name>            — config/claude/hooks/<name>* の実在するファイル

(共有 AGENTS.md「ユーザーからのフィードバックは不可視なローカルメモに
閉じ込めない」、ADR-543「既存手段の前倒し接地と、決定論への昇格導線」参照)"
}

# ---------------------------------------------------------------------------
# decide_tokens を上書き(attribution-guard.sh の同名関数を差し替える)。
# 1投稿ぶんのトークン列を受け、feedback ラベル + Target: を検査する。
# ---------------------------------------------------------------------------
decide_tokens() {
  local -a tok=("$@")
  has_feedback_label "${tok[@]}" || return 1 # feedback で無ければ対象外(判定不能で通す)

  local n=${#tok[@]} i=0 have_flag=0 has_hd=0 p
  local -a texts=()

  for ((i = 0; i < n; i++)); do
    if [[ ${tok[i]} == *'<<'* ]]; then
      has_hd=1
      break
    fi
  done || true

  i=0
  while ((i < n)); do
    case "${tok[i]}" in
      --body | -b)
        have_flag=1
        if ((i + 1 < n)); then
          texts+=("${tok[i + 1]}")
          i=$((i + 2))
          continue
        fi
        ;;
      --body=*)
        have_flag=1
        texts+=("${tok[i]#--body=}")
        ;;
      --body-file | -F)
        have_flag=1
        if ((i + 1 < n)); then
          p="${tok[i + 1]}"
          [[ -f $p && -r $p ]] && texts+=("$(cat -- "$p")")
          i=$((i + 2))
          continue
        fi
        ;;
      --body-file=*)
        have_flag=1
        p="${tok[i]#--body-file=}"
        [[ -f $p && -r $p ]] && texts+=("$(cat -- "$p")")
        ;;
    esac
    i=$((i + 1))
  done

  ((have_flag)) || return 1 # 本文フラグ無し → 判定不能で通す(feedback は付いているが body 無しは gh 側で別途弾かれる)

  local text=""
  ((${#texts[@]} > 0)) && text="$(printf '%s\n' "${texts[@]}")"

  if ((has_hd)) && [[ -n ${HD_BODIES:-} ]]; then
    text+=$'\n'"$HD_BODIES"
  elif [[ $text == *'$('* || $text == *'`'* ]]; then
    return 1 # 本文がコマンド置換 → 中身が不明 → 判定不能で通す
  fi

  [[ -n ${text//[[:space:]]/} ]] || return 1

  local target
  target="$(find_target_value "$text")"
  if [[ -z $target ]]; then
    deny_reason_feedback "Target: 行がありません"
    return 0
  fi
  if ! validate_target "$target"; then
    deny_reason_feedback "Target: ${target} が実在しません"
    return 0
  fi
  return 1 # OK
}

# ---------------------------------------------------------------------------
# hook 入出力
# ---------------------------------------------------------------------------

emit_deny() {
  jq -n --arg reason "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0

  local input tool cmd reason
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ $tool == Bash ]] || exit 0

  cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  reason="$(decide "$cmd")" && {
    emit_deny "$reason"
    exit 0
  }
  exit 0
}

# ---------------------------------------------------------------------------
# selftest(attribution-guard.sh の同名関数を上書きする)。
# `set -u` 下で EXIT trap が関数ローカル変数の解体後に走ると unbound
# variable で落ちるため、adr-number.sh と同じくグローバル変数に逃がす。
# ---------------------------------------------------------------------------
SELFTEST_TMP=""
cleanup_selftest() { [[ -n ${SELFTEST_TMP:-} ]] && rm -rf "$SELFTEST_TMP"; }

selftest() {
  local fails=0 tmp
  SELFTEST_TMP="$(mktemp -d)"
  trap cleanup_selftest EXIT
  tmp="$SELFTEST_TMP"

  expect_pass() { # $1=名前 $2=cmd
    local out
    if out="$(HOME="$tmp/home" decide "$2")"; then
      echo "FAIL $1 (expected pass, denied: $out)" >&2
      fails=$((fails + 1))
    else
      echo "ok   $1"
    fi
  }
  expect_deny() { # $1=名前 $2=cmd $3=期待する部分文字列(省略可)
    local out
    if out="$(HOME="$tmp/home" decide "$2")"; then
      if [[ -n ${3-} ]] && ! grep -qF -- "$3" <<< "$out"; then
        echo "FAIL $1 (denied but missing substring [$3]: $out)" >&2
        fails=$((fails + 1))
      else
        echo "ok   $1"
      fi
    else
      echo "FAIL $1 (expected deny, but passed)" >&2
      fails=$((fails + 1))
    fi
  }

  # --- 実在する skill/hook/AGENTS.md 見出しのフィクスチャ ---
  mkdir -p "$tmp/home/.claude/skills/precedent-grounding"
  : > "$tmp/home/.claude/skills/precedent-grounding/SKILL.md"
  mkdir -p "$tmp/home/.claude/hooks"
  : > "$tmp/home/.claude/hooks/foo-gate.sh"
  mkdir -p "$tmp/home/.agents"
  printf '# 見出しテスト\n\nある節\n' > "$tmp/home/.agents/AGENTS.md"

  expect_pass "feedback ラベルなしは対象外" \
    "gh issue create --label bug --body 'Target: skill/precedent-grounding'"

  expect_deny "feedback + Target なし" \
    "gh issue create --label feedback --body '本文だけ'" \
    "Target: 行がありません"

  expect_pass "feedback + 実在する skill/" \
    "gh issue create --label feedback --body 'Target: skill/precedent-grounding'"

  expect_deny "feedback + 実在しない skill/" \
    "gh issue create --label feedback --body 'Target: skill/nonexistent'" \
    "実在しません"

  expect_pass "feedback + 実在する hook/" \
    "gh issue create --label feedback --body 'Target: hook/foo-gate'"

  expect_pass "feedback + 実在する agents-md/" \
    "gh issue create --label feedback --body 'Target: agents-md/見出しテスト'"

  expect_deny "feedback + 実在しない agents-md/" \
    "gh issue create --label feedback --body 'Target: agents-md/存在しない節'" \
    "実在しません"

  expect_pass "カンマ区切りラベルの中に feedback" \
    "gh issue create --label bug,feedback --body 'Target: hook/foo-gate'"

  expect_pass "gh issue edit は対象外" \
    "gh issue edit 1 --label feedback --body 'Target: skill/precedent-grounding'"

  expect_pass "コマンド位置外の綴りは発火しない" \
    "echo 'gh issue create --label feedback --body x'"

  expect_pass "本文が \$() は判定不能で通す" \
    "gh issue create --label feedback --body \"\$(echo x)\""

  local footer_cmd
  footer_cmd="$(printf "gh issue create --label feedback --body \"\$(cat <<%sEOF%s\nTarget: skill/precedent-grounding\nEOF\n)\"" "'" "'")"
  expect_pass "heredoc 本体に Target: がある" "$footer_cmd"

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  *) main ;;
esac
