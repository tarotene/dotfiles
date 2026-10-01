#!/usr/bin/env bash
# codex-plan-gate.sh — Codex CLI の Plan mode(`<proposed_plan>` ブロック)
# に、Claude Code の ExitPlanMode 相当の機械検査を課す Stop hook
# (ADR-0032 Amendment #531、D1/D2)。
#
# 設計と根拠: docs/claude/codex-plan-gate.md(このリポジトリ内)
#
# Codex CLI に ExitPlanMode という UI 概念(ツールコール)は無く、Plan mode
# の提案は応答本文に埋め込まれた `<proposed_plan>...</proposed_plan>` ブロック
# として現れる(Codex TUI バイナリの文字列リテラルで確認、2026-09-28)。この
# hook は Stop イベントで直前の応答(`last_assistant_message`)から
# `<proposed_plan>` を検出し、見つかったときだけ既存の
# plan-scope-gate --check-plan / plan-precedent-gate --check を
# そのまま呼ぶ。新しい判定ロジックは一切持たない(D1)。両者は Rust バイナリ
# (crates/plan-scope-gate・crates/plan-precedent-gate、#412)なので `bash` を
# 介さず直接実行する。
#
# 無限 block 対策(D2): config/claude/hooks/pr-gate.sh と同じ設計 —
# `stop_hook_active` は見ず、session_id ごとの独自カウンタが上限
# (${CODEX_PLAN_GATE_MAX_BLOCKS:-4})に達したら 1 回だけ escalate して
# 以後そのセッションは無条件で通す。理由は pr-gate.sh のコメント参照:
# `stop_hook_active` 素通しだと block 直後の再呼び出しが判定に届かない
# (docs/claude/copilot-plan-review.md の「第二次の非収束」と同型)。
#
# 縮退(ADR-0005 の binary-existence gating に倣う):
#   jq 不在 / plan-scope-gate・plan-precedent-gate 不在 / 判定不能な
#   stdin は黙って exit 0(判定不能を deny に変えない)。
#
# エスケープハッチ: touch ~/.codex/codex-plan-gate/skip または
# SKIP_CODEX_PLAN_GATE=1(stack-base-guard.sh と同型)。
#
# hooks.json の登録は home/modules/claude.nix の registerCodexHooks 経由
# (Stop、matcher なし)。
#
# 使い方: 通常は Codex CLI から stdin JSON で呼ばれる(引数無し)。
#   codex-plan-gate.sh --selftest   回帰テスト。
set -uo pipefail

STATE_ROOT="${CODEX_PLAN_GATE_DIR:-$HOME/.codex/codex-plan-gate}"
STATE_DIR="$STATE_ROOT/state"
MAX_BLOCKS="${CODEX_PLAN_GATE_MAX_BLOCKS:-4}"

CLAUDE_HOOKS_DIR="${CODEX_PLAN_GATE_CLAUDE_HOOKS_DIR:-$HOME/.claude/hooks}"
PLAN_SCOPE_GATE="$CLAUDE_HOOKS_DIR/plan-scope-gate"
PLAN_PRECEDENT_GATE="$CLAUDE_HOOKS_DIR/plan-precedent-gate"

emit_block() { # $1=reason
  jq -n --arg reason "$1" '{decision: "block", reason: $reason}'
}

# 直前の応答本文から `<proposed_plan>...</proposed_plan>` の中身だけを
# 取り出す(タグの外側は破棄)。最初の1ブロックのみ対象 — Codex は1ターンに
# 1つしか proposed_plan を出さない前提(TUI が「complete replacement」を
# 要求する設計、tui/src 文字列リテラル参照)。
extract_plan() { # $1=last_assistant_message
  local msg="$1"
  [[ $msg == *"<proposed_plan>"* ]] || return 1
  msg="${msg#*<proposed_plan>}"
  msg="${msg%%</proposed_plan>*}"
  printf '%s' "$msg"
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0
  [[ -x $PLAN_SCOPE_GATE && -x $PLAN_PRECEDENT_GATE ]] || exit 0

  if [[ -e "$STATE_ROOT/skip" || "${SKIP_CODEX_PLAN_GATE:-0}" == "1" ]]; then
    exit 0
  fi

  local input event msg session_id
  input="$(cat)" || exit 0
  event="$(jq -r '.hook_event_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ $event == "Stop" ]] || exit 0

  msg="$(jq -r '.last_assistant_message // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $msg ]] || exit 0
  session_id="$(jq -r '.session_id // "unknown"' <<< "$input" 2> /dev/null)" || session_id="unknown"

  local plan_body
  plan_body="$(extract_plan "$msg")" || exit 0
  [[ -n $plan_body ]] || exit 0

  mkdir -p "$STATE_DIR" 2> /dev/null || exit 0
  local count_file escalated_file count
  count_file="$STATE_DIR/${session_id}.count"
  escalated_file="$STATE_DIR/${session_id}.escalated"
  [[ -e $escalated_file ]] && exit 0

  local plan_file
  plan_file="$(mktemp "${TMPDIR:-/tmp}/codex-plan-gate.XXXXXX.md" 2> /dev/null)" || exit 0
  trap 'rm -f "$plan_file"' EXIT
  printf '%s\n' "$plan_body" > "$plan_file"

  local out1 out2 rc1=0 rc2=0
  out1="$("$PLAN_SCOPE_GATE" --check-plan "$plan_file" 2>&1)" || rc1=$?
  out2="$("$PLAN_PRECEDENT_GATE" --check "$plan_file" 2>&1)" || rc2=$?

  if [[ $rc1 -eq 0 && $rc2 -eq 0 ]]; then
    exit 0
  fi

  count="$(cat "$count_file" 2> /dev/null || echo 0)"
  count=$((count + 1))
  printf '%s' "$count" > "$count_file" 2> /dev/null || true

  if [[ $count -ge $MAX_BLOCKS ]]; then
    touch "$escalated_file" 2> /dev/null || true
    exit 0
  fi

  local reason
  reason="Codex の Plan(<proposed_plan>)が要求インベントリ/先行例接地の形式検査を通過していません。修正してから再度 Plan を提案してください。"$'\n\n'
  [[ $rc1 -ne 0 ]] && reason+="--- plan-scope-gate ---"$'\n'"$out1"$'\n\n'
  [[ $rc2 -ne 0 ]] && reason+="--- plan-precedent-gate ---"$'\n'"$out2"
  emit_block "$reason"
  exit 0
}

selftest() {
  local fails=0 self tmp claude_hooks scope_bin precedent_bin out decision
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  claude_hooks="$tmp/claude-hooks"
  mkdir -p "$claude_hooks"
  scope_bin="$claude_hooks/plan-scope-gate"
  precedent_bin="$claude_hooks/plan-precedent-gate"

  local ok_or_fail
  ok_or_fail="$tmp/mode" # ファイルの有無で stub の pass/fail を切り替える

  cat > "$scope_bin" << STUB
#!/usr/bin/env bash
if [[ -e "$ok_or_fail" ]]; then echo "R1: 処分が未記載です"; exit 1; fi
echo "OK: 検査を通過しました。"
STUB
  chmod +x "$scope_bin"
  cat > "$precedent_bin" << 'STUB'
#!/usr/bin/env bash
echo "OK: 検査を通過しました。"
STUB
  chmod +x "$precedent_bin"

  run_hook() { # $1=stdin json
    CODEX_PLAN_GATE_DIR="$tmp/state-root" CODEX_PLAN_GATE_CLAUDE_HOOKS_DIR="$claude_hooks" \
      bash "$self" <<< "$1"
  }

  local with_plan
  with_plan="$(jq -nc --arg sid "sid1" '{hook_event_name:"Stop",session_id:$sid,last_assistant_message:"before\n<proposed_plan>\n## 要求インベントリ\n- R1: x — 段1で実装\n</proposed_plan>\nafter"}')"
  local without_plan
  without_plan="$(jq -nc --arg sid "sid2" '{hook_event_name:"Stop",session_id:$sid,last_assistant_message:"no plan here"}')"

  echo "1: proposed_plan 無し → 無出力"
  out="$(run_hook "$without_plan")"
  if [[ -n $out ]]; then
    echo "FAIL(無出力期待): 出力=$out" >&2
    fails=$((fails + 1))
  fi

  echo "2: proposed_plan あり、両gate OK → 無出力"
  out="$(run_hook "$with_plan")"
  if [[ -n $out ]]; then
    echo "FAIL(無出力期待): 出力=$out" >&2
    fails=$((fails + 1))
  fi

  echo "3: proposed_plan あり、scope-gate が指摘 → block"
  touch "$ok_or_fail"
  out="$(run_hook "$with_plan")"
  decision="$(jq -r '.decision // empty' <<< "$out" 2> /dev/null)"
  if [[ $decision != "block" ]]; then
    echo "FAIL(block 期待): 出力=$out" >&2
    fails=$((fails + 1))
  fi

  echo "4: 上限到達後は escalate して無条件で通す"
  local i
  for i in 1 2 3 4 5; do
    out="$(run_hook "$with_plan")"
  done
  decision="$(jq -r '.decision // empty' <<< "$out" 2> /dev/null)"
  if [[ -n $out ]]; then
    echo "FAIL(escalate 後は無出力期待、count=$i): 出力=$out" >&2
    fails=$((fails + 1))
  fi
  rm -f "$ok_or_fail"

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
