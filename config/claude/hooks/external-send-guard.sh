#!/usr/bin/env bash
# external-send-guard.sh — 外部宛メールの直接送信を deny し、Gmail の下書き
# 作成(create_draft)へ誘導する PreToolUse hook。
#
# 設計と根拠: docs/claude/external-send-guard.md(このリポジトリ内)
#
# 動機(motivating case): とある個人プロジェクトで、公式サイトに実在する
# メールアドレスを一般問い合わせに使ったところ、実際は求人問い合わせ専用の
# 窓口で、相手から「一般の方とはこのアドレスでやりとりすることは無い」との
# 指摘を受けた(2026-09-22)。アドレスの存在確認だけでは窓口の文脈までは
# 保証されない。機械では窓口文脈を判定できないので、外部宛送信そのものを
# 一律 deny し、ユーザーが Gmail 上で内容を確認・編集して送信する運用に
# 倒す(ユーザーの編集・送信そのものを承認点にする)。
#
# 判定は 1 つだけ:
#   外部宛(自分のアドレス以外を含む、または reply で宛先が暗黙)の
#   send_message / reply / forward → deny。create_draft・自分宛のみは通す。
#
# なぜ deny であって ask ではないか: attribution-guard.sh と同じ理由
# (PreToolUse で deny、Stop では取り返せない不可逆操作)に加え、この操作は
# Claude が自分で代替(create_draft)を実行できるので、ask で人間の手数を
# 増やす理由がない。
#
# なぜ Bash 経由の送信(curl/sendmail 等)・LINE・Web フォーム送信は対象外か:
# この hook が仲介できるのは Claude Code の PreToolUse イベントだけで、
# それ以外の経路は一切見ない(bleep README が引く Saltzer &
# Schroeder の complete mediation の限界と同じ)。Gmail MCP tool という
# 単一の書き込み経路を確実に塞ぐことに範囲を絞った。
#
# 自分のアドレス判定: ${XDG_CONFIG_HOME:-~/.config}/external-send-guard/self.txt
# (1行1アドレス、# コメント・空行は無視)。bleep と同じ理由で、
# 個人のメールアドレスをこのリポジトリにコミットしない —
# ファイルが無い/空なら「自分宛アドレスは0件」として扱う(fail-closed:
# 全ての送信を外部宛とみなして deny する)。これは安全側のデフォルトで、
# 代替手段(create_draft)は常に使えるため実用上の支障はない。
#
# bypass: 環境変数 EXTERNAL_SEND_GUARD_ALLOW=1 が立っていると即座に pass する。
# bleep と同じ方針で、この env var 名は deny の理由文には書かない
# (制約される当事者が自分で bypass を再実行できてしまうため)。
#
# 縮退(ADR-0005 の binary-existence gating に倣う): jq 不在・stdin 不正は
# 黙って exit 0。判定できない場合は断定に変えず素通す。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "mcp__.*")から
#                stdin JSON で呼ばれる
#   自己検査:   external-send-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail

export LC_ALL=C

# 対象 tool 名(Gmail の送信系 3 種のみ)。create_draft/update_draft/
# list_drafts 等の下書き系・get_thread 等の読み取り系は対象外。
TARGET_TOOL_RE='^mcp__.*Gmail.*__(send_message|reply|forward)$'

# $1=tool_input JSON; self.txt のパスを返す。
self_list_path() {
  printf '%s/external-send-guard/self.txt' "${XDG_CONFIG_HOME:-$HOME/.config}"
}

# 自分のアドレス集合(小文字化、改行区切り)を stdout に出す。ファイルが
# 無ければ何も出さない(= 自分宛アドレス0件、fail-closed)。改行区切りに
# するのは、NUL 区切りだと `$(...)` コマンド置換が NUL を保持できず
# (bash の文字列は NUL 終端)、呼び出し元の変数代入で切れてしまうため
# (メールアドレスに改行は含まれ得ないので、区切りとして安全)。
load_self_addresses() {
  local path
  path="$(self_list_path)"
  [[ -f $path && -r $path ]] || return 0
  grep -Ev '^[[:space:]]*(#|$)' -- "$path" 2> /dev/null \
    | tr '[:upper:]' '[:lower:]' \
    | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' \
    | grep -v '^$' || true
}

# $1=アドレス $2=self 集合(改行区切り文字列); 集合に含まれれば 0。
addr_in_set() {
  local needle="${1,,}" hay="$2" item
  while IFS= read -r item; do
    [[ -n $item && $item == "$needle" ]] && return 0
  done <<< "$hay"
  return 1
}

# $1=tool $2=input JSON; deny なら理由文を stdout に出して 0、通すなら非 0。
decide_mcp() {
  local tool="$1" input="$2" self_set has_to has_cc has_bcc
  local -a recipients=()
  local a

  grep -qE "$TARGET_TOOL_RE" <<< "$tool" || return 1

  self_set="$(load_self_addresses)"

  has_to="$(jq -r 'has("to")' <<< "$(jq -c '.tool_input' <<< "$input")" 2> /dev/null)" || has_to=false
  has_cc="$(jq -r 'has("cc")' <<< "$(jq -c '.tool_input' <<< "$input")" 2> /dev/null)" || has_cc=false
  has_bcc="$(jq -r 'has("bcc")' <<< "$(jq -c '.tool_input' <<< "$input")" 2> /dev/null)" || has_bcc=false

  while IFS= read -r a; do
    [[ -n $a ]] && recipients+=("$a")
  done < <(jq -r '(.tool_input.to // [])[], (.tool_input.cc // [])[], (.tool_input.bcc // [])[]' <<< "$input" 2> /dev/null || true)

  # reply で to/cc/bcc が全く指定されていない場合、実際の宛先はスレッド由来
  # (元メールの送信者)で hook からは分からない。判定不能を「安全側」に倒し
  # deny する(create_draft + replyToMessageId で下書きにすれば同じ結果が
  # 得られ、かつ承認点を挟める)。
  if [[ ${#recipients[@]} -eq 0 ]]; then
    if [[ $tool == *reply* && $has_to == false && $has_cc == false && $has_bcc == false ]]; then
      deny_reason "(宛先未指定・スレッド由来のため判定不能)"
      return 0
    fi
    return 1 # to/cc/bcc が空配列で明示されている等、判定不能 → 通す
  fi

  for a in "${recipients[@]}"; do
    if ! addr_in_set "$a" "$self_set"; then
      deny_reason "${recipients[*]}"
      return 0
    fi
  done
  return 1 # 全宛先が自分のアドレス → 通す
}

# $1=宛先一覧(表示用文字列); deny 理由文を stdout に出す。
deny_reason() {
  printf '%s' "外部宛のメール送信は直接実行せず、Gmail の下書き作成(create_draft。返信は replyToMessageId 付き)を使ってください(deny)。宛先: $1。下書きを作成したら、ユーザーが Gmail 上で内容を確認・編集して送信します。あわせて、宛先アドレスが公式の一般問い合わせ窓口として文脈まで確認済みか(求人・採用等の別目的窓口の転用ではないか)を確認し、出典 URL・取得日を送信記録に残してください。"
}

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
  [[ "${EXTERNAL_SEND_GUARD_ALLOW:-}" == 1 ]] && exit 0

  local input tool reason
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $tool ]] || exit 0

  reason="$(decide_mcp "$tool" "$input")" || exit 0
  emit_deny "$reason"
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

  expect_deny() { # $1=ラベル $2=tool $3=input JSON
    local out
    if ! out="$(decide_mcp "$2" "$3")"; then
      echo "FAIL($1: deny 期待、通した)" >&2
      fails=$((fails + 1))
    elif [[ -z $out ]]; then
      echo "FAIL($1: deny 期待、理由が空)" >&2
      fails=$((fails + 1))
    fi
  }
  expect_pass() { # $1=ラベル $2=tool $3=input JSON
    if decide_mcp "$2" "$3" > /dev/null; then
      echo "FAIL($1: pass 期待、deny した)" >&2
      fails=$((fails + 1))
    fi
  }

  mkdir -p "$tmp/cfg/external-send-guard"
  printf '# comment\nme@example.com\n\nalt@example.com\n' > "$tmp/cfg/external-send-guard/self.txt"
  export XDG_CONFIG_HOME="$tmp/cfg"

  # 1: 自分宛のみの send_message → 通す
  expect_pass "1 自分宛 send_message" "mcp__claude_ai_Gmail__send_message" \
    '{"tool_input":{"to":["me@example.com"]}}'
  # 2: 外部宛を含む send_message → deny
  expect_deny "2 外部宛 send_message" "mcp__claude_ai_Gmail__send_message" \
    '{"tool_input":{"to":["stranger@example.com"]}}'
  # 3: cc に外部宛を含む → deny
  expect_deny "3 cc 外部宛" "mcp__claude_ai_Gmail__send_message" \
    '{"tool_input":{"to":["me@example.com"],"cc":["stranger@example.com"]}}'
  # 4: reply で to/cc/bcc すべて未指定 → 判定不能 → deny(安全側)
  expect_deny "4 reply 宛先暗黙" "mcp__claude_ai_Gmail__reply" \
    '{"tool_input":{"messageId":"abc"}}'
  # 5: reply で自分宛のみ明示 → 通す
  expect_pass "5 reply 自分宛明示" "mcp__claude_ai_Gmail__reply" \
    '{"tool_input":{"messageId":"abc","to":["alt@example.com"]}}'
  # 6: reply で外部宛明示 → deny
  expect_deny "6 reply 外部宛明示" "mcp__claude_ai_Gmail__reply" \
    '{"tool_input":{"messageId":"abc","to":["stranger@example.com"]}}'
  # 7: forward で外部宛 → deny
  expect_deny "7 forward 外部宛" "mcp__claude_ai_Gmail__forward" \
    '{"tool_input":{"to":["stranger@example.com"]}}'
  # 8: create_draft は対象外 → 通す(tool 名で非対象)
  expect_pass "8 create_draft 非対象" "mcp__claude_ai_Gmail__create_draft" \
    '{"tool_input":{"to":["stranger@example.com"]}}'
  # 9: 無関係ツール(get_thread) → 通す
  expect_pass "9 get_thread 非対象" "mcp__claude_ai_Gmail__get_thread" \
    '{"tool_input":{"threadId":"1"}}'
  # 10: 大文字小文字の違いを無視して自分宛と判定
  expect_pass "10 大文字小文字無視" "mcp__claude_ai_Gmail__send_message" \
    '{"tool_input":{"to":["ME@EXAMPLE.COM"]}}'
  # 11: self.txt が存在しない(fail-closed: 自分のアドレス宛でも deny)
  XDG_CONFIG_HOME="$tmp/cfg-empty"
  expect_deny "11 self.txt 不在は fail-closed" "mcp__claude_ai_Gmail__send_message" \
    '{"tool_input":{"to":["me@example.com"]}}'
  XDG_CONFIG_HOME="$tmp/cfg"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest) selftest ;;
    *) main ;;
  esac
fi
