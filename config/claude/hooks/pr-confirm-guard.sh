#!/usr/bin/env bash
# pr-confirm-guard.sh — PR 本文に (i) 未チェックの task list(`- [ ]`)、
# または (ii) `## 要確認` に Issue 参照を持たない項目があれば `gh pr create`/
# `gh pr edit` の呼び出し自体を deny する PreToolUse hook。
#
# 設計と根拠: docs/claude/pr-confirm-guard.md(このリポジトリ内)
#
# 動機(このリポジトリのグリルセッション、2026-09-30 — チェックボックスで
# 残タスクを払い出し、人の確認完了を待ってマージするフローを廃する決定):
# 「要確認」が人間にしかできない残作業を正しく1件に絞れていても、その手順の
# 粒度が甘いという失敗が pr-description スキル §6 の散文だけでは防げず2度
# 再発していた(1度目: tarotene/dotfiles#239。2度目: 別リポジトリで手順の
# 粒度が甘かったケース)。3度目の再発は「人待ちの未チェック task list を
# PR 本文に残したままマージする」慣行そのもの(社内の別リポジトリで観測
# された実例が動機)——確認者の予定が PR の寿命を決め、base が進むほど
# rebase 負債になる。
# 対処として「要確認」を人手ブロッキング項目を手順付きで書く節から、既に
# 払い出した後続 Issue へのポインタだけを書く節に転換し、未チェック task
# list そのものを本文に存在できない形にした。
#
# 対象コマンド: コマンド位置の `gh pr create --body ...` と
# `gh pr edit ... --body ...`(`-R/--repo` によるクロスリポジトリ指定にも
# 対応。`--body-file` 経由も同様に読む)。
#
# 発火は全リポジトリで無条件(owner スコープを持たない)。この規律は
# 「自分が書く PR 本文に人待ち作業を残さない」という書き手側の規律であり、
# attribution-guard.sh(自分の投稿本文の規律、owner スコープ無し)と同じ
# 類型 —— pr-title-guard.sh の ADR-0031 D4(squash title = main の履歴、
# tarotene 配下の merge 設定に依存する契約)とは根拠が異なるため、その
# 限定は継承しない。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / CMD_SEPS)は
# attribution-guard.sh を source して再利用する(pr-title-guard.sh /
# stack-base-guard.sh と同じ「1つの判定エンジンを source する」型 — 既存
# 手段: config/claude/hooks/attribution-guard.sh)。本文の抽出ロジック
# (extract_body)は attribution-guard.sh の decide_tokens() の本文抽出部分と
# 同じ形だが、マーカー判定はせず本文テキストをそのまま返す(pr-title-guard.sh
# が decide_tokens を呼ばず parse_pr_title_tokens を独自に書いたのと同じ理由
# — 対象フィールドが違う)。
#
# 判定基準:
#   (i)  本文全体(フェンス外・インラインコードスパン除去後)に、未チェック
#        の task list 行(`- [ ]` / `* [ ]` / `+ [ ]`、インデント可)が
#        1つでもあれば違反。完了した項目は `[x]`/`[X]` に倒すか、人の確認が
#        要るなら後続 Issue へ払い出して `## 要確認` に `- #N — <一言>` の
#        形で参照する。
#   (ii) `## 要確認`(または任意レベルの見出しで「要確認」を含むもの)が
#        本文にあれば、配下の各トップレベル項目(フェンス外・列頭の
#        `- `/`* `/`N. ` で始まる行から次の列頭マーカーまたは節末までを
#        1項目とする)ごとに、Issue への参照(`#<番号>` または
#        `github.com/<owner>/<repo>/issues/<番号>` の URL)を1つ以上持つ
#        ことを要求する。見出しが無ければ (ii) は無条件 pass。見出しは
#        あるが項目が無くても pass。
# (i)(ii) いずれの違反も同じ1つの deny メッセージに合流させ(`gh pr edit`
# 1回で両方直せるため)、`G_visual`(docs/claude/pr-gate.md)と同じ二層
# 分担を保つ ——「手順・払い出し先の Issue が実在し妥当か」はここでは判定
# せず pr-description スキル(LLM の判断)の責務にする。
#
# escape hatch: 環境変数 PR_CONFIRM_GUARD_ALLOW=1 で一時的に無効化する
# (pr-title-guard.sh と同型 — 本文タグ型の恒久エスケープではない)。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる
#   手動 e2e:   pr-confirm-guard.sh --check '<コマンド文字列>' [<project-dir>]
#   自己検査:   pr-confirm-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail
export LC_ALL=C

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

# ---------------------------------------------------------------------------
# コマンド位置判定(attribution-guard.sh の同名関数を上書きする)
# ---------------------------------------------------------------------------

is_target_at() {
  local i=$1 n=${#TOK[@]} base
  ((i + 2 < n)) || return 1
  base="${TOK[i]##*/}"
  [[ $base == gh ]] || return 1
  [[ ${TOK[i + 1]} == pr ]] || return 1
  case "${TOK[i + 2]}" in
    create) TARGET_KIND=create ;;
    edit) TARGET_KIND=edit ;;
    *) return 1 ;;
  esac
  return 0
}

# ---------------------------------------------------------------------------
# 本文抽出(attribution-guard.sh の decide_tokens() と同じ形。マーカー判定は
# せず本文テキストを EXTRACTED_BODY にセットする)
# ---------------------------------------------------------------------------

# $@=1コマンドぶんのトークン列; 本文が取れれば0で EXTRACTED_BODY にセット、
# 判定不能(フラグ無し・値が空・コマンド置換で中身不明)なら1。
extract_body() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0 have_flag=0 has_hd=0 p
  local -a texts=()

  for ((i = 0; i < n; i++)); do
    if [[ ${tok[i]} == *'<<'* ]]; then
      has_hd=1
      break
    fi
  done

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

  ((have_flag)) || return 1

  local text=""
  ((${#texts[@]} > 0)) && text="$(printf '%s\n' "${texts[@]}")"

  if ((has_hd)) && [[ -n ${HD_BODIES:-} ]]; then
    text+=$'\n'"$HD_BODIES"
  elif [[ $text == *'$('* || $text == *'`'* ]]; then
    return 1 # 本文がコマンド置換 → 中身が不明 → 判定不能で通す
  fi

  [[ -n ${text//[[:space:]]/} ]] || return 1

  EXTRACTED_BODY="$text"
  return 0
}

# ---------------------------------------------------------------------------
# フェンス・インラインコードスパンの除去(pr-gate.sh の strip_code_spans と
# 同じ簡略化 — GitHub が実際にどう解釈するかを判定基準にする)。
# ---------------------------------------------------------------------------

strip_code_spans() { # strip_code_spans <text>
  awk '
    BEGIN { fence = 0 }
    /^[[:space:]]*(```|~~~)/ { fence = 1 - fence; next }
    fence { next }
    { gsub(/`[^`]*`/, " "); print }
  ' <<< "$1"
}

# ---------------------------------------------------------------------------
# (i) 未チェックの task list — 本文のどこにあっても違反
# ---------------------------------------------------------------------------

UNCHECKED_TASK_RE='^[[:space:]]*[-*+][[:space:]]+\[[[:space:]]\]'

# $1=本文; 未チェック task list 行が1つでもあれば0、無ければ1。
body_has_unchecked_task() {
  local stripped
  stripped="$(strip_code_spans "$1")"
  grep -Eq -- "$UNCHECKED_TASK_RE" <<< "$stripped"
}

# ---------------------------------------------------------------------------
# (ii) `## 要確認` 節の切り出し・項目分割・Issue 参照の判定
# ---------------------------------------------------------------------------

# $1=本文; 「要確認」を含む見出し行の次行から、次の見出し行の手前までを返す
# (pr-gate.sh の before_after_has_fence と同じ簡略化 — 見出しレベルの追跡は
# しない)。見出しが無ければ空文字列。
find_confirm_section() {
  awk '
    BEGIN { insec = 0 }
    /^#+[[:space:]]/ {
      insec = ($0 ~ /要確認/) ? 1 : 0
      next
    }
    insec { print }
  ' <<< "$1"
}

# $1=節テキスト; グローバル配列 ITEMS に、フェンス外の列頭 `- `/`* `/`N. `
# で始まる各項目(次の列頭マーカーまたは節末まで)を積む。
collect_items() {
  ITEMS=()
  local cur="" started=0 infence=0 line
  while IFS= read -r line; do
    if [[ $line =~ ^(\`\`\`|~~~) ]]; then
      infence=$((1 - infence))
      ((started)) && cur+=$'\n'"$line"
      continue
    fi
    if ((! infence)) && { [[ $line =~ ^[-*][[:space:]] ]] || [[ $line =~ ^[0-9]+\.[[:space:]] ]]; }; then
      ((started)) && ITEMS+=("$cur")
      cur="$line"
      started=1
      continue
    fi
    ((started)) && cur+=$'\n'"$line"
  done <<< "$1"
  ((started)) && ITEMS+=("$cur")
}

# GitHub の Issue 参照(`#<番号>` または issues URL)。closing keyword と違い
# merge 挙動を左右しないので、参照先が実在するかは判定しない(pr-description
# スキルの責務)。
ISSUE_REF_RE='(^|[^[:alnum:]/])#[0-9]+|github\.com/[^/[:space:]]+/[^/[:space:]]+/issues/[0-9]+'

# $1=項目テキスト; Issue 参照があれば0、無ければ1(コードスパン内の引用は
# 数えない — G_link の inline-code 回帰ガードと同じ理由)。
item_has_issue_ref() {
  local stripped
  stripped="$(strip_code_spans "$1")"
  grep -Eq -- "$ISSUE_REF_RE" <<< "$stripped"
}

# $1=本文; `## 要確認` の項目に Issue 参照が無いものがあれば1行1件で出力して
# 0、見出し/項目が無ければ1、全項目に参照があっても1。
judge_confirm_section() {
  local body="$1" section
  section="$(find_confirm_section "$body")"
  [[ -n $section ]] || return 1

  collect_items "$section"
  ((${#ITEMS[@]} > 0)) || return 1

  local -a violations=()
  local i=1 it
  for it in "${ITEMS[@]}"; do
    item_has_issue_ref "$it" \
      || violations+=("項目${i}: Issue 参照(#N または .../issues/N の URL)がありません")
    i=$((i + 1))
  done

  ((${#violations[@]} > 0)) || return 1

  printf '%s\n' "${violations[@]}"
  return 0
}

# $1=本文; (i)(ii) いずれかの違反があれば案内メッセージを出力して0、両方とも
# 無ければ1。
judge_confirm() {
  local body="$1" section_out
  local -a violations=()

  if body_has_unchecked_task "$body"; then
    violations+=("未チェックの task list(\`- [ ]\`)が本文にあります。完了して
いれば \`[x]\` に倒し、人の確認が要るなら後続 Issue へ払い出して
\`## 要確認\` に \`- #N — <一言>\` の形で参照してください。")
  fi

  if section_out="$(judge_confirm_section "$body")"; then
    violations+=("\`## 要確認\` に Issue 参照の無い項目があります:
${section_out}")
  fi

  ((${#violations[@]} > 0)) || return 1

  printf 'PR 本文に次の不備があります:\n\n'
  printf '%s\n\n' "${violations[@]}"
  printf '各項目は次の形で書いてください(pr-description スキル §1):
  ## 検証
  - [x] <実施済みの確認>

  ## 要確認
  - #<Issue番号> — <一言>

一時的に無効化するには PR_CONFIRM_GUARD_ALLOW=1 を設定してください。'
  return 0
}

# ---------------------------------------------------------------------------
# 1 コマンド範囲のトークンから --body 系を抜き出して判定する
# ---------------------------------------------------------------------------

# $1=kind(create|edit) $2=project; 残り=範囲トークン。deny なら理由文を
# stdout に出して 0、通すなら非 0。
judge_confirm_range() {
  local kind="$1" project="$2"
  shift 2

  [[ ${PR_CONFIRM_GUARD_ALLOW:-0} != 1 ]] || return 1

  local -a tok=("$@")
  extract_body "${tok[@]}" || return 1

  judge_confirm "$EXTRACTED_BODY"
}

# ---------------------------------------------------------------------------
# コマンド文字列全体からの範囲切り出し(pr-title-guard.sh の
# decide_pr_title() と同じ設計)
# ---------------------------------------------------------------------------

decide_pr_confirm() {
  local cmd="$1" project="$2"
  split_heredoc "$cmd"

  TOK=()
  local t
  while IFS= read -r -d '' t; do TOK+=("$t"); done < <(tokenize "$CMD_NOHD") || true
  ((${#TOK[@]} > 0)) || return 1

  local n=${#TOK[@]} i at_cmd_pos=1
  local -a starts=() kinds=()
  for ((i = 0; i < n; i++)); do
    if ((at_cmd_pos)) && is_target_at "$i"; then
      starts+=("$i")
      kinds+=("$TARGET_KIND")
    fi
    if is_sep "${TOK[i]}"; then at_cmd_pos=1; else at_cmd_pos=0; fi
  done
  ((${#starts[@]} > 0)) || return 1

  local m=${#starts[@]} s e reason
  for ((i = 0; i < m; i++)); do
    s=${starts[i]}
    if ((i + 1 < m)); then e=${starts[i + 1]}; else e=$n; fi
    reason="$(judge_confirm_range "${kinds[i]}" "$project" "${TOK[@]:s:e - s}")" && {
      printf '%s' "$reason"
      return 0
    }
  done
  return 1
}

# ---------------------------------------------------------------------------
# hook 入出力(pr-title-guard.sh と同じ契約)
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

  local input tool project reason cmd
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)}" || project=""
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

  check() { # $1=名前 $2=期待 $3=実際
    if [[ $2 == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected [$2], got [$3])" >&2
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

  repo_tarotene="$tmp/repo-tarotene"
  mkdir -p "$repo_tarotene"
  git -C "$repo_tarotene" init -q
  git -C "$repo_tarotene" remote add origin https://github.com/tarotene/dotfiles.git

  repo_other="$tmp/repo-other"
  mkdir -p "$repo_other"
  git -C "$repo_other" init -q
  git -C "$repo_other" remote add origin https://github.com/example/example.git

  run_decide() { decide_pr_confirm "$1" "$2"; }

  good_body='Closes #1

## 検証
- [x] cargo test

## 要確認
- #42 — 実機での uart 受信確認'

  echo "チェックボックス無し・要確認無し:"

  rc=0
  run_decide "gh pr create --body 'Closes #1'" "$repo_tarotene" > /dev/null || rc=$?
  check "1 チェックボックス無し・要確認無しは pass" "1" "$rc"

  echo "適合本文:"

  rc=0
  run_decide "gh pr create --body '$good_body'" "$repo_tarotene" > /dev/null || rc=$?
  check "2 全チェック済み+要確認に Issue 参照は pass" "1" "$rc"

  upper_x='Closes #1

## 検証
- [X] cargo test'
  rc=0
  run_decide "gh pr create --body '$upper_x'" "$repo_tarotene" > /dev/null || rc=$?
  check "3 大文字 [X] も完了扱いで pass" "1" "$rc"

  url_ref='Closes #1

## 要確認
- https://github.com/tarotene/dotfiles/issues/42 — 実機での uart 受信確認'
  rc=0
  run_decide "gh pr create --body '$url_ref'" "$repo_tarotene" > /dev/null || rc=$?
  check "4 issues URL 参照も pass" "1" "$rc"

  heading_only='Closes #1

## 要確認'
  rc=0
  run_decide "gh pr create --body '$heading_only'" "$repo_tarotene" > /dev/null || rc=$?
  check "5 要確認見出しのみ・項目無しは pass" "1" "$rc"

  fenced_task='Closes #1

## 検証
```
- [ ] not a real checkbox (fenced example)
```'
  rc=0
  run_decide "gh pr create --body '$fenced_task'" "$repo_tarotene" > /dev/null || rc=$?
  check "6 fence 内の [ ] は数えない: pass" "1" "$rc"

  inline_task='Closes #1

## 検証
記法の例: `- [ ] foo` の形で書く。'
  rc=0
  run_decide "gh pr create --body '$inline_task'" "$repo_tarotene" > /dev/null || rc=$?
  check "7 インラインコードスパン内の [ ] は数えない: pass" "1" "$rc"

  echo "未チェック task list は deny:"

  unchecked='Closes #1

## 検証
- [x] cargo test
- [ ] 実機で確認'
  out=""
  if out="$(run_decide "gh pr create --body '$unchecked'" "$repo_tarotene")"; then
    check_contains "8 未チェック項目は deny" "task list" "$out"
  else
    check "8 deny 期待" "deny" "pass"
  fi

  unchecked_star='Closes #1

## 検証
* [ ] 実機で確認'
  out=""
  if out="$(run_decide "gh pr create --body '$unchecked_star'" "$repo_tarotene")"; then
    check_contains "9 * マーカーの未チェックも deny" "task list" "$out"
  else
    check "9 deny 期待" "deny" "pass"
  fi

  unchecked_indented='Closes #1

## 検証
  - [ ] インデント付きの未チェック'
  out=""
  if out="$(run_decide "gh pr create --body '$unchecked_indented'" "$repo_tarotene")"; then
    check_contains "10 インデント付き未チェックも deny" "task list" "$out"
  else
    check "10 deny 期待" "deny" "pass"
  fi

  unchecked_in_confirm='Closes #1

## 要確認
- [ ] #42 — 実機での uart 受信確認'
  out=""
  if out="$(run_decide "gh pr create --body '$unchecked_in_confirm'" "$repo_tarotene")"; then
    check_contains "11 要確認直下の未チェックも deny" "task list" "$out"
  else
    check "11 deny 期待" "deny" "pass"
  fi

  echo "要確認の Issue 参照欠落は deny:"

  no_ref='Closes #1

## 要確認
- 実機での uart 受信確認(担当者待ち)'
  out=""
  if out="$(run_decide "gh pr create --body '$no_ref'" "$repo_tarotene")"; then
    check_contains "12 Issue 参照無しは deny" "Issue 参照" "$out"
  else
    check "12 deny 期待" "deny" "pass"
  fi

  two_second_no_ref='Closes #1

## 要確認
- #10 — 資格情報の発行
- 実機での uart 受信確認(担当者待ち)'
  out=""
  if out="$(run_decide "gh pr create --body '$two_second_no_ref'" "$repo_tarotene")"; then
    check_contains "13 2項目中2番目だけ参照無しは deny(項目2を指す)" "項目2" "$out"
  else
    check "13 deny 期待" "deny" "pass"
  fi

  both_violations='Closes #1

## 検証
- [ ] cargo test

## 要確認
- 実機での uart 受信確認(担当者待ち)'
  out=""
  if out="$(run_decide "gh pr create --body '$both_violations'" "$repo_tarotene")"; then
    check_contains "14 両違反は1つの deny に合流(task list)" "task list" "$out"
    check_contains "14 両違反は1つの deny に合流(Issue 参照)" "Issue 参照" "$out"
  else
    check "14 deny 期待" "deny" "pass"
  fi

  echo "escape hatch / スコープ(全リポジトリで発火):"

  rc=0
  PR_CONFIRM_GUARD_ALLOW=1 \
    run_decide "gh pr create --body '$unchecked'" "$repo_tarotene" > /dev/null || rc=$?
  check "15 escape hatch は pass" "1" "$rc"

  out=""
  if out="$(run_decide "gh pr create --body '$unchecked'" "$repo_other")"; then
    check_contains "16 tarotene 以外でも deny(owner スコープを持たない)" "task list" "$out"
  else
    check "16 deny 期待" "deny" "pass"
  fi

  out=""
  if out="$(run_decide "gh pr create -R other/repo --body '$unchecked'" "$repo_tarotene")"; then
    check_contains "17 -R 指定でも deny" "task list" "$out"
  else
    check "17 deny 期待" "deny" "pass"
  fi

  echo "--body-file / 非コマンド位置:"

  bf="$tmp/body.md"
  printf '%s' "$unchecked" > "$bf"
  out=""
  if out="$(run_decide "gh pr create --body-file '$bf'" "$repo_tarotene")"; then
    check_contains "18 --body-file 経由も判定される" "task list" "$out"
  else
    check "18 deny 期待" "deny" "pass"
  fi

  rc=0
  run_decide "echo 'gh pr create --body x'" "$repo_tarotene" > /dev/null || rc=$?
  check "19 非コマンド位置は pass" "1" "$rc"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest) selftest ;;
    --check)
      proj="${3:-$PWD}"
      if reason="$(decide_pr_confirm "${2-}" "$proj")"; then
        printf 'deny: %s\n' "$reason"
        exit 1
      fi
      echo "pass"
      ;;
    *) main ;;
  esac
fi
