#!/usr/bin/env bash
# decision-colocation-guard.sh — 決定成果物(ADR / 設計文書 / skill)の追加を
# その執行点と同じ PR に作成時から機械強制する PreToolUse hook(ADR-396)。
#
# 設計と根拠: docs/claude/decision-colocation.md(このリポジトリ内)
#
# 対象コマンド: コマンド位置の `gh pr create`(`-R/--repo` によるクロス
# リポジトリ指定にも対応)。判定は scripts/decision-colocation-check 1 本に
# 一元化する(client guard とサーバ側 required check が同じ判定根拠を共有
# する — pr-title-guard.sh / adr-number-check と同じ型)。
#
# base の解決順: 呼び出しの --base/-B フラグ → stack-base-guard.sh と同じ
# default_branch()(origin/HEAD symref → gh repo view)。解決できた base が
# ローカルに存在しない場合(worktree で origin/<base> のみ持つケースを
# 含む)は origin/<base> にフォールバックする。どちらも無ければ判定不能で
# 通す(ADR-0005 の binary-existence gating と同じ fail-open 方針)。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / CMD_SEPS)は
# attribution-guard.sh を source して再利用する(pr-title-guard.sh /
# stack-base-guard.sh と同じ「1つの判定エンジンを source する」型)。
# `is_target_at` はこのファイルで上書き定義する。attribution-guard.sh
# 自身の decide/main/dispatch は呼ばない。
#
# escape hatch: 環境変数 SKIP_DECISION_COLOCATION_GUARD=1 で一時的に無効化
# する(PR_TITLE_GUARD_ALLOW と同じ性質 — 恒久的な本文タグではない)。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる
#   手動 e2e:   decision-colocation-guard.sh --check '<コマンド文字列>' [<project-dir>]
#   自己検査:   decision-colocation-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail
export LC_ALL=C

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

have() { command -v "$1" > /dev/null 2>&1; }

# 呼び出しのたびに解決する(pr-title-guard.sh の resolve_pr_title_check() と
# 同じ理由 — --selftest の *_BIN 差し替えを起動時1回の解決だと効かせられ
# ない)。source tree からの相対パスを先に試し、無ければ PATH 上の配備済み
# `decision-colocation-check`(~/.local/bin)にフォールバックする。
resolve_checker() {
  local candidate="${DECISION_COLOCATION_CHECK_BIN:-$GUARD_SELF_DIR/../../../scripts/decision-colocation-check}"
  if [[ -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  command -v decision-colocation-check 2> /dev/null
}

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

# $1=project $2=nwo; stack-base-guard.sh の default_branch() と同一式の複製。
default_branch() {
  local project="$1" nwo="$2" ref
  ref="$(git -C "$project" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2> /dev/null)" && {
    printf '%s\n' "${ref#origin/}"
    return 0
  }
  gh repo view -R "$nwo" --json defaultBranchRef -q '.defaultBranchRef.name // empty' 2> /dev/null
}

# $1=project $2=branch名; ローカルに実在すればそのまま、無ければ
# origin/<branch> にフォールバックする ref を返す。どちらも無ければ非0。
resolve_ref() {
  local project="$1" branch="$2"
  if git -C "$project" rev-parse --verify --quiet "$branch" > /dev/null 2>&1; then
    printf '%s\n' "$branch"
    return 0
  fi
  if git -C "$project" rev-parse --verify --quiet "origin/$branch" > /dev/null 2>&1; then
    printf '%s\n' "origin/$branch"
    return 0
  fi
  return 1
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
  [[ ${TOK[i + 2]} == create ]] || return 1
  return 0
}

# ---------------------------------------------------------------------------
# 1 コマンド範囲のトークンから --base / --repo を抜き出す
# ---------------------------------------------------------------------------

# $@=1 `gh pr create` 呼び出しぶんのトークン列。グローバルに結果をセット:
#   F_BASE F_HAS_BASE F_REPO
parse_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0

  F_BASE="" F_HAS_BASE=0 F_REPO=""

  while ((i < n)); do
    case "${tok[i]}" in
      --base | -B)
        F_HAS_BASE=1
        if ((i + 1 < n)); then
          F_BASE="${tok[i + 1]}"
          i=$((i + 2))
          continue
        fi
        ;;
      --base=*)
        F_HAS_BASE=1
        F_BASE="${tok[i]#--base=}"
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

# $1=project; 残り=範囲トークン。deny なら理由を stdout に出して 0、
# 通すなら非 0。
judge_range() {
  local project="$1"
  shift
  parse_tokens "$@"

  [[ ${SKIP_DECISION_COLOCATION_GUARD:-0} != 1 ]] || return 1
  local checker
  checker="$(resolve_checker)"
  [[ -n $checker && -x $checker ]] || return 1

  local nwo="" branch base_ref
  if [[ -n $F_REPO ]]; then
    nwo="$F_REPO"
  else
    nwo="$(owner_repo "$project")" || nwo=""
  fi

  if ((F_HAS_BASE)); then
    branch="$F_BASE"
  else
    [[ -n $nwo ]] || return 1
    branch="$(default_branch "$project" "$nwo")" || branch=""
  fi
  [[ -n $branch ]] || return 1

  base_ref="$(resolve_ref "$project" "$branch")" || return 1

  local out rc=0
  out="$(cd "$project" && "$checker" --base "$base_ref" 2>&1)" || rc=$?
  ((rc == 0)) && return 1 # 適合 -> 通す
  ((rc == 1)) || return 1 # 判定不能(rc=2 等)-> 通す

  printf '%s' "決定成果物(ADR/設計文書/skill)の新規追加、または既存 ADR への Amendment 追加が、その執行点(実際に実装する変更)を同じ PR に伴っていません(config/agents/AGENTS.md「決定成果物は執行点と同じ PR に出す」、docs/adr/396-decision-colocation.md)。詳細:
${out}
一時的に無効化するには SKIP_DECISION_COLOCATION_GUARD=1 を設定してください。"
  return 0
}

# ---------------------------------------------------------------------------
# コマンド文字列全体からの範囲切り出し(attribution-guard.sh の decide() /
# pr-title-guard.sh の decide_pr_title() と同じ設計)
# ---------------------------------------------------------------------------

decide_colocation() {
  local cmd="$1" project="$2"
  split_heredoc "$cmd"

  TOK=()
  local t
  while IFS= read -r -d '' t; do TOK+=("$t"); done < <(tokenize "$CMD_NOHD") || true
  ((${#TOK[@]} > 0)) || return 1

  local n=${#TOK[@]} i at_cmd_pos=1
  local -a starts=()
  for ((i = 0; i < n; i++)); do
    if ((at_cmd_pos)) && is_target_at "$i"; then
      starts+=("$i")
    fi
    if is_sep "${TOK[i]}"; then at_cmd_pos=1; else at_cmd_pos=0; fi
  done
  ((${#starts[@]} > 0)) || return 1

  local m=${#starts[@]} s e reason
  for ((i = 0; i < m; i++)); do
    s=${starts[i]}
    if ((i + 1 < m)); then e=${starts[i + 1]}; else e=$n; fi
    reason="$(judge_range "$project" "${TOK[@]:s:e - s}")" && {
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
      reason="$(decide_colocation "$cmd" "$project")" || exit 0
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
  check_contains() { # $1=名前 $2=部分文字列 $3=対象
    if grep -qF -- "$2" <<< "$3"; then
      echo "ok   $1"
    else
      echo "FAIL $1 (substring [$2] not found in [$3])" >&2
      fails=$((fails + 1))
    fi
  }

  # --- checker スタブ(decision-colocation-check の代わりに固定応答) ---
  mkdir -p "$tmp/bin"
  cat > "$tmp/bin/decision-colocation-check" << 'STUB'
#!/usr/bin/env bash
case "$2" in
  ok-base) exit 0 ;;
  unresolvable) exit 2 ;;
  *) echo "decision-colocation-check: 非適合(スタブ)"; exit 1 ;;
esac
STUB
  chmod +x "$tmp/bin/decision-colocation-check"
  export DECISION_COLOCATION_CHECK_BIN="$tmp/bin/decision-colocation-check"

  # --- 実験環境: main ブランチを持つ git repo(github remote 付き) ---
  repo="$tmp/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q -b main
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m base
  git -C "$repo" remote add origin https://github.com/tarotene/dotfiles.git
  git -C "$repo" update-ref refs/remotes/origin/main "$(git -C "$repo" rev-parse HEAD)"
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  git -C "$repo" switch -c ng-base -q
  git -C "$repo" switch -c ok-base -q
  git -C "$repo" switch main -q

  run_decide() { decide_colocation "$1" "$2"; }

  echo "適合/非適合:"

  # 1: 非適合(base=main、checker が非0を返す) -> deny
  out=""
  if out="$(run_decide "gh pr create --base main --title t --body b" "$repo")"; then
    check_contains "1 非適合は deny" "決定成果物" "$out"
    check_contains "1 checker出力を含む" "非適合(スタブ)" "$out"
  else
    check "1 deny 期待" "deny" "pass"
  fi

  # 2: 適合(base=ok-base) -> pass
  rc=0
  run_decide "gh pr create --base ok-base --title t --body b" "$repo" > /dev/null || rc=$?
  check "2 適合は pass" "1" "$rc"

  echo "base 解決:"

  # 3: --base 省略、default branch(origin/HEAD symref)= main を解決 -> deny
  out=""
  if out="$(run_decide "gh pr create --title t --body b" "$repo")"; then
    check_contains "3 --base省略はdefault branchで判定" "決定成果物" "$out"
  else
    check "3 deny 期待" "deny" "pass"
  fi

  # 4: --base にローカルに実在しないブランチ -> origin/<branch> にも無ければ判定不能で pass
  rc=0
  run_decide "gh pr create --base no-such-branch --title t --body b" "$repo" > /dev/null || rc=$?
  check "4 未解決baseは判定不能でpass" "1" "$rc"

  # 5: checker が判定不能(rc=2)を返す -> pass
  rc=0
  run_decide "gh pr create --base unresolvable --title t --body b" "$repo" > /dev/null || rc=$?
  check "5 checkerが判定不能ならpass" "1" "$rc"

  echo "escape hatch / 縮退:"

  # 6: SKIP_DECISION_COLOCATION_GUARD=1 -> pass
  rc=0
  SKIP_DECISION_COLOCATION_GUARD=1 \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || rc=$?
  check "6 escape hatch は pass" "1" "$rc"

  # 7: checker が実行不能 -> 判定不能で pass
  rc=0
  DECISION_COLOCATION_CHECK_BIN="$tmp/bin/does-not-exist" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || rc=$?
  check "7 checker 不在は pass" "1" "$rc"

  # 8: 非コマンド位置(echo の引数内)は発火しない
  rc=0
  run_decide "echo 'gh pr create --base main'" "$repo" > /dev/null || rc=$?
  check "8 非コマンド位置は pass" "1" "$rc"

  # 9: -R 指定と --base 指定が共存しても解析が壊れない(-R は default
  #    branch 解決にしか使わないパスだが、フラグ解析そのものの回帰を守る)
  rc=0
  run_decide "gh pr create -R tarotene/dotfiles --base ok-base --title t --body b" "$repo" > /dev/null || rc=$?
  check "9 -R と --base の共存は正しく適合判定" "1" "$rc"

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
      if reason="$(decide_colocation "${2-}" "$proj")"; then
        printf 'deny: %s\n' "$reason"
        exit 1
      fi
      echo "pass"
      ;;
    *) main ;;
  esac
fi
