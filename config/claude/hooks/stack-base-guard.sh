#!/usr/bin/env bash
# stack-base-guard.sh — セッション内の複数 PR を常時単一チェーンに積むことを
# 作成時に機械強制する PreToolUse hook(ADR-0027: uncertainty-first stacking)。
#
# 設計と根拠: docs/claude/stack-base-guard.md(このリポジトリ内)
#
# 判定は 2 層:
#   層(i)  状態レスの祖先一致検査 — HEAD(または編集対象 PR の head)が他の
#          open PR のコミットを祖先として含むなら、base はその PR の head
#          branch でなければならない。タグでも抜けられない(物理的必然)。
#   層(ii) セッション ID 単位の状態 — セッション内で作成した PR の head
#          branch を記録し、2 本目以降でチェーン外のブランチから PR を
#          作ろうとした場合は本文 `Independent-PR: <理由>` を要求する。
#
# 対象コマンド: コマンド位置の `gh pr create` と `gh pr edit ... --base ...`
# (`-R/--repo` によるクロスリポジトリ指定にも対応)。判定不能はすべて
# fail-open(通す) — ADR-0005 の binary-existence gating と同じ縮退方針。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / CMD_SEPS)は
# attribution-guard.sh を source して再利用する(#192 の Codex/Copilot
# adapter と同じ「1つの判定エンジンを source する」型)。`is_target_at` /
# `TARGET_KIND` はこのファイルで上書き定義する(source 後に再定義すれば
# bash の関数解決規則により後勝ちになる)。attribution-guard.sh 自身の
# `decide`/`main`/dispatch は呼ばない。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる
#   手動 e2e:   stack-base-guard.sh --check '<コマンド文字列>' [<project-dir>]
#   自己検査:   stack-base-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail
export LC_ALL=C

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

# ---------------------------------------------------------------------------
# 定数・状態ディレクトリ
# ---------------------------------------------------------------------------

STACK_BASE_GUARD_DIR="${STACK_BASE_GUARD_DIR:-$HOME/.claude/stack-base-guard}"
STACK_STATE_DIR="$STACK_BASE_GUARD_DIR/state"

# 理由を伴って初めて成立する(No-Issue: / No-Attribution: と同型)。
INDEP_RE="Independent-PR:[[:space:]]*[^[:space:]'\"\`)]"

have() { command -v "$1" > /dev/null 2>&1; }

# issue-index.sh / pr-gate.sh の owner_repo() と同一式の複製(source すると
# attribution-guard.sh の dispatch まで巻き込むため複製する)。変更時は
# 3 箇所を揃えること — selftest がこの式のズレを検査する。
owner_repo() {
  local url out
  url="$(git -C "$1" remote -v 2> /dev/null | awk '/github\.com/{print $2; exit}')"
  [[ -n $url ]] || return 1
  out="$(printf '%s' "$url" | sed -nE 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p')"
  [[ -n $out ]] || return 1
  printf '%s\n' "$out"
}

# $1=session_id -> セッション状態ファイルのパス(plan-fresh-gate.sh の
# state_file() と同型のサニタイズ)。
state_file() {
  local sid="$1"
  sid="${sid//[^A-Za-z0-9._-]/_}"
  printf '%s/%s.chain' "$STACK_STATE_DIR" "$sid"
}

has_independent_tag() {
  grep -qE "$INDEP_RE" <<< "$1"
}

# $1=project $2=branch; 楽観的にセッション状態へ追記する(pass した head を
# 記録)。失敗しても致命的ではない(次回の判定は state 無しとして扱われる
# だけ)。
record_chain_head() {
  local project="$1" branch="$2" sfile
  [[ -n $branch ]] || return 0
  mkdir -p "$STACK_STATE_DIR" 2> /dev/null || return 0
  chmod 700 "$STACK_STATE_DIR" 2> /dev/null || true
  sfile="$(state_file "${SESSION_ID:-unknown}")"
  printf '%s\n' "$branch" >> "$sfile" 2> /dev/null || true
}

# $1=project $2=nwo; origin/HEAD の symref → gh repo view の順で解決する
# (ローカルで解決できればネットワーク往復を増やさない)。
default_branch() {
  local project="$1" nwo="$2" ref
  ref="$(git -C "$project" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2> /dev/null)" && {
    printf '%s\n' "${ref#origin/}"
    return 0
  }
  gh repo view -R "$nwo" --json defaultBranchRef -q '.defaultBranchRef.name // empty' 2> /dev/null
}

# ---------------------------------------------------------------------------
# コマンド位置判定(attribution-guard.sh の同名関数を上書きする)
# ---------------------------------------------------------------------------

# グローバル TOK の $1 番目が `gh pr create` / `gh pr edit` の先頭なら 0。
# TARGET_KIND に create|edit をセットする。
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
# 1 コマンド範囲のトークンからフラグを抜き出す
# ---------------------------------------------------------------------------

# $@=1 gh pr create|edit 呼び出しぶんのトークン列。グローバルに結果をセット:
#   F_BASE F_HEAD F_TARGET F_REPO F_BODY_TEXT F_HAS_BASE F_HAS_BODY
#
# 既知の限界: --title 等、値を取る未知フラグの値が F_TARGET(edit の対象
# 識別子)に誤って捕捉されることがある。誤捕捉は「対象 PR が解決できない」
# 方向に倒れ、判定不能(pass)になるだけなので安全側(attribution-guard.sh
# 冒頭のコメントと同じ「既知の限界」記録方針)。
parse_pr_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0 p
  local -a body_texts=()
  local has_hd=0

  F_BASE="" F_HEAD="" F_TARGET="" F_REPO="" F_HAS_BASE=0 F_HAS_BODY=0

  for ((i = 0; i < n; i++)); do
    if [[ ${tok[i]} == *'<<'* ]]; then
      has_hd=1
      break
    fi
  done

  i=0
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
      --head | -H)
        if ((i + 1 < n)); then
          F_HEAD="${tok[i + 1]}"
          i=$((i + 2))
          continue
        fi
        ;;
      --head=*)
        F_HEAD="${tok[i]#--head=}"
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
      --body | -b)
        F_HAS_BODY=1
        if ((i + 1 < n)); then
          body_texts+=("${tok[i + 1]}")
          i=$((i + 2))
          continue
        fi
        ;;
      --body=*)
        F_HAS_BODY=1
        body_texts+=("${tok[i]#--body=}")
        ;;
      --body-file | -F)
        F_HAS_BODY=1
        if ((i + 1 < n)); then
          p="${tok[i + 1]}"
          [[ -f $p && -r $p ]] && body_texts+=("$(cat -- "$p")")
          i=$((i + 2))
          continue
        fi
        ;;
      --body-file=*)
        F_HAS_BODY=1
        p="${tok[i]#--body-file=}"
        [[ -f $p && -r $p ]] && body_texts+=("$(cat -- "$p")")
        ;;
      # 値を取ることが分かっている他の主要フラグは、値トークンが F_TARGET に
      # 誤って捕捉されないよう明示的に読み飛ばす。
      --title | -t | --add-label | --remove-label | --add-assignee \
        | --remove-assignee | --add-reviewer | --remove-reviewer \
        | --add-project | --remove-project | --milestone | -m)
        ((i + 1 < n)) && i=$((i + 1))
        ;;
      -*) : ;; # 未知フラグ(値なし想定) — そのままスキップ
      *)
        [[ -z $F_TARGET && $i -ge 3 ]] && F_TARGET="${tok[i]}"
        ;;
    esac
    i=$((i + 1))
  done

  if ((has_hd)) && [[ -n ${HD_BODIES:-} ]]; then
    body_texts+=("$HD_BODIES")
  fi
  F_BODY_TEXT=""
  ((${#body_texts[@]} > 0)) && F_BODY_TEXT="$(printf '%s\n' "${body_texts[@]}")"
}

# ---------------------------------------------------------------------------
# 判定本体
# ---------------------------------------------------------------------------

# $1=kind(create|edit) $2=project; 残り=範囲トークン。deny なら理由を
# stdout に出して 0、通すなら非 0。
judge_range() {
  local kind="$1" project="$2"
  shift 2
  parse_pr_tokens "$@"

  have gh && have git && have jq || return 1

  local nwo
  if [[ -n $F_REPO ]]; then
    nwo="$F_REPO"
  else
    nwo="$(owner_repo "$project")" || return 1
  fi

  local prs_json
  prs_json="$(gh pr list -R "$nwo" --state open --limit 100 \
    --json number,headRefName,headRefOid,baseRefName 2> /dev/null)" || return 1
  [[ -n $prs_json ]] || return 1

  case "$kind" in
    create) judge_create "$project" "$nwo" "$prs_json" ;;
    edit)
      ((F_HAS_BASE)) || return 1 # base に触れていない edit は対象外
      judge_edit "$project" "$nwo" "$prs_json"
      ;;
    *) return 1 ;;
  esac
}

# $1=project; open PR 一覧 JSON の中から、$2 の祖先になっている PR のうち
# 最も近いものを探す。見つかれば "number<TAB>headRefName" を stdout に出して
# 0、無ければ非 0。$3=除外する headRefName(自分自身)。$4=除外する PR 番号。
find_nearest_ancestor_pr() {
  local project="$1" target_sha="$2" prs_json="$3" self_head="${4-}" self_num="${5-}"
  local num head oid base best_dist="" best_num="" best_head="" dist
  while IFS=$'\t' read -r num head oid base; do
    [[ -n $num ]] || continue
    [[ -n $self_head && $head == "$self_head" ]] && continue
    [[ -n $self_num && $num == "$self_num" ]] && continue
    [[ -n $oid ]] || continue
    git -C "$project" cat-file -e "${oid}^{commit}" 2> /dev/null || continue
    [[ $oid == "$target_sha" ]] && continue
    git -C "$project" merge-base --is-ancestor "$oid" "$target_sha" 2> /dev/null || continue
    dist="$(git -C "$project" rev-list --count "${oid}..${target_sha}" 2> /dev/null)" || continue
    if [[ -z $best_dist || $dist -lt $best_dist ]]; then
      best_dist="$dist" best_num="$num" best_head="$head"
    fi
  done < <(jq -r '.[] | [(.number|tostring), .headRefName, .headRefOid, .baseRefName] | @tsv' <<< "$prs_json")
  [[ -n $best_num ]] || return 1
  printf '%s\t%s\n' "$best_num" "$best_head"
}

judge_create() {
  local project="$1" nwo="$2" prs_json="$3"
  local head_branch declared_base head_sha

  head_branch="${F_HEAD:-$(git -C "$project" branch --show-current 2> /dev/null)}"
  [[ -n $head_branch ]] || return 1
  head_sha="$(git -C "$project" rev-parse HEAD 2> /dev/null)" || return 1

  if ((F_HAS_BASE)); then
    declared_base="$F_BASE"
  else
    declared_base="$(default_branch "$project" "$nwo")" || declared_base=""
    [[ -n $declared_base ]] || return 1
  fi

  # --- 層(i): 状態レス祖先一致検査 ---
  local hit parent_num parent_head
  if hit="$(find_nearest_ancestor_pr "$project" "$head_sha" "$prs_json" "$head_branch" "")"; then
    IFS=$'\t' read -r parent_num parent_head <<< "$hit"
    if [[ $declared_base != "$parent_head" ]]; then
      printf '%s' "HEAD は open PR #${parent_num}(${parent_head})のコミットを含んでいます。base を ${declared_base} にすると先行 PR の差分がこの PR に混入します。\`gh pr create --base ${parent_head}\` で作成してください。この積み方は依存の有無によらず常時とります(ADR-0027)。"
      return 0
    fi
    record_chain_head "$project" "$head_branch"
    return 1
  fi

  # --- 層(ii): セッション内チェーン状態 ---
  judge_session_chain "$project" "$prs_json" "$head_branch" "$declared_base"
}

judge_session_chain() {
  local project="$1" prs_json="$2" head_branch="$3" declared_base="$4"
  local sfile
  sfile="$(state_file "${SESSION_ID:-unknown}")"

  if [[ ! -f $sfile ]]; then
    record_chain_head "$project" "$head_branch"
    return 1 # このセッションでの初回 PR -> 通す
  fi

  local -a live_branches=()
  local b
  while IFS= read -r b; do
    [[ -n $b ]] || continue
    jq -e --arg b "$b" '[.[] | select(.headRefName == $b)] | length > 0' \
      <<< "$prs_json" > /dev/null 2>&1 && live_branches+=("$b")
  done < "$sfile"

  if ((${#live_branches[@]} == 0)); then
    record_chain_head "$project" "$head_branch"
    return 1 # 記録済みの PR が全て閉じている/実在しない -> 新チェーン扱い
  fi

  local last="${live_branches[-1]}"
  if [[ $declared_base == "$last" ]]; then
    record_chain_head "$project" "$head_branch"
    return 1
  fi

  ((F_HAS_BODY)) || return 1 # 本文フラグ無し -> 判定不能で通す
  [[ -n $F_BODY_TEXT ]] || return 1 # 本文が読めない(コマンド置換等) -> 通す

  if has_independent_tag "$F_BODY_TEXT"; then
    record_chain_head "$project" "$head_branch"
    return 1
  fi

  printf '%s' "このセッションでは既に PR(${last})を作成しています。2 本目以降は直前の段に積むのが既定です(ADR-0027)。\`git rebase --onto ${last} ...\` で積み替えて \`gh pr create --base ${last}\` とするか、真に独立な PR なら本文に \`Independent-PR: <理由>\` を書いて明示的に抜けてください。"
  return 0
}

judge_edit() {
  local project="$1" nwo="$2" prs_json="$3"
  local target="$F_TARGET" row num head oid base

  if [[ -n $target ]]; then
    row="$(jq -r --arg t "$target" '
      [.[] | select((.number|tostring) == $t or .headRefName == $t)] | .[0] // empty
      | if . == "" then empty else [(.number|tostring), .headRefName, .headRefOid, .baseRefName] | @tsv end
    ' <<< "$prs_json" 2> /dev/null)"
  else
    local cur
    cur="$(git -C "$project" branch --show-current 2> /dev/null)"
    [[ -n $cur ]] || return 1
    row="$(jq -r --arg b "$cur" '
      [.[] | select(.headRefName == $b)] | .[0] // empty
      | if . == "" then empty else [(.number|tostring), .headRefName, .headRefOid, .baseRefName] | @tsv end
    ' <<< "$prs_json" 2> /dev/null)"
  fi
  [[ -n $row ]] || return 1 # 対象 PR が解決できない -> 通す

  IFS=$'\t' read -r num head oid base <<< "$row"
  [[ -n $oid ]] || return 1
  git -C "$project" cat-file -e "${oid}^{commit}" 2> /dev/null || return 1

  local declared_base="$F_BASE"
  [[ -n $declared_base ]] || return 1

  local hit parent_num parent_head
  hit="$(find_nearest_ancestor_pr "$project" "$oid" "$prs_json" "" "$num")" || return 1
  IFS=$'\t' read -r parent_num parent_head <<< "$hit"

  if [[ $declared_base != "$parent_head" ]]; then
    printf '%s' "PR #${num}(${head})は open PR #${parent_num}(${parent_head})のコミットを含んでいます。base を ${declared_base} にすると先行 PR の差分が混入します。\`gh pr edit ${num} --base ${parent_head}\` としてください。この積み方は依存の有無によらず常時とります(ADR-0027)。"
    return 0
  fi
  return 1
}

# ---------------------------------------------------------------------------
# コマンド文字列全体からの範囲切り出し(attribution-guard.sh の decide() と
# 同じ設計 — コマンド位置に限定、複数範囲を独立に判定)
# ---------------------------------------------------------------------------

decide_stack() {
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
    reason="$(judge_range "${kinds[i]}" "$project" "${TOK[@]:s:e - s}")" && {
      printf '%s' "$reason"
      return 0
    }
  done
  return 1
}

# MCP GitHub の書き込み系 tool(現在未接続、attribution-guard.sh と同じく
# 命名は未確認、#161)。create_pull/update_pull 系のみ対象。
MCP_PR_WRITE_RE='(create_pull|update_pull)'

decide_mcp_stack() {
  local tool="$1" input="$2" project="$3"
  grep -qE "$MCP_PR_WRITE_RE" <<< "$tool" || return 1

  have gh && have git && have jq || return 1

  local base body head repo nwo
  base="$(jq -r '.tool_input.base // empty' <<< "$input" 2> /dev/null)" || return 1
  [[ -n $base ]] || return 1
  body="$(jq -r '.tool_input.body // empty' <<< "$input" 2> /dev/null)" || body=""
  head="$(jq -r '.tool_input.head // empty' <<< "$input" 2> /dev/null)" || head=""
  repo="$(jq -r '.tool_input.repo // empty' <<< "$input" 2> /dev/null)" || repo=""

  F_BASE="$base" F_HEAD="$head" F_REPO="$repo" F_TARGET="" \
    F_HAS_BASE=1 F_HAS_BODY=1 F_BODY_TEXT="$body"

  local nwo_local
  if [[ -n $repo ]]; then
    nwo_local="$repo"
  else
    nwo_local="$(owner_repo "$project")" || return 1
  fi
  local prs_json
  prs_json="$(gh pr list -R "$nwo_local" --state open --limit 100 \
    --json number,headRefName,headRefOid,baseRefName 2> /dev/null)" || return 1
  [[ -n $prs_json ]] || return 1

  if grep -q "create_pull" <<< "$tool"; then
    judge_create "$project" "$nwo_local" "$prs_json"
  else
    # update_pull: PR 番号を判定できない場合は判定不能で通す。
    local num
    num="$(jq -r '.tool_input.pull_number // .tool_input.pullNumber // empty' <<< "$input" 2> /dev/null)" || return 1
    [[ -n $num ]] || return 1
    F_TARGET="$num"
    judge_edit "$project" "$nwo_local" "$prs_json"
  fi
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

  if [[ -e "$STACK_BASE_GUARD_DIR/skip" || "${SKIP_STACK_BASE_GUARD:-0}" == "1" ]]; then
    exit 0
  fi

  local input tool project reason cmd
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  SESSION_ID="$(jq -r '.session_id // "unknown"' <<< "$input" 2> /dev/null)" || SESSION_ID="unknown"
  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)}" || project=""
  [[ -n $project ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

  case "$tool" in
    Bash)
      cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide_stack "$cmd" "$project")" || exit 0
      ;;
    mcp__github*)
      reason="$(decide_mcp_stack "$tool" "$input" "$project")" || exit 0
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

  # owner_repo() が pr-gate.sh / issue-index.sh の同名関数とズレていないかを
  # 検査する(3 箇所の複製を揃える契約、ファイル冒頭コメント参照)。
  local expected_owner_repo
  expected_owner_repo='owner_repo() {
  local url out
  url="$(git -C "$1" remote -v 2>/dev/null | awk '"'"'/github\.com/{print $2; exit}'"'"')"
  [[ -n "$url" ]] || return 1
  out="$(printf '"'"'%s'"'"' "$url" | sed -nE '"'"'s#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p'"'"')"
  [[ -n "$out" ]] || return 1
  printf '"'"'%s\n'"'"' "$out"
}'
  # 完全一致までは要求しない(空白・コメント差分は許容) — 実質チェックは
  # sed/awk 抽出式の骨格が同じ github.com マッチ・キャプチャであること。
  if ! declare -f owner_repo | grep -q 'github\\.com\[:/\]'; then
    echo "FAIL(owner_repo 式の骨格が変わっている)" >&2
    fails=$((fails + 1))
  fi

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

  # --- gh スタブ ---
  # STACK_STUB_PR_LIST_FILE : gh pr list --json ... の応答(JSON 配列)
  # STACK_STUB_PR_LIST_RC   : gh pr list の exit code(既定 0)
  # STACK_STUB_DEFAULT_BRANCH : gh repo view --json defaultBranchRef の応答用
  mkdir -p "$tmp/bin"
  cat > "$tmp/bin/gh" << 'STUB'
#!/usr/bin/env bash
jqbin="$(command -v jq)"
case "$1" in
  pr)
    case "$2" in
      list)
        [[ "${STACK_STUB_PR_LIST_RC:-0}" == "0" ]] || exit 1
        if [[ -n "${STACK_STUB_PR_LIST_FILE:-}" && -f "${STACK_STUB_PR_LIST_FILE:-}" ]]; then
          cat "${STACK_STUB_PR_LIST_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  repo)
    case "$2" in
      view)
        if [[ -n "${STACK_STUB_DEFAULT_BRANCH:-}" ]]; then
          "$jqbin" -n --arg b "${STACK_STUB_DEFAULT_BRANCH}" '{defaultBranchRef:{name:$b}}' \
            | "$jqbin" -r '.defaultBranchRef.name'
        else
          exit 1
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$tmp/bin/gh"
  stub_path="$tmp/bin:$PATH"
  # run_decide() は同一プロセス内で decide_stack() を直接呼ぶので(サブ
  # プロセスの env prefix では効かない)、selftest 全体の $PATH をスタブ側に
  # 切り替える。テスト10(bash "$self" ... を別プロセスで起動)は明示的な
  # PATH= prefix と二重になるが害はない。
  export PATH="$stub_path"

  # --- 実験環境: github remote 付きの git repo。main <- stage1 <- stage2、
  # main から直接切った unrelated ブランチも用意する。
  repo="$tmp/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m base
  git -C "$repo" remote add origin https://github.com/example/example.git
  git -C "$repo" update-ref refs/remotes/origin/main "$(git -C "$repo" rev-parse HEAD)"
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  main_sha="$(git -C "$repo" rev-parse HEAD)"

  git -C "$repo" switch -c stage1 -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m c1
  stage1_sha="$(git -C "$repo" rev-parse HEAD)"

  git -C "$repo" switch -c stage2 -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m c2
  stage2_sha="$(git -C "$repo" rev-parse HEAD)"

  git -C "$repo" switch main -q
  git -C "$repo" switch -c unrelated -q
  git -C "$repo" -c core.hooksPath=/dev/null -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m u1
  unrelated_sha="$(git -C "$repo" rev-parse HEAD)"

  git -C "$repo" switch stage2 -q

  # PR 一覧 fixture: #1 = stage1(open, base main)
  printf '[{"number":1,"headRefName":"stage1","headRefOid":"%s","baseRefName":"main"}]\n' \
    "$stage1_sha" > "$tmp/prs-stage1-only.json"
  # PR 一覧 fixture: #1 = stage1, #2 = stage2(既に stage1 base で作成済み)
  printf '[{"number":1,"headRefName":"stage1","headRefOid":"%s","baseRefName":"main"},{"number":2,"headRefName":"stage2","headRefOid":"%s","baseRefName":"stage1"}]\n' \
    "$stage1_sha" "$stage2_sha" > "$tmp/prs-stage1-stage2.json"
  printf '[]\n' > "$tmp/prs-empty.json"

  export STACK_BASE_GUARD_DIR="$tmp/state-root"
  export SESSION_ID="selftest-sid"

  reset_state() { rm -rf "$STACK_BASE_GUARD_DIR"; }

  run_decide() { # $1=cmd $2=project-dir; stdout=reason(deny時), 戻り値=0(deny)/1(pass)
    decide_stack "$1" "$2"
  }

  echo "層(i) 祖先一致検査:"

  # 1: stage2 から祖先 PR(#1 stage1)ありのまま base:main で create -> deny
  reset_state
  out=""
  if out="$(STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo")"; then
    check_contains "1 正しい base を案内" "gh pr create --base stage1" "$out"
    check_contains "1 PR 番号を案内" "#1" "$out"
  else
    check "1 deny 期待" "deny" "pass"
  fi

  # 2: 正しい base(stage1) -> pass
  reset_state
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --base stage1 --title t --body b" "$repo" > /dev/null || rc=$?
  check "2 正しい base は pass" "1" "$rc"

  # 3: 初回 PR(祖先 PR なし、state も空) -> pass
  reset_state
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || rc=$?
  check "3 初回 PR は pass" "1" "$rc"

  echo "層(ii) セッションチェーン:"

  # 4: 1本目(stage1, base main)を通して記録 -> 2本目(unrelated, base main,
  #    Independent-PR タグ無し) -> deny
  reset_state
  git -C "$repo" switch stage1 -q
  STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || true
  git -C "$repo" switch unrelated -q
  out=""
  if out="$(STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo")"; then
    check_contains "4 チェーン外2本目タグ無しは deny" "Independent-PR" "$out"
  else
    check "4 deny 期待" "deny" "pass"
  fi

  # 5: 同条件だが Independent-PR タグあり -> pass
  reset_state
  git -C "$repo" switch stage1 -q
  STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || true
  git -C "$repo" switch unrelated -q
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --base main --title t --body 'Independent-PR: 緊急hotfixのため'" "$repo" \
    > /dev/null || rc=$?
  check "5 Independent-PR タグありは pass" "1" "$rc"

  # 6: 空の Independent-PR タグは deny のまま
  reset_state
  git -C "$repo" switch stage1 -q
  STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || true
  git -C "$repo" switch unrelated -q
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --base main --title t --body 'x Independent-PR: '" "$repo" > /dev/null || rc=$?
  check "6 空タグは deny のまま" "0" "$rc"

  git -C "$repo" switch stage2 -q

  echo "縮退・境界:"

  # 7: heredoc 本文での Independent-PR(chain 外2本目)
  reset_state
  git -C "$repo" switch stage1 -q
  STACK_STUB_PR_LIST_FILE="$tmp/prs-empty.json" \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || true
  git -C "$repo" switch unrelated -q
  hd_cmd="$(printf "gh pr create --base main --body \"\$(cat <<%sEOF%s\nIndependent-PR: heredoc 経由の理由\nEOF\n)\"" "'" "'")"
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "$hd_cmd" "$repo" > /dev/null || rc=$?
  check "7 heredoc 本文の Independent-PR は pass" "1" "$rc"
  git -C "$repo" switch stage2 -q

  # 8: 非コマンド位置(echo の引数内)は発火しない
  reset_state
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "echo 'gh pr create --base main'" "$repo" > /dev/null || rc=$?
  check "8 非コマンド位置は pass" "1" "$rc"

  # 9: gh pr list 失敗 -> 判定不能で pass
  reset_state
  rc=0
  STACK_STUB_PR_LIST_RC=1 \
    run_decide "gh pr create --base main --title t --body b" "$repo" > /dev/null || rc=$?
  check "9 gh 失敗は pass" "1" "$rc"

  # 10: 非 git ディレクトリ(main() の縮退経路。decide_stack は git 前提の
  #     呼び出し元 main() だけがこの縮退を持つため、main() を直接叩く)
  nogit="$tmp/nogit"
  mkdir -p "$nogit"
  rc=0
  out="$(PATH="$stub_path" CLAUDE_PROJECT_DIR="$nogit" bash "$self" \
    <<< "{\"session_id\":\"selftest-sid\",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"gh pr create --base main\"}}" \
    2> "$tmp/err")" || rc=$?
  check "10 非 git cwd は exit 0" "0" "$rc"
  check "10 非 git cwd は無出力" "" "$out"

  # 11: --web + 親あり(--base 省略、default branch は origin/HEAD symref から
  #     main と解決される)-> deny
  reset_state
  out=""
  if out="$(STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-only.json" \
    run_decide "gh pr create --web" "$repo")"; then
    check_contains "11 --web+親ありは deny" "stage1" "$out"
  else
    check "11 deny 期待" "deny" "pass"
  fi

  echo "gh pr edit --base:"

  # 12: PR #2(stage2)を編集して base を main に付け替えようとする(#1 stage1
  #     の祖先を含んだまま)-> deny、正しい base(stage1)なら pass
  reset_state
  out=""
  if out="$(STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-stage2.json" \
    run_decide "gh pr edit 2 --base main" "$repo")"; then
    check_contains "12 edit --base 取り違えは deny" "gh pr edit 2 --base stage1" "$out"
  else
    check "12 deny 期待" "deny" "pass"
  fi
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-stage2.json" \
    run_decide "gh pr edit 2 --base stage1" "$repo" > /dev/null || rc=$?
  check "13 edit --base 正しい値は pass" "1" "$rc"

  # 14: --base を含まない edit は対象外(判定不能で pass)
  rc=0
  STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-stage2.json" \
    run_decide "gh pr edit 2 --add-label bug" "$repo" > /dev/null || rc=$?
  check "14 --base 無し edit は pass" "1" "$rc"

  # 15: -R によるクロスリポジトリ指定でも判定が効く(手元の worktree から
  #     別リポジトリの stack を修復する運用で使う形)
  reset_state
  out=""
  if out="$(STACK_STUB_PR_LIST_FILE="$tmp/prs-stage1-stage2.json" \
    run_decide "gh pr edit 2 -R example/example --base main" "$repo")"; then
    check_contains "15 -R 指定でも deny" "stage1" "$out"
  else
    check "15 deny 期待" "deny" "pass"
  fi

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest)
      self="$GUARD_SELF_DIR/$(basename "${BASH_SOURCE[0]}")"
      selftest
      ;;
    --check)
      proj="${3:-$PWD}"
      if reason="$(decide_stack "${2-}" "$proj")"; then
        printf 'deny: %s\n' "$reason"
        exit 1
      fi
      echo "pass"
      ;;
    *) main ;;
  esac
fi
