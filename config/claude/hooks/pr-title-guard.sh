#!/usr/bin/env bash
# pr-title-guard.sh — PR タイトルを commit-message 契約として作成時に
# 機械強制する PreToolUse hook(ADR-0031)。
#
# 設計と根拠: docs/claude/pr-title-contract.md(このリポジトリ内)
#
# 対象コマンド: コマンド位置の `gh pr create --title ...` と
# `gh pr edit ... --title ...`(`-R/--repo` によるクロスリポジトリ指定にも
# 対応)。文法検査は scripts/pr-title-check 1 本に一元化する(client guard
# とサーバ側 required check が同じ判定根拠を共有する)。
#
# 発火は owner が tarotene のリポジトリに限定する(ADR-0031 D4)。owner が
# 解決できない場合は fail-open(通す)。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / CMD_SEPS)は
# attribution-guard.sh を source して再利用する(stack-base-guard.sh と
# 同じ「1つの判定エンジンを source する」型)。`is_target_at` はこの
# ファイルで上書き定義する。attribution-guard.sh 自身の decide/main/
# dispatch は呼ばない。
#
# escape hatch: 環境変数 PR_TITLE_GUARD_ALLOW=1 で一時的に無効化する
# (No-Issue: のような本文タグ型ではない — 単発の緊急対応向けの一時解除
# であり、本文に恒久的に残す決定ではないため)。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる
#   手動 e2e:   pr-title-guard.sh --check '<コマンド文字列>' [<project-dir>]
#   自己検査:   pr-title-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail
export LC_ALL=C

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

have() { command -v "$1" > /dev/null 2>&1; }

# 呼び出しのたびに解決する(env 上書きをテストごとに切り替えられるように
# 遅延評価する — 起動時 1 回の解決だと --selftest の PR_TITLE_CHECK_BIN
# 差し替えが効かない)。source tree からの相対パスを先に試し、無ければ
# PATH 上の配備済み `pr-title-check`(~/.local/bin)にフォールバックする。
resolve_pr_title_check() {
  local candidate="${PR_TITLE_CHECK_BIN:-$GUARD_SELF_DIR/../../../scripts/pr-title-check}"
  if [[ -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  command -v pr-title-check 2> /dev/null
}

# issue-index.sh / pr-gate.sh / stack-base-guard.sh の owner_repo() と同一式
# の複製(source すると attribution-guard.sh の dispatch まで巻き込むため
# 複製する)。変更時は揃えること。
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
# 1 コマンド範囲のトークンから --title / --repo を抜き出す
# ---------------------------------------------------------------------------

# $@=1 gh pr create|edit 呼び出しぶんのトークン列。グローバルに結果をセット:
#   F_TITLE F_HAS_TITLE F_REPO
parse_pr_title_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0

  F_TITLE="" F_HAS_TITLE=0 F_REPO=""

  while ((i < n)); do
    case "${tok[i]}" in
      --title | -t)
        F_HAS_TITLE=1
        if ((i + 1 < n)); then
          F_TITLE="${tok[i + 1]}"
          i=$((i + 2))
          continue
        fi
        ;;
      --title=*)
        F_HAS_TITLE=1
        F_TITLE="${tok[i]#--title=}"
        ;;
      --repo | -R)
        if ((i + 1 < n)); then
          F_REPO="${tok[i + 1]}"
          i=$((i + 2))
          continue
        fi
        ;;
      --repo=*)
        F_REPO="${tok[i]#--repo=}"
        ;;
    esac
    i=$((i + 1))
  done
}

# ---------------------------------------------------------------------------
# 判定本体
# ---------------------------------------------------------------------------

# $1=kind(create|edit) $2=project; 残り=範囲トークン。deny なら理由を
# stdout に出して 0、通すなら非 0。
judge_title_range() {
  local kind="$1" project="$2"
  shift 2
  parse_pr_title_tokens "$@"

  [[ ${PR_TITLE_GUARD_ALLOW:-0} != 1 ]] || return 1
  local checker
  checker="$(resolve_pr_title_check)"
  [[ -n $checker && -x $checker ]] || return 1

  local nwo
  if [[ -n $F_REPO ]]; then
    nwo="$F_REPO"
  else
    nwo="$(owner_repo "$project")" || return 1
  fi
  is_tarotene_owned "$nwo" || return 1

  # タイトルが分からない(--title 未指定 = --web でエディタ入力等)場合は
  # 判定不能で通す — ローカルで内容を検査できない。
  ((F_HAS_TITLE)) || return 1
  [[ -n $F_TITLE ]] || return 1

  "$checker" "$F_TITLE" > /dev/null 2>&1 && return 1

  printf '%s' "PR タイトル '${F_TITLE}' は commit-message 契約(ADR-0031)に非適合です。'type(scope)?!?: subject' 形式(type は feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)にしてください。一時的に無効化するには PR_TITLE_GUARD_ALLOW=1 を設定してください。"
  return 0
}

# ---------------------------------------------------------------------------
# コマンド文字列全体からの範囲切り出し(attribution-guard.sh の decide() /
# stack-base-guard.sh の decide_stack() と同じ設計)
# ---------------------------------------------------------------------------

decide_pr_title() {
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
    reason="$(judge_title_range "${kinds[i]}" "$project" "${TOK[@]:s:e - s}")" && {
      printf '%s' "$reason"
      return 0
    }
  done
  return 1
}

# ---------------------------------------------------------------------------
# hook 入出力(attribution-guard.sh と同じ契約)
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
  have jq || exit 0

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
      reason="$(decide_pr_title "$cmd" "$project")" || exit 0
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

  # --- checker スタブ(pr-title-check の代わりに固定応答を返す) ---
  mkdir -p "$tmp/bin"
  cat > "$tmp/bin/pr-title-check" << 'STUB'
#!/usr/bin/env bash
case "$1" in
  "feat: 適合するタイトル") exit 0 ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$tmp/bin/pr-title-check"
  export PR_TITLE_CHECK_BIN="$tmp/bin/pr-title-check"

  # --- 実験環境: tarotene と他 owner のリモートをそれぞれ持つ git repo ---
  repo_tarotene="$tmp/repo-tarotene"
  mkdir -p "$repo_tarotene"
  git -C "$repo_tarotene" init -q
  git -C "$repo_tarotene" remote add origin https://github.com/tarotene/dotfiles.git

  repo_other="$tmp/repo-other"
  mkdir -p "$repo_other"
  git -C "$repo_other" init -q
  git -C "$repo_other" remote add origin https://github.com/example/example.git

  run_decide() { decide_pr_title "$1" "$2"; }

  echo "tarotene リポジトリ:"

  # 1: 非適合タイトルの create -> deny
  out=""
  if out="$(run_decide "gh pr create --title 'PR タイトルを直す' --body b" "$repo_tarotene")"; then
    check_contains "1 非適合は deny" "commit-message 契約" "$out"
  else
    check "1 deny 期待" "deny" "pass"
  fi

  # 2: 適合タイトルの create -> pass
  rc=0
  run_decide "gh pr create --title 'feat: 適合するタイトル' --body b" "$repo_tarotene" > /dev/null || rc=$?
  check "2 適合は pass" "1" "$rc"

  # 3: --title 省略(--web 等) -> 判定不能で pass
  rc=0
  run_decide "gh pr create --web" "$repo_tarotene" > /dev/null || rc=$?
  check "3 --title 省略は pass" "1" "$rc"

  # 4: edit --title 非適合 -> deny
  out=""
  if out="$(run_decide "gh pr edit 1 --title 'PR タイトルを直す'" "$repo_tarotene")"; then
    check_contains "4 edit 非適合は deny" "commit-message 契約" "$out"
  else
    check "4 deny 期待" "deny" "pass"
  fi

  # 5: --title を含まない edit -> 対象外で pass
  rc=0
  run_decide "gh pr edit 1 --add-label bug" "$repo_tarotene" > /dev/null || rc=$?
  check "5 --title 無し edit は pass" "1" "$rc"

  echo "owner スコープ:"

  # 6: tarotene 以外の owner -> 非適合タイトルでも pass(スコープ外)
  rc=0
  run_decide "gh pr create --title 'PR タイトルを直す' --body b" "$repo_other" > /dev/null || rc=$?
  check "6 tarotene 以外は pass" "1" "$rc"

  # 7: -R で明示的に tarotene 指定 -> deny(project 自体は他 owner でも -R が勝つ)
  out=""
  if out="$(run_decide "gh pr create -R tarotene/dotfiles --title 'PR タイトルを直す'" "$repo_other")"; then
    check_contains "7 -R tarotene 指定は deny" "commit-message 契約" "$out"
  else
    check "7 deny 期待" "deny" "pass"
  fi

  echo "escape hatch / 縮退:"

  # 8: PR_TITLE_GUARD_ALLOW=1 -> pass
  rc=0
  PR_TITLE_GUARD_ALLOW=1 \
    run_decide "gh pr create --title 'PR タイトルを直す' --body b" "$repo_tarotene" > /dev/null || rc=$?
  check "8 escape hatch は pass" "1" "$rc"

  # 9: checker が実行不能 -> 判定不能で pass
  rc=0
  PR_TITLE_CHECK_BIN="$tmp/bin/does-not-exist" \
    run_decide "gh pr create --title 'PR タイトルを直す' --body b" "$repo_tarotene" > /dev/null || rc=$?
  check "9 checker 不在は pass" "1" "$rc"

  # 10: 非コマンド位置(echo の引数内)は発火しない
  rc=0
  run_decide "echo 'gh pr create --title x'" "$repo_tarotene" > /dev/null || rc=$?
  check "10 非コマンド位置は pass" "1" "$rc"

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
      if reason="$(decide_pr_title "${2-}" "$proj")"; then
        printf 'deny: %s\n' "$reason"
        exit 1
      fi
      echo "pass"
      ;;
    *) main ;;
  esac
fi
