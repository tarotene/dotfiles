#!/usr/bin/env bash
# external-send-guard.sh — 外部宛メール・Slack への直接送信を deny し、
# 下書き作成(Gmail の create_draft / Slack の draft 系 tool)へ誘導する
# PreToolUse hook。
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
# Slack への拡張(#463、2026-09-25): 別プロジェクトでの作業中にユーザーから
# 「外部発信を伴うタスクは常に下書きに留めたい」というフィードバックがあり、
# Gmail と同じ理由(取り消しづらい・内容の最終確認は人間が行うべき)で
# Slack の送信系 tool にも同じ deny → draft 誘導を適用した。GitHub は
# 対象外(下の「対象範囲」参照)。
#
# 判定は 2 つ:
#   - Gmail: 外部宛(自分のアドレス以外を含む、または reply で宛先が暗黙)の
#     send_message / reply / forward → deny。create_draft・自分宛のみは通す。
#   - Slack: send_message / reply / schedule_message / post_message 系の
#     tool → 宛先(チャンネル)を問わず無条件 deny。draft 系 tool
#     (`slack_draft_message` / `send_message_draft` 等、名前に draft を
#     含む tool)は対象外。
#
# なぜ deny であって ask ではないか: attribution-guard.sh と同じ理由
# (PreToolUse で deny、Stop では取り返せない不可逆操作)に加え、この操作は
# Claude が自分で代替(create_draft / draft 系 tool)を実行できるので、ask
# で人間の手数を増やす理由がない。
#
# なぜ GitHub(Issue/PR コメント等)は対象外か: このリポジトリ自身の完了定義
# (`AGENTS.md`「実装タスクの完了定義」)が `gh pr create` / `gh issue
# comment` 等を確認を挟まず実行することを要求しており、GitHub への発信を
# 一律 deny すると自己矛盾する。GitHub 側の「取り消しづらさ」への対処は
# レビュープロセス(PR は merge されるまで訂正可能)に委ねる。
#
# なぜ Bash 経由の送信(curl/sendmail 等)・LINE・Web フォーム送信は対象外か:
# この hook が仲介できるのは Claude Code の PreToolUse イベントだけで、
# それ以外の経路は一切見ない(bleep README が引く Saltzer &
# Schroeder の complete mediation の限界と同じ)。Gmail/Slack の MCP tool
# という書き込み経路を確実に塞ぐことに範囲を絞った。
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

# 対象 tool 名(Gmail の送信系 3 種)。create_draft/update_draft/
# list_drafts 等の下書き系・get_thread 等の読み取り系は対象外。
GMAIL_TARGET_TOOL_RE='^mcp__.*Gmail.*__(send_message|reply|forward)$'

# 対象 tool 名(Slack の送信系)。tool 名に "draft" を含むもの
# (`slack_draft_message` 等)は下書き系として対象外にする — 下の
# decide_mcp 側で先に除外する。サーバー名は `mcp__.*[Ss]lack.*__` で
# 緩く見る(claude.ai コネクタの `mcp__claude_ai_Slack__*` 形と、Slack 公式
# MCP の `mcp__slack__slack_*` 形の両方を拾う)。
SLACK_TARGET_TOOL_RE='^mcp__.*[Ss]lack.*__(slack_)?(send_message|reply|schedule_message|post_message)$'

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
  local tool="$1" input="$2"

  # draft 系 tool は Gmail/Slack のどちらの対象パターンにもマッチしうる名前
  # (例: mcp__slack__slack_send_message_draft)を含みうるので、両方の
  # マッチングより先に「名前に draft を含む」を除外する。
  [[ $tool == *[Dd]raft* ]] && return 1

  if grep -qE "$SLACK_TARGET_TOOL_RE" <<< "$tool"; then
    slack_deny_reason
    return 0
  fi

  decide_gmail "$tool" "$input"
}

# $1=tool $2=input JSON; deny なら理由文を stdout に出して 0、通すなら非 0。
decide_gmail() {
  local tool="$1" input="$2" self_set has_to has_cc has_bcc
  local -a recipients=()
  local a

  grep -qE "$GMAIL_TARGET_TOOL_RE" <<< "$tool" || return 1

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
      gmail_deny_reason "(宛先未指定・スレッド由来のため判定不能)"
      return 0
    fi
    return 1 # to/cc/bcc が空配列で明示されている等、判定不能 → 通す
  fi

  for a in "${recipients[@]}"; do
    if ! addr_in_set "$a" "$self_set"; then
      gmail_deny_reason "${recipients[*]}"
      return 0
    fi
  done
  return 1 # 全宛先が自分のアドレス → 通す
}

# $1=宛先一覧(表示用文字列); deny 理由文を stdout に出す。
gmail_deny_reason() {
  printf '%s' "外部宛のメール送信は直接実行せず、Gmail の下書き作成(create_draft。返信は replyToMessageId 付き)を使ってください(deny)。宛先: $1。下書きを作成したら、ユーザーが Gmail 上で内容を確認・編集して送信します。あわせて、宛先アドレスが公式の一般問い合わせ窓口として文脈まで確認済みか(求人・採用等の別目的窓口の転用ではないか)を確認し、出典 URL・取得日を送信記録に残してください。"
}

# Slack 送信 deny 理由文を stdout に出す(#463)。宛先チャンネルは問わず
# 無条件 deny なので、Gmail 側と違い引数は取らない。draft 系 tool が
# 存在しない接続先の可能性もあるため、無い場合の代替(チャット本文に出す)
# も案内する。
slack_deny_reason() {
  printf '%s' "Slack への直接送信は行わず、下書き系 tool(例: slack_draft_message / send_message_draft。接続先の MCP サーバーに存在しない場合は本文をチャットに出力し、ユーザー自身が Slack へ貼り付けてください)を使ってください(deny)。下書きを作成/提示したら、ユーザーが内容を確認・編集して送信します。"
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

  # Slack(#463): 宛先チャンネルを問わず無条件 deny。draft 系 tool は対象外。
  # 12: claude.ai コネクタ形 send_message → deny
  expect_deny "12 slack claude.ai send_message" "mcp__claude_ai_Slack__send_message" \
    '{"tool_input":{"channel":"C123","text":"hi"}}'
  # 13: 公式 MCP 形 slack_send_message → deny
  expect_deny "13 slack 公式 MCP slack_send_message" "mcp__slack__slack_send_message" \
    '{"tool_input":{"channel":"C123","text":"hi"}}'
  # 14: reply → deny
  expect_deny "14 slack reply" "mcp__claude_ai_Slack__reply" \
    '{"tool_input":{"channel":"C123","thread_ts":"1.1","text":"hi"}}'
  # 15: schedule_message → deny(予約送信も取り消しづらい発信)
  expect_deny "15 slack schedule_message" "mcp__slack__slack_schedule_message" \
    '{"tool_input":{"channel":"C123","text":"hi","post_at":123}}'
  # 16: post_message → deny
  expect_deny "16 slack post_message" "mcp__claude_ai_Slack__post_message" \
    '{"tool_input":{"channel":"C123","text":"hi"}}'
  # 17: draft 系 tool(名前に draft を含む)は対象外 → 通す
  expect_pass "17 slack draft は非対象" "mcp__slack__slack_draft_message" \
    '{"tool_input":{"channel":"C123","text":"hi"}}'
  expect_pass "17b send_message_draft も非対象" "mcp__claude_ai_Slack__send_message_draft" \
    '{"tool_input":{"channel":"C123","text":"hi"}}'
  # 18: 無関係ツール(Slack の読み取り系)は非対象 → 通す
  expect_pass "18 slack 読み取り系は非対象" "mcp__claude_ai_Slack__search_messages" \
    '{"tool_input":{"query":"x"}}'
  # 19: Gmail の create_draft は draft 除外ではなく元々のパターン非一致で
  # 通る(回帰: draft 除外を先頭に足したことで壊れていないことの確認)
  expect_pass "19 gmail create_draft は引き続き非対象" "mcp__claude_ai_Gmail__create_draft" \
    '{"tool_input":{"to":["stranger@example.com"]}}'

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
