#!/usr/bin/env bash
# pr-confirm-guard.sh — PR 本文の `## 要確認` に、機械的下限(閉語彙の
# ブロッキング理由・番号手順・完了確認: 行)を満たさない項目があれば
# `gh pr create`/`gh pr edit` の呼び出し自体を deny する PreToolUse hook。
#
# 設計と根拠: docs/claude/pr-confirm-guard.md(このリポジトリ内)
#
# 動機(ADR-543 D2 の昇格判断): 「要確認」が人間にしかできない残作業を正しく
# 1件に絞れていても、その手順の粒度が甘いという失敗が pr-description スキル
# §6 の散文だけでは防げず2度再発した(1度目: tarotene/dotfiles#239、残作業を
# 全部要確認に書いてしまった。2度目: 別リポジトリで、ブロッキング理由は正しい
# 1件だが「クラウド環境の credentials に登録する」とだけ書き、画面遷移・
# 完了確認が無かった)。散文からの昇格条件(同じ規範違反の再発)に該当するため
# PreToolUse gate へ昇格する。
#
# 対象コマンド: コマンド位置の `gh pr create --body ...` と
# `gh pr edit ... --body ...`(`-R/--repo` によるクロスリポジトリ指定にも
# 対応。`--body-file` 経由も同様に読む)。
#
# 発火は owner が tarotene のリポジトリに限定する(pr-title-guard.sh と同じ
# 理由、ADR-0031 D4 — 会社ホストにも common 層として配備されるため)。owner
# が解決できない場合は fail-open(通す)。
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
# 判定基準(`## 要確認` を含む見出しが本文に無ければ無条件 pass。見出しは
# あるが項目が無くても pass): 配下の各トップレベル項目(フェンス外・列頭の
# `- `/`* `/`N. ` で始まる行から次の列頭マーカーまたは節末までを1項目とする)
# ごとに次の3条件の AND:
#   (a) `ブロッキング: <資格情報|ハードウェア|secrets 衛生|GUI|判断>`
#       (pr-description スキル §6 の既存閉集合をそのまま採用)
#   (b) フェンス外にインデントされた `N. ` 形式の手順行が1つ以上
#   (c) `完了確認:` 行(理由必須)
# `G_visual`(docs/claude/pr-gate.md)と同じ二層分担 —「手順が実際に妥当か」
# はここでは判定せず pr-description スキル(LLM の判断)の責務にする。
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

# issue-index.sh / pr-gate.sh / stack-base-guard.sh / pr-title-guard.sh の
# owner_repo() と同一式の複製(source すると attribution-guard.sh の
# dispatch まで巻き込むため複製する)。変更時は揃えること。
owner_repo() {
  local url out
  url="$(git -C "$1" remote -v 2> /dev/null | awk '/github\.com/{print $2; exit}')"
  [[ -n $url ]] || return 1
  out="$(printf '%s' "$url" | sed -nE 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p')"
  [[ -n $out ]] || return 1
  printf '%s\n' "$out"
}

# $1=owner/repo (または repo 単体); tarotene 所有なら 0
is_tarotene_owned() {
  local nwo="$1"
  [[ ${nwo%%/*} == tarotene ]]
}

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
# `## 要確認` 節の切り出し・項目分割・項目判定
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

# pr-description スキル §6 の既存閉集合(新語彙を作らない)。
CONFIRM_REASON_TAGS='資格情報|ハードウェア|secrets[[:space:]]衛生|GUI|判断'

# $1=項目テキスト; フェンス外にインデントされた `N. ` 行が1つ以上あれば0。
item_has_step() {
  local item="$1" infence=0 line found=1
  while IFS= read -r line; do
    if [[ $line =~ ^[[:space:]]*(\`\`\`|~~~) ]]; then
      infence=$((1 - infence))
      continue
    fi
    ((infence)) && continue
    if [[ $line =~ ^[[:space:]]+[0-9]+\.[[:space:]] ]]; then
      found=0
      break
    fi
  done <<< "$item"
  return "$found"
}

# $1=項目テキスト $2=項目番号; 欠落があれば複数行で出力して0、無ければ1。
judge_item() {
  local item="$1" idx="$2"
  local -a missing=()

  grep -Eq "ブロッキング:[[:space:]]*(${CONFIRM_REASON_TAGS})" <<< "$item" \
    || missing+=("ブロッキング理由(閉語彙: 資格情報|ハードウェア|secrets 衛生|GUI|判断)")
  item_has_step "$item" || missing+=("インデントされた番号手順(例: '  1. ...')")
  grep -Eq '^[[:space:]]*完了確認:[[:space:]]*[^[:space:]]' <<< "$item" \
    || missing+=("完了確認: <観測可能な確認方法>")

  ((${#missing[@]} > 0)) || return 1

  printf '項目%s: 次が不足しています\n' "$idx"
  printf '  - %s\n' "${missing[@]}"
  return 0
}

# $1=本文; 欠落があれば案内メッセージを出力して0、無ければ1。
judge_confirm() {
  local body="$1" section
  section="$(find_confirm_section "$body")"
  [[ -n $section ]] || return 1

  collect_items "$section"
  ((${#ITEMS[@]} > 0)) || return 1

  local -a violations=()
  local i=1 it v
  for it in "${ITEMS[@]}"; do
    v="$(judge_item "$it" "$i")" && violations+=("$v")
    i=$((i + 1))
  done

  ((${#violations[@]} > 0)) || return 1

  printf '%s' "PR 本文の \`## 要確認\` に形式不足の項目があります:
$(printf '%s\n' "${violations[@]}")
各項目は次の形で書いてください(pr-description スキル §1):
  - **<ラベル>**(ブロッキング: 資格情報|ハードウェア|secrets 衛生|GUI|判断)
    1. <手順>
    完了確認: <観測可能な確認方法>
    完了後: <再開する人/次に何をするか>

一時的に無効化するには PR_CONFIRM_GUARD_ALLOW=1 を設定してください。"
  return 0
}

# ---------------------------------------------------------------------------
# 1 コマンド範囲のトークンから --body 系 / --repo を抜き出して判定する
# ---------------------------------------------------------------------------

# $1=kind(create|edit) $2=project; 残り=範囲トークン。deny なら理由文を
# stdout に出して 0、通すなら非 0。
judge_confirm_range() {
  local kind="$1" project="$2"
  shift 2

  [[ ${PR_CONFIRM_GUARD_ALLOW:-0} != 1 ]] || return 1

  local -a tok=("$@")
  local n=${#tok[@]} i=0 repo=""
  while ((i < n)); do
    case "${tok[i]}" in
      --repo | -R)
        if ((i + 1 < n)); then
          repo="${tok[i + 1]}"
          i=$((i + 2))
          continue
        fi
        ;;
      --repo=*) repo="${tok[i]#--repo=}" ;;
    esac
    i=$((i + 1))
  done

  local nwo
  if [[ -n $repo ]]; then
    nwo="$repo"
  else
    nwo="$(owner_repo "$project")" || return 1
  fi
  is_tarotene_owned "$nwo" || return 1

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

## 要確認

- **鍵の登録**(ブロッキング: secrets 衛生)
  1. 値を取得する
  完了確認: curl で200が返る'

  echo "見出し無し・項目無し:"

  rc=0
  run_decide "gh pr create --body 'Closes #1'" "$repo_tarotene" > /dev/null || rc=$?
  check "1 要確認見出し無しは pass" "1" "$rc"

  rc=0
  run_decide "gh pr create --body 'Closes #1

## 要確認'" "$repo_tarotene" > /dev/null || rc=$?
  check "2 見出しのみ・項目無しは pass" "1" "$rc"

  echo "適合項目:"

  rc=0
  run_decide "gh pr create --body '$good_body'" "$repo_tarotene" > /dev/null || rc=$?
  check "3 1項目適合は pass" "1" "$rc"

  two_good="$good_body
- **別の鍵**(ブロッキング: 資格情報)
  1. 手順
  完了確認: 確認方法"
  rc=0
  run_decide "gh pr create --body '$two_good'" "$repo_tarotene" > /dev/null || rc=$?
  check "4 2項目とも適合は pass" "1" "$rc"

  fenced_body='Closes #1

## 要確認

- **鍵の登録**(ブロッキング: GUI)
  1. 次のコマンドを実行する:
     ```
     echo 1. not a step
     ```
  完了確認: 目視確認'
  rc=0
  run_decide "gh pr create --body '$fenced_body'" "$repo_tarotene" > /dev/null || rc=$?
  check "5 手順内フェンスがあっても pass" "1" "$rc"

  echo "欠落は deny:"

  no_reason='Closes #1

## 要確認

- **鍵の登録**
  1. 手順
  完了確認: 確認方法'
  out=""
  if out="$(run_decide "gh pr create --body '$no_reason'" "$repo_tarotene")"; then
    check_contains "6 理由欠落は deny" "ブロッキング理由" "$out"
  else
    check "6 deny 期待" "deny" "pass"
  fi

  bad_vocab='Closes #1

## 要確認

- **鍵の登録**(ブロッキング: 面倒)
  1. 手順
  完了確認: 確認方法'
  out=""
  if out="$(run_decide "gh pr create --body '$bad_vocab'" "$repo_tarotene")"; then
    check_contains "7 閉語彙外の理由は deny" "ブロッキング理由" "$out"
  else
    check "7 deny 期待" "deny" "pass"
  fi

  no_step='Closes #1

## 要確認

- **鍵の登録**(ブロッキング: secrets 衛生)
  完了確認: 確認方法'
  out=""
  if out="$(run_decide "gh pr create --body '$no_step'" "$repo_tarotene")"; then
    check_contains "8 番号手順無しは deny" "番号手順" "$out"
  else
    check "8 deny 期待" "deny" "pass"
  fi

  no_confirm='Closes #1

## 要確認

- **鍵の登録**(ブロッキング: secrets 衛生)
  1. 手順'
  out=""
  if out="$(run_decide "gh pr create --body '$no_confirm'" "$repo_tarotene")"; then
    check_contains "9 完了確認無しは deny" "完了確認" "$out"
  else
    check "9 deny 期待" "deny" "pass"
  fi

  two_second_bad="$good_body
- **別の鍵**
  1. 手順"
  out=""
  if out="$(run_decide "gh pr create --body '$two_second_bad'" "$repo_tarotene")"; then
    check_contains "10 2項目中2番目だけ非適合は deny(項目2を指す)" "項目2" "$out"
  else
    check "10 deny 期待" "deny" "pass"
  fi

  echo "escape hatch / スコープ:"

  rc=0
  PR_CONFIRM_GUARD_ALLOW=1 \
    run_decide "gh pr create --body '$no_reason'" "$repo_tarotene" > /dev/null || rc=$?
  check "11 escape hatch は pass" "1" "$rc"

  rc=0
  run_decide "gh pr create --body '$no_reason'" "$repo_other" > /dev/null || rc=$?
  check "12 tarotene 以外は pass" "1" "$rc"

  out=""
  if out="$(run_decide "gh pr create -R tarotene/dotfiles --body '$no_reason'" "$repo_other")"; then
    check_contains "13 -R tarotene 指定は deny" "要確認" "$out"
  else
    check "13 deny 期待" "deny" "pass"
  fi

  echo "--body-file / 非コマンド位置:"

  bf="$tmp/body.md"
  printf '%s' "$no_reason" > "$bf"
  out=""
  if out="$(run_decide "gh pr create --body-file '$bf'" "$repo_tarotene")"; then
    check_contains "14 --body-file 経由も判定される" "要確認" "$out"
  else
    check "14 deny 期待" "deny" "pass"
  fi

  rc=0
  run_decide "echo 'gh pr create --body x'" "$repo_tarotene" > /dev/null || rc=$?
  check "15 非コマンド位置は pass" "1" "$rc"

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
