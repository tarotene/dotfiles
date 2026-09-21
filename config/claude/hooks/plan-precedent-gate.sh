#!/usr/bin/env bash
# plan-precedent-gate.sh — ExitPlanMode 直前に、先行例との対比(config/claude/
# CLAUDE.md の「発明する前に先行例を確認する」節、config/claude/skills/
# precedent-grounding/SKILL.md)の欠落を機械的に検査する hook。
#
# 設計と根拠: docs/adr/0012-precedent-grounding-over-prompted-adversarial-
# review.md、docs/claude/precedent-grounding.md(このリポジトリ内)
#
# plan-scope-gate.sh と同じ二層構造(docs/claude/precedent-grounding.md
# 「なぜ形式は機械 gate、内容は critic の報告にしたか」節): 形式(節または
# 免除行が存在するか、各 Dn に必要な要素があるか)は LLM を呼ばずここで検査
# する。内容(引用が本当に主張を支えているか)は copilot-plan-review.sh の
# lens A に残す — この gate は文字列の形しか見ない。
#
#   - `## 先行例との対比` 節が無ければ、`先行例: 該当なし — <理由>` の
#     免除行(1行、ダッシュ種は — / – / - のいずれでも可)があるかを見る。
#     どちらも無ければ deny。
#   - 節がある場合、`- Dn:` 行が1件以上あるか・重複が無いかを見る。各 Dn は
#     `先行例なし: <非空テキスト>`、または
#     `先行例: <出典(URL・#N・owner/repo#N・パス風文字列のいずれか)>` +
#     `(取得 YYYY-MM-DD)` + `差分:(一致|異なる)` のいずれかを満たすことを
#     見る。満たさない要素があれば、その Dn について deny(不足要素を列挙)。
#   - allow は決して返さない。問題が無ければ何も決定しない(exit 0)。
#
# 既知の限界(意図的な選択):
#   - 出典トークンの**存在**しか見ない。URL が実在するか、パスが本当に
#     その主張を支えるかは検査しない(critic の職責、docs/claude/
#     precedent-grounding.md 参照)。
#   - 取得日の妥当性(未来日付・古すぎる日付)は検査しない。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: ExitPlanMode)から
#                stdin JSON で呼ばれる
#   手動 e2e:   plan-precedent-gate.sh --check <plan.md>
#   自己検査:   plan-precedent-gate.sh --selftest(ネットワーク不使用)
#
# スキップ手段:
#   touch ~/.claude/plan-precedent-gate/skip   または   SKIP_PLAN_PRECEDENT_GATE=1
#
# 既知バグ対策:
#   - ExitPlanMode hook は cwd=~ で走る → stdin JSON の .cwd へ明示 cd
#     (anthropics/claude-code#22343、copilot-plan-review.sh / plan-scope-gate.sh
#     と同じ対策)。
set -u

GATE_DIR="${CLAUDE_PLAN_PRECEDENT_GATE_DIR:-$HOME/.claude/plan-precedent-gate}"

# ---------------------------------------------------------------------------
# hook 出力(plan-scope-gate.sh と同じ契約)
# ---------------------------------------------------------------------------

pass_through() { # $1=警告メッセージ(省略可)
  local msg="${1:-}"
  [[ -n "$msg" ]] && jq -n --arg m "$msg" '{systemMessage: $m}'
  exit 0
}

deny_with() { # $1=理由(Claude に届く)
  local reason="$1"
  if [[ "${EVENT:-PreToolUse}" == "PermissionRequest" ]]; then
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PermissionRequest", decision: {behavior: "deny", message: $m}}}'
  else
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
  fi
  exit 0
}

# ---------------------------------------------------------------------------
# 抽出・判定ロジック(selftest がネットワーク無しに検査できる)
# ---------------------------------------------------------------------------

EXEMPT_RE='先行例:[[:space:]]*該当なし[[:space:]]*[—–-][[:space:]]*[^[:space:]]'
DN_RE='^[[:space:]]*[-*]?[[:space:]]*\*{0,2}D([0-9]+)\*{0,2}[:.)]'
CITATION_RE='(https?://[^[:space:])]+|#[0-9]+|[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+)'
DATE_RE='取得[[:space:]]*[0-9]{4}-[0-9]{2}-[0-9]{2}'
DIFF_RE='差分:[[:space:]]*(一致|異なる)'
NONE_RE='先行例なし:[[:space:]]*[^[:space:]]'

# そのまま貼れば書式検査を通る完全な例文ブロック。deny メッセージ末尾と
# --check の指摘あり出力に同梱する(2026-09-21 実測: 本 gate の deny の
# 68% が同一セッションで反復していた — 不足要素名だけでは書式の綴りが
# 当たらず往復していたため、通る完全形を直接見せる。docs/claude/
# precedent-grounding.md「実測」節参照)。
#
# 取得日は実行時の日付を動的生成する(固定プレースホルダだと DATE_RE に
# 一致せず、丸写しした瞬間に再 deny してしまうため)。selftest で
# 「この関数の出力が judge_precedent を無条件に通る」ことを機械検証する。
example_block() {
  local today
  today="$(date +%Y-%m-%d)"
  cat <<EOF
そのまま構造を写し、山括弧の中身だけ事実に置き換えれば書式検査は通ります:

## 先行例との対比

- D1: <採った設計判断を1文で>
  先行例: <著者/組織, タイトル> https://example.com/doc (取得 ${today})
  差分: 一致
- D2: <採った設計判断を1文で>
  先行例なし: <どこを・何のキーワードで・一次/二次のどちらまで探したか>

出典は URL のほか #123 / owner/repo#123 / リポジトリ内パス でも可。
先行例から意図的に外れた場合は「差分: 異なる — <理由>」。
設計判断を含まないプランなら、節の代わりに次の1行だけ:

先行例: 該当なし — <理由(例: typo 修正で設計判断を含まない)>
EOF
}

# `## 先行例との対比` 節の本文だけを出力(次の見出しまで)。無ければ何も出さない。
extract_precedent_section() {
  awk '
    BEGIN { insec = 0 }
    /^#+[[:space:]]*先行例との対比/ { insec = 1; print; next }
    insec && /^#+[[:space:]]/ { exit }
    insec { print }
  ' <<< "$1"
}

# $1=id $2=block(Dn行を含む複数行テキスト) ; 問題があれば1行1件で出力
check_dn_block() {
  local id="$1" block="$2"

  grep -Eq "$NONE_RE" <<< "$block" && return 0

  if grep -Eq '先行例:' <<< "$block"; then
    local -a missing=()
    grep -Eq "$CITATION_RE" <<< "$block" || missing+=("出典(URL・#N・owner/repo#N・リポジトリ内パスのいずれか)")
    grep -Eq "$DATE_RE" <<< "$block" || missing+=("取得日(「(取得 YYYY-MM-DD)」の形)")
    grep -Eq "$DIFF_RE" <<< "$block" || missing+=("差分:(一致|異なる)")
    if ((${#missing[@]} > 0)); then
      local joined i
      joined="${missing[0]}"
      for ((i = 1; i < ${#missing[@]}; i++)); do
        joined+="、${missing[$i]}"
      done
      printf 'D%s: 先行例の記載に不足があります — %s\n' "$id" "$joined"
    fi
    return 0
  fi

  printf 'D%s には「先行例:」または「先行例なし:」の記載がありません\n' "$id"
}

# $1=plan_body ; 見つかった問題を1行1件で出力(無ければ何も出さない)
judge_precedent() {
  local plan="$1" section
  section="$(extract_precedent_section "$plan")"

  if [[ -z $section ]]; then
    if grep -Eq "$EXEMPT_RE" <<< "$plan"; then
      return 0
    fi
    printf '%s\n' '`## 先行例との対比` 節が見つかりません。非自明な設計判断ごとに Dn 行で先行例と対比するか、設計判断が無いなら `先行例: 該当なし — <理由>` の1行を書いてください(precedent-grounding スキル参照)。'
    return 0
  fi

  local -A seen=()
  local -A blocks=()
  local -a ids=()
  local current_id="" line

  while IFS= read -r line; do
    if [[ $line =~ $DN_RE ]]; then
      current_id="${BASH_REMATCH[1]}"
      if [[ -n "${seen[$current_id]:-}" ]]; then
        printf 'D%s が複数回出現しています(重複)\n' "$current_id"
      else
        ids+=("$current_id")
      fi
      seen[$current_id]=1
      blocks[$current_id]="$line"
    elif [[ -n $current_id ]]; then
      blocks[$current_id]+=$'\n'"$line"
    fi
  done <<< "$section"

  if ((${#ids[@]} == 0)); then
    printf '%s\n' '`## 先行例との対比` 節に `- D1:` のような設計判断の行が1件もありません。'
    return 0
  fi

  local id
  for id in "${ids[@]}"; do
    check_dn_block "$id" "${blocks[$id]}"
  done
}

# ---------------------------------------------------------------------------
# hook モード
# ---------------------------------------------------------------------------

main() {
  command -v jq > /dev/null 2>&1 || pass_through

  if [[ -e "$GATE_DIR/skip" || "${SKIP_PLAN_PRECEDENT_GATE:-0}" == "1" ]]; then
    pass_through
  fi

  local INPUT EVENT CWD
  INPUT="$(cat)"
  EVENT="$(jq -r '.hook_event_name // "PreToolUse"' <<< "$INPUT")"
  CWD="$(jq -r '.cwd // empty' <<< "$INPUT")"
  [[ -d $CWD ]] || CWD="$HOME"
  cd -- "$CWD" 2> /dev/null || true

  # プラン本文の取得: tool_input.plan → planFilePath → 最新の ~/.claude/plans/*.md
  local plan_text plan_path plan_body=""
  plan_text="$(jq -r '.tool_input.plan // empty' <<< "$INPUT")"
  plan_path="$(jq -r '.tool_input.planFilePath // empty' <<< "$INPUT")"
  if [[ -n $plan_text ]]; then
    plan_body="$plan_text"
  elif [[ -n $plan_path && -f $plan_path ]]; then
    plan_body="$(cat "$plan_path")"
  else
    local latest_plan
    latest_plan="$(ls -t "$HOME/.claude/plans/"*.md 2> /dev/null | head -1)"
    [[ -n $latest_plan ]] || pass_through
    plan_body="$(cat "$latest_plan")"
  fi

  local -a deny_lines=()
  local problem
  while IFS= read -r problem; do
    [[ -n $problem ]] && deny_lines+=("$problem")
  done < <(judge_precedent "$plan_body")

  if ((${#deny_lines[@]} == 0)); then
    pass_through
  fi

  local msg
  msg="先行例との対比の検査で問題が見つかりました。precedent-grounding スキルの手順に従って計画を修正してください。

$(printf '%s\n' "${deny_lines[@]}")

$(example_block)"
  deny_with "$msg"
}

# ---------------------------------------------------------------------------
# --check(手動 e2e)
# ---------------------------------------------------------------------------

cmd_check() { # $1=plan_file
  local plan_file="$1" plan_body problem found=0
  [[ -f $plan_file ]] || {
    echo "plan file not found: $plan_file" >&2
    exit 1
  }
  plan_body="$(cat "$plan_file")"
  while IFS= read -r problem; do
    [[ -n $problem ]] || continue
    found=1
    echo "$problem"
  done < <(judge_precedent "$plan_body")
  if [[ $found -eq 0 ]]; then
    echo "OK: 先行例との対比の検査を通過しました。"
  else
    printf '\n%s\n' "$(example_block)"
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# --selftest(ネットワークを使わない)
# ---------------------------------------------------------------------------

selftest() {
  local fails=0
  expect_empty() { # $1=actual(複数行) $2=label
    [[ -z "$1" ]] || {
      echo "FAIL(${2}): 空を期待したが得た: [${1}]" >&2
      fails=$((fails + 1))
    }
  }
  expect_nonempty() { # $1=actual $2=label
    [[ -n "$1" ]] || {
      echo "FAIL(${2}): 非空を期待したが空だった" >&2
      fails=$((fails + 1))
    }
  }
  expect_count() { # $1=actual(複数行) $2=expected件数 $3=label
    local n
    n="$(grep -c . <<< "$1" 2> /dev/null || echo 0)"
    [[ -z "$1" ]] && n=0
    [[ "$n" == "$2" ]] || {
      echo "FAIL(${3}): 期待件数=${2} 実際=${n}: [${1}]" >&2
      fails=$((fails + 1))
    }
  }

  # --- 節なし・免除行なし → deny(1件) ---
  local out
  out="$(judge_precedent $'依頼: 何かをする\n')"
  expect_count "$out" 1 "節なし・免除行なし"

  # --- 免除行のみ(節なし) → pass ---
  out="$(judge_precedent $'依頼の説明\n先行例: 該当なし — typo 修正で設計判断を含まない\n')"
  expect_empty "$out" "免除行(em dash)"

  # --- 免除行の別ダッシュ種(ASCII hyphen) → pass ---
  out="$(judge_precedent $'依頼の説明\n先行例: 該当なし - typo 修正\n')"
  expect_empty "$out" "免除行(hyphen)"

  # --- 免除行の別ダッシュ種(en dash) → pass ---
  out="$(judge_precedent $'依頼の説明\n先行例: 該当なし \xe2\x80\x93 typo 修正\n')"
  expect_empty "$out" "免除行(en dash)"

  # --- 節あり、全項目適合(先行例あり + 先行例なし混在) → pass ---
  local plan_ok
  plan_ok=$'## 先行例との対比\n\n- D1: 三層構成で規律を配置する\n  先行例: 自リポジトリ scope-inventory — config/claude/CLAUDE.md (取得 2026-09-15)\n  差分: 一致\n- D2: 別の判断\n  先行例なし: 公式ドキュメントと社内 ADR を検索したが見つからなかった\n'
  out="$(judge_precedent "$plan_ok")"
  expect_empty "$out" "節あり・全項目適合"

  # --- 節あり、Dn 行が1件もない → deny(1件) ---
  out="$(judge_precedent $'## 先行例との対比\n\n自由記述だけで Dn 行が無い\n')"
  expect_count "$out" 1 "節あり・Dn行なし"

  # --- 節あり、取得日欠落 → deny(D1 について1件) ---
  local plan_missing_date
  plan_missing_date=$'## 先行例との対比\n\n- D1: 判断\n  先行例: https://example.com/x\n  差分: 一致\n'
  out="$(judge_precedent "$plan_missing_date")"
  expect_nonempty "$out" "取得日欠落"
  grep -q '^D1:' <<< "$out" || {
    echo "FAIL(取得日欠落): D1 の指摘が出ていない: [${out}]" >&2
    fails=$((fails + 1))
  }

  # --- 節あり、差分欠落 → deny ---
  local plan_missing_diff
  plan_missing_diff=$'## 先行例との対比\n\n- D1: 判断\n  先行例: https://example.com/x (取得 2026-09-15)\n'
  out="$(judge_precedent "$plan_missing_diff")"
  expect_nonempty "$out" "差分欠落"

  # --- 節あり、先行例/先行例なしのどちらも無い Dn → deny ---
  out="$(judge_precedent $'## 先行例との対比\n\n- D1: 判断だけ書いた\n')"
  expect_nonempty "$out" "先行例記載なし"

  # --- 節あり、重複 Dn → deny(重複1件 + 内容は適合) ---
  local plan_dup
  plan_dup=$'## 先行例との対比\n\n- D1: 判断\n  先行例なし: 探索範囲\n- D1: 別の判断\n  先行例なし: 探索範囲2\n'
  out="$(judge_precedent "$plan_dup")"
  expect_nonempty "$out" "重複 Dn"
  grep -q '複数回出現' <<< "$out" || {
    echo "FAIL(重複 Dn): 重複メッセージが出ていない: [${out}]" >&2
    fails=$((fails + 1))
  }

  # --- 太字マーカー付き Dn 行(- **D1:**)も認識する ---
  local plan_bold
  plan_bold=$'## 先行例との対比\n\n- **D1:** 判断\n  先行例なし: 探索範囲\n'
  out="$(judge_precedent "$plan_bold")"
  expect_empty "$out" "太字マーカー付き Dn"

  # --- extract_precedent_section: 次の見出しで止まる ---
  local sec
  sec="$(extract_precedent_section $'前文\n## 先行例との対比\n- D1: x\n## 次の節\n無関係\n')"
  grep -q '次の節' <<< "$sec" && {
    echo "FAIL: extract_precedent_section が次の見出し以降まで含んでいる" >&2
    fails=$((fails + 1))
  }

  # --- example_block は無条件に judge_precedent を通る(deny 文の自己整合性) ---
  out="$(judge_precedent "$(example_block)")"
  expect_empty "$out" "example_block が gate を通過する"

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  --check)
    shift
    cmd_check "${1-}"
    ;;
  *) main ;;
esac
