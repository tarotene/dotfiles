#!/usr/bin/env bash
# pr-gate.sh — PR の完了を待つ Stop hook（+ 現在状態を運ぶ SessionStart hook）。
#
# 設計と根拠: docs/pr-gate.md（このリポジトリ内）
#
# 「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」という 2 種の事故を、
# Stop の 1 点だけで hard gate する。base 鮮度・未コミット変更は advisory
# (block するときだけ相乗りで伝える。それ単独では終了を止めない)。
#
#   G_push : ローカル HEAD == PR の headRefOid                → block
#   G_CI   : 期待される check がすべて pass/skipping           → block
#   G_base : origin/<base> に対する ahead/behind               → advisory
#   G_wt   : 未コミット件数                                    → advisory
#
# G_CI の中心不変条件は「揃っていない集合を緑と読まないこと」。次の 3 経路で
# `gh pr checks` は「チェックが無い/揃っていない」を「exit 0」で返す。どの経路でも
# 判定をその exit code に置かない — 期待集合をサーバから取り、判定は jq が行う:
#   1. push 直後で check run がまだ API に現れていない (cli/cli#7401)
#   2. ruleset の対象外(base が main 以外の stacked PR)で required が 0 件
#   3. 6 件のうち一部だけが現れ、その部分集合が pass した時点で
#      `--watch` が早期終了する (cli/cli#9973) — これが一番危ない。
#      「1 件以上現れたら待機開始」では防げないので、`--watch` は待つための
#      道具に格下げし、その exit code を acceptance criterion にしない。
#
# 発火範囲: allowlist ファイル(既定 ~/.claude/pr-gate-repos)に nwo が無ければ
# 完全沈黙。全ホストに無条件配備する前提(会社リポジトリでは既定で沈黙する)。
#
# 縮退:
#   完全沈黙(exit 0, 出力なし) → allowlist 外 / git repo でない / GitHub remote
#     でない / jq・gh・git 不在 / open PR が無い / skip
#   警告 1 行 + fail-open      → gh 未ログイン・API 失敗・fetch 失敗
#   hard block(exit 2)         → G_push 不一致 / G_CI が揃わない・失敗・pending
#
# `stop_hook_active` は見ない。wrapup-stop-gate.sh と同じ即 exit 0 にすると、
# G_push で 1 回 block した直後の再呼び出しが CI 判定に到達しない
# (docs/codex-plan-review.md の「第二次の非収束」と同型)。上限は独自カウンタ:
# state/<sid>.count が ${PR_GATE_MAX_BLOCKS:-3} に達したら 1 回だけ escalate し、
# touch state/<sid>.escalated。以後そのセッションは無条件で素通る(escalated の
# チェックは上限判定より前 — docs/codex-plan-review.md の closer と同じ置き方)。
#
# 使い方:
#   hook として:  settings.json の SessionStart / Stop から
#                 stdin JSON で呼ばれる(argv[1] = "session-start" / "stop")
#   自己検査:     pr-gate.sh --selftest
#
# エスケープハッチ: touch ~/.claude/pr-gate/skip または SKIP_PR_GATE=1
set -euo pipefail

STATE_ROOT="${PR_GATE_DIR:-$HOME/.claude/pr-gate}"
STATE_DIR="$STATE_ROOT/state"
ALLOWLIST="${PR_GATE_ALLOWLIST:-$HOME/.claude/pr-gate-repos}"

CI_TIMEOUT="${PR_GATE_CI_TIMEOUT:-300}"
CHECK_APPEAR_TIMEOUT="${PR_GATE_CHECK_APPEAR_TIMEOUT:-60}"
QUIESCE="${PR_GATE_QUIESCE:-15}"
FETCH_TTL="${PR_GATE_FETCH_TTL:-600}"
MAX_BLOCKS="${PR_GATE_MAX_BLOCKS:-3}"

# 自身の絶対パス。symlink は辿らない — 配備後は ~/.claude/hooks/ 配下が nix store
# への symlink であり、世代を跨いで安定な symlink 側を指示文に出したい
# (wrapup-stop-gate.sh / issue-index.sh と同じ前例)。
self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

ensure_dirs() {
  mkdir -p "$STATE_DIR"
  chmod 700 "$STATE_ROOT" "$STATE_DIR" 2>/dev/null || true
  find "$STATE_ROOT" -maxdepth 2 -type f ! -name skip -perm /077 \
    -exec chmod 600 {} + 2>/dev/null || true
}

# $1 の git repo の origin(相当)remote から "owner/repo" を取り出す。
# issue-index.sh:owner_repo() と同一式の複製(source すると hook 本体まで走るため)。
# 変更時は両方を揃えること — selftest がこの式のズレを検査する。
owner_repo() {
  local url out
  url="$(git -C "$1" remote -v 2>/dev/null | awk '/github\.com/{print $2; exit}')"
  [[ -n "$url" ]] || return 1
  out="$(printf '%s' "$url" | sed -nE 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?/?$#\1/\2#p')"
  [[ -n "$out" ]] || return 1
  printf '%s\n' "$out"
}

allowlisted() {
  local nwo="$1"
  [[ -f "$ALLOWLIST" ]] || return 1
  grep -vE '^[[:space:]]*(#|$)' "$ALLOWLIST" 2>/dev/null | grep -qxF "$nwo"
}

url_encode_slash() { printf '%s' "$1" | sed 's#/#%2F#g'; }

git_common_dir() {
  git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null
}

# $1=project $2=base -> "ahead\tbehind"("?\t?" は origin/<base> が手元に無い/不明)
compute_ahead_behind() {
  local project="$1" base="$2" ab
  if git -C "$project" rev-parse --verify -q "origin/$base" >/dev/null 2>&1; then
    ab="$(git -C "$project" rev-list --left-right --count "HEAD...origin/$base" 2>/dev/null)" || ab=""
    if [[ -n "$ab" ]]; then
      printf '%s\n' "$ab"
      return 0
    fi
  fi
  printf '?\t?\n'
}

uncommitted_count() {
  local n
  n="$(git -C "$1" status --porcelain 2>/dev/null | wc -l | tr -d ' ')" || n="?"
  [[ -n "$n" ]] || n="?"
  printf '%s' "$n"
}

# --- state: block 回数 / escalated フラグ -------------------------------------

count_file() { printf '%s/%s.count' "$STATE_DIR" "$1"; }
escalated_file() { printf '%s/%s.escalated' "$STATE_DIR" "$1"; }

read_count() {
  local f n
  f="$(count_file "$1")"
  n=0
  if [[ -f "$f" ]]; then
    n="$(cat "$f" 2>/dev/null)" || n=0
    [[ "$n" =~ ^[0-9]+$ ]] || n=0
  fi
  printf '%s' "$n"
}

bump_count() {
  local f n
  f="$(count_file "$1")"
  n=$(($(read_count "$1") + 1))
  printf '%s' "$n" >"$f"
  printf '%s' "$n"
}

# --- G_CI: 期待集合の取得と判定 -----------------------------------------------

# サーバの ruleset から $2(base ブランチ)の required_status_checks の
# context 名一覧を取る。取れない/無い場合は "[]" を返す(空 = quiesce モード)。
expected_contexts() {
  local nwo="$1" base="$2" raw out
  raw="$(gh api "repos/${nwo}/rules/branches/$(url_encode_slash "$base")" 2>/dev/null)" || raw=""
  out=""
  if [[ -n "$raw" ]]; then
    out="$(jq -c '[.[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context]' \
      <<<"$raw" 2>/dev/null)" || out=""
  fi
  [[ -n "$out" ]] || out='[]'
  printf '%s' "$out"
}

reported_checks() {
  local pr_num="$1" nwo="$2" out
  out="$(gh pr checks "$pr_num" -R "$nwo" --json name,bucket,link,workflow 2>/dev/null)" || out=""
  [[ -n "$out" ]] || out='[]'
  printf '%s' "$out"
}

# 失敗/取消チェックをジョブ名 + link + gh run view コマンドで整形する。
render_failed_checks() {
  local pr_num="$1" nwo="$2" reported name link run_id job_id
  reported="$(reported_checks "$pr_num" "$nwo")"
  while IFS=$'\t' read -r name link; do
    [[ -n "$name" ]] || continue
    printf '失敗: %s\n  %s\n' "$name" "$link"
    run_id="$(sed -nE 's#.*/runs/([0-9]+)/job/([0-9]+).*#\1#p' <<<"$link")"
    job_id="$(sed -nE 's#.*/runs/([0-9]+)/job/([0-9]+).*#\2#p' <<<"$link")"
    if [[ -n "$run_id" && -n "$job_id" ]]; then
      printf '\nログ:\n  gh run view %s --log-failed --job %s\n' "$run_id" "$job_id"
    fi
  done < <(jq -r '[.[] | select(.bucket=="fail" or .bucket=="cancel")] | .[] | "\(.name)\t\(.link)"' \
    <<<"$reported" 2>/dev/null || true)
}

# G_CI の判定。結果はグローバル変数 G_CI_STATUS
# (PASS/EMPTY/MISSING/FAILED/PENDING) と G_CI_DETAIL に置く。
# 戻り値: PASS なら 0、それ以外は 1。
run_g_ci() {
  local nwo="$1" pr_num="$2" base="$3"
  G_CI_STATUS=""
  G_CI_DETAIL=""

  local expected expected_count mode note=""
  expected="$(expected_contexts "$nwo" "$base")"
  expected_count="$(jq 'length' <<<"$expected" 2>/dev/null)" || expected_count=0

  local required_flag=()
  if [[ "$expected_count" -gt 0 ]]; then
    mode="required"
    required_flag=(--required)
  else
    mode="quiesce"
    note="base ${base} は ruleset の対象外(または取得失敗)。報告された全チェックで判定した"
  fi

  local deadline reported names_json missing missing_count
  deadline=$(($(date +%s) + CHECK_APPEAR_TIMEOUT))

  if [[ "$mode" == "required" ]]; then
    # 出現待ち: 期待集合 E が報告集合 R に完全に含まれるまで待つ(cli#7401 / #9973 対策)。
    while :; do
      reported="$(reported_checks "$pr_num" "$nwo")"
      names_json="$(jq -c '[.[].name]' <<<"$reported" 2>/dev/null)" || names_json='[]'
      missing="$(jq -cn --argjson e "$expected" --argjson r "$names_json" '$e - $r' 2>/dev/null)" || missing="$expected"
      missing_count="$(jq 'length' <<<"$missing" 2>/dev/null)" || missing_count=1
      [[ "$missing_count" -eq 0 ]] && break
      if [[ "$(date +%s)" -ge "$deadline" ]]; then
        G_CI_STATUS="MISSING"
        G_CI_DETAIL="$(jq -r 'join(", ")' <<<"$missing" 2>/dev/null)" || G_CI_DETAIL="(不明)"
        return 1
      fi
      sleep 5
    done
  else
    # quiescence: E が定義できない(stacked PR 等)ので、報告件数が
    # QUIESCE 秒増えなくなるまで待って「揃った」とみなす。
    local prev_count=-1 stable_since=-1 cur_count now
    while :; do
      reported="$(reported_checks "$pr_num" "$nwo")"
      cur_count="$(jq 'length' <<<"$reported" 2>/dev/null)" || cur_count=0
      now="$(date +%s)"
      if [[ "$cur_count" -gt 0 && "$cur_count" -eq "$prev_count" ]]; then
        [[ "$stable_since" -lt 0 ]] && stable_since="$now"
        if ((now - stable_since >= QUIESCE)); then
          break
        fi
      else
        stable_since=-1
      fi
      prev_count="$cur_count"
      if [[ "$now" -ge "$deadline" ]]; then
        if [[ "$cur_count" -eq 0 ]]; then
          G_CI_STATUS="EMPTY"
          return 1
        fi
        break # 部分的でも上限に達したら今の集合で判定に進む(下の判定が最終防御)
      fi
      sleep 5
    done
  fi

  # terminal state まで待つだけの道具。exit code は acceptance criterion にしない
  # (cli/cli#9973: 部分集合が pass した時点で早期終了することがある)。
  timeout "$CI_TIMEOUT" gh pr checks "$pr_num" -R "$nwo" "${required_flag[@]}" \
    --watch --fail-fast --interval 10 >/dev/null 2>&1 || true

  # 判定は --json を取り直して jq(judge)が行う。--watch の exit code は使わない。
  reported="$(reported_checks "$pr_num" "$nwo")"
  local judged rel_n missing_n failed_n pending_n
  judged="$(jq -cn --argjson e "$expected" --argjson r "$reported" '
    ( if ($e|length) > 0
      then [ $r[] | select(.name as $n | ($e | index($n)) != null) ]
      else $r
      end
    ) as $rel
    | {
        n: ($rel | length),
        missing: ($e - [$rel[].name]),
        failed: [$rel[] | select(.bucket == "fail" or .bucket == "cancel")],
        pending: [$rel[] | select(.bucket == "pending")]
      }' 2>/dev/null)" || judged=""
  [[ -n "$judged" ]] || judged='{"n":0,"missing":[],"failed":[],"pending":[]}'

  rel_n="$(jq -r '.n' <<<"$judged")" || rel_n=0
  missing_n="$(jq -r '.missing | length' <<<"$judged")" || missing_n=0
  failed_n="$(jq -r '.failed | length' <<<"$judged")" || failed_n=0
  pending_n="$(jq -r '.pending | length' <<<"$judged")" || pending_n=0

  if [[ "$mode" == "quiesce" && "$rel_n" -eq 0 ]]; then
    G_CI_STATUS="EMPTY"
    return 1
  fi
  if [[ "$missing_n" -gt 0 ]]; then
    G_CI_STATUS="MISSING"
    G_CI_DETAIL="$(jq -r '.missing | join(", ")' <<<"$judged")" || G_CI_DETAIL="(不明)"
    return 1
  fi
  if [[ "$failed_n" -gt 0 ]]; then
    G_CI_STATUS="FAILED"
    return 1
  fi
  if [[ "$pending_n" -gt 0 ]]; then
    G_CI_STATUS="PENDING"
    return 1
  fi
  G_CI_STATUS="PASS"
  G_CI_DETAIL="$note"
  return 0
}

# --- block / escalate ----------------------------------------------------------

block_or_escalate() {
  local sid="$1" message="$2" n
  n="$(bump_count "$sid")"
  if [[ "$n" -ge "$MAX_BLOCKS" ]]; then
    : >"$(escalated_file "$sid")"
    cat >&2 <<EOF
${MAX_BLOCKS} 回ブロックしましたが解消していません。残存:
${message}

AskUserQuestion で GO/NO-GO を取ってから終了してください。
(次回以降このゲートは素通りします)
EOF
  else
    printf '%s\n' "$message" >&2
  fi
  exit 2
}

# --- SessionStart hook ----------------------------------------------------------

cmd_session_start() {
  # command -v は複数引数を渡すと「1つでも見つかれば exit 0」になる(全部揃っている
  # ことの検査にならない)ので、既存 hook(issue-index.sh 等)と同じく単発で確認する。
  command -v jq >/dev/null 2>&1 || exit 0
  command -v gh >/dev/null 2>&1 || exit 0
  command -v git >/dev/null 2>&1 || exit 0
  local input project nwo
  input="$(cat)"
  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input" 2>/dev/null)}" || project=""
  [[ -n "$project" && -d "$project" ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0
  nwo="$(owner_repo "$project")" || exit 0
  allowlisted "$nwo" || exit 0
  [[ -f "$STATE_ROOT/skip" ]] && exit 0
  [[ "${SKIP_PR_GATE:-0}" == "1" ]] && exit 0

  local common_dir
  common_dir="$(git_common_dir "$project")" || exit 0

  local do_fetch=1 fetch_note=""
  if [[ -f "$common_dir/FETCH_HEAD" ]]; then
    local mtime now
    mtime="$(stat -c %Y "$common_dir/FETCH_HEAD" 2>/dev/null)" || mtime=0
    now="$(date +%s)"
    ((now - mtime < FETCH_TTL)) && do_fetch=0
  fi
  if [[ "$do_fetch" == 1 ]]; then
    if ! timeout 15 git -C "$project" fetch --quiet --prune origin 2>/dev/null; then
      fetch_note=" (リモート未確認: fetch 失敗)"
    fi
  fi

  local branch
  branch="$(git -C "$project" branch --show-current 2>/dev/null)" || branch=""
  [[ -n "$branch" ]] || exit 0

  local pr_json pr_num
  pr_json="$(gh pr list -R "$nwo" --head "$branch" --state open --limit 1 \
    --json number,baseRefName,headRefOid 2>/dev/null)" || pr_json=""
  [[ -n "$pr_json" ]] || pr_json='[]'
  pr_num="$(jq -r '.[0].number // empty' <<<"$pr_json")"
  [[ -n "$pr_num" ]] || exit 0 # PR が無ければ運ぶ情報が無い(PR 有無自体は issue-index が示す)

  local base head_oid
  base="$(jq -r '.[0].baseRefName' <<<"$pr_json")"
  head_oid="$(jq -r '.[0].headRefOid' <<<"$pr_json")"

  local ab ahead behind unpushed="?"
  ab="$(compute_ahead_behind "$project" "$base")"
  ahead="${ab%%$'\t'*}"
  behind="${ab##*$'\t'}"
  if git -C "$project" cat-file -e "$head_oid" 2>/dev/null; then
    unpushed="$(git -C "$project" rev-list --count "$head_oid..HEAD" 2>/dev/null)" || unpushed="?"
  fi

  local checks_json bucket_summary
  checks_json="$(reported_checks "$pr_num" "$nwo")"
  bucket_summary="$(jq -r '
    if length == 0 then "不明"
    else [.[].bucket] | group_by(.) | map("\(.[0]): \(length)") | join(", ")
    end' <<<"$checks_json" 2>/dev/null)" || bucket_summary="不明"

  local ctx
  ctx="[pr-gate] PR #${pr_num} (base: ${base}) — CI: ${bucket_summary}${fetch_note}
base 追従: ahead ${ahead} / behind ${behind}
未 push: ${unpushed} 件"

  jq -n --arg ctx "$ctx" '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
  exit 0
}

# --- Stop hook ------------------------------------------------------------------

cmd_stop() {
  # command -v の複数引数の落とし穴は cmd_session_start と同じ。単発で確認する。
  command -v jq >/dev/null 2>&1 || exit 0
  command -v gh >/dev/null 2>&1 || exit 0
  command -v git >/dev/null 2>&1 || exit 0
  [[ -f "$STATE_ROOT/skip" ]] && exit 0
  [[ "${SKIP_PR_GATE:-0}" == "1" ]] && exit 0

  local input sid project nwo
  input="$(cat)"
  sid="$(jq -r '.session_id // "unknown"' <<<"$input" 2>/dev/null)" || sid="unknown"
  sid="${sid//[^A-Za-z0-9._-]/_}"

  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input" 2>/dev/null)}" || project=""
  [[ -n "$project" && -d "$project" ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0
  nwo="$(owner_repo "$project")" || exit 0
  allowlisted "$nwo" || exit 0

  ensure_dirs
  [[ -f "$(escalated_file "$sid")" ]] && exit 0
  [[ "$(read_count "$sid")" -ge "$MAX_BLOCKS" ]] && exit 0

  local branch
  branch="$(git -C "$project" branch --show-current 2>/dev/null)" || branch=""
  [[ -n "$branch" ]] || exit 0

  local pr_json pr_num
  pr_json="$(gh pr list -R "$nwo" --head "$branch" --state open --limit 1 \
    --json number,baseRefName,headRefOid 2>/dev/null)" || pr_json=""
  [[ -n "$pr_json" ]] || pr_json='[]'
  pr_num="$(jq -r '.[0].number // empty' <<<"$pr_json")"
  [[ -n "$pr_num" ]] || exit 0 # open PR が無い → 完全沈黙

  local base head_oid
  base="$(jq -r '.[0].baseRefName' <<<"$pr_json")"
  head_oid="$(jq -r '.[0].headRefOid' <<<"$pr_json")"

  # advisory: base 追従 / 未コミット(block 時だけ相乗り、単独では終了を止めない)
  local ab ahead behind wt_count advisory
  ab="$(compute_ahead_behind "$project" "$base")"
  ahead="${ab%%$'\t'*}"
  behind="${ab##*$'\t'}"
  wt_count="$(uncommitted_count "$project")"
  advisory="base 追従: ahead ${ahead} / behind ${behind}
未コミット: ${wt_count} ファイル"

  # G_push
  local local_head
  local_head="$(git -C "$project" rev-parse HEAD 2>/dev/null)" || local_head=""
  if [[ -n "$local_head" && "$local_head" != "$head_oid" ]]; then
    local unpushed="?"
    if git -C "$project" cat-file -e "$head_oid" 2>/dev/null; then
      unpushed="$(git -C "$project" rev-list --count "$head_oid..HEAD" 2>/dev/null)" || unpushed="?"
    fi
    block_or_escalate "$sid" "未 push の commit が ${unpushed} 件あります。
CI の緑は古い head (${head_oid:0:7}) の結果です。

push してから終了してください。
注: この worktree は pre-push に阻まれます。--no-verify を
    使う前にユーザーに確認してください。

${advisory}"
  fi

  # G_CI
  if ! run_g_ci "$nwo" "$pr_num" "$base"; then
    case "$G_CI_STATUS" in
      EMPTY)
        block_or_escalate "$sid" "CI のチェックがまだ 1 件も報告されていません。
gh pr checks で確認してから終わってください。

${advisory}"
        ;;
      MISSING)
        block_or_escalate "$sid" "CI のチェックが揃っていません。未出現: ${G_CI_DETAIL}
gh pr checks --watch で待ってから終わってください。

${advisory}"
        ;;
      FAILED)
        block_or_escalate "$sid" "CI が赤です。PR #${pr_num} (head ${head_oid:0:7})

$(render_failed_checks "$pr_num" "$nwo")
修正して push してから終わってください。

${advisory}"
        ;;
      *)
        block_or_escalate "$sid" "CI がまだ pending です。gh pr checks --watch で待ってから終わってください。

${advisory}"
        ;;
    esac
  fi

  if [[ "$G_CI_STATUS" == "PASS" && -n "$G_CI_DETAIL" ]]; then
    advisory="${advisory}
${G_CI_DETAIL}"
  fi
  printf '%s\n' "$advisory" >&2
  exit 0
}

# --- サブコマンド: --selftest ---------------------------------------------------

if [[ "${1:-}" == "--selftest" ]]; then
  self="$(self_path)"
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  check() { # check <名前> <期待> <実際>
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected [$2], got [$3])" >&2
      fail=1
    fi
  }
  check_grep() { # check_grep <名前> <パターン> <対象文字列>
    if grep -qF -- "$2" <<<"$3"; then
      echo "ok   $1"
    else
      echo "FAIL $1 (pattern [$2] not found in [$3])" >&2
      fail=1
    fi
  }

  # --- gh スタブ ---
  # PR_GATE_STUB_NO_PR=1            : gh pr list が [] を返す
  # PR_GATE_STUB_PR_NUM / _BASE / _HEAD_OID : gh pr list が返す PR の中身
  # PR_GATE_STUB_RULES_FILE         : gh api rules/branches の応答(未指定なら [])
  # PR_GATE_STUB_CHECKS_FILE        : gh pr checks --json の応答(未指定なら [])
  # PR_GATE_STUB_WATCH_RC           : gh pr checks --watch の exit code(既定 0)
  mkdir -p "$dir/bin"
  cat >"$dir/bin/gh" <<'STUB'
#!/usr/bin/env bash
jqbin="$(command -v jq)"
case "$1" in
  pr)
    case "$2" in
      list)
        if [[ "${PR_GATE_STUB_NO_PR:-0}" == "1" ]]; then
          echo '[]'
        else
          "$jqbin" -n --arg num "${PR_GATE_STUB_PR_NUM:-37}" \
            --arg base "${PR_GATE_STUB_BASE:-main}" \
            --arg head "${PR_GATE_STUB_HEAD_OID:-0000000000000000000000000000000000000000}" \
            '[{number:($num|tonumber), baseRefName:$base, headRefOid:$head}]'
        fi
        ;;
      checks)
        if [[ "$*" == *"--watch"* ]]; then
          exit "${PR_GATE_STUB_WATCH_RC:-0}"
        fi
        if [[ -n "${PR_GATE_STUB_CHECKS_FILE:-}" && -f "${PR_GATE_STUB_CHECKS_FILE:-}" ]]; then
          cat "${PR_GATE_STUB_CHECKS_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  api)
    case "$*" in
      *rules/branches*)
        if [[ -n "${PR_GATE_STUB_RULES_FILE:-}" && -f "${PR_GATE_STUB_RULES_FILE:-}" ]]; then
          cat "${PR_GATE_STUB_RULES_FILE}"
        else
          echo '[]'
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$dir/bin/gh"
  stub_path="$dir/bin:$PATH"

  # --- 実験環境: github remote 付きの git repo ---
  repo="$dir/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" -c user.email=t@example.com -c user.name=t commit --allow-empty -q -m base
  git -C "$repo" remote add origin https://github.com/example/example.git
  git -C "$repo" update-ref refs/remotes/origin/main "$(git -C "$repo" rev-parse HEAD)"
  # fetch(ネットワーク I/O)は selftest の対象外。FETCH_HEAD を touch して
  # TTL 判定(「新しければ fetch しない」)だけを検査する。
  touch "$repo/.git/FETCH_HEAD"
  git -C "$repo" -c user.email=t@example.com -c user.name=t commit --allow-empty -q -m c1
  git -C "$repo" -c user.email=t@example.com -c user.name=t commit --allow-empty -q -m c2
  real_head="$(git -C "$repo" rev-parse HEAD)"

  export PR_GATE_DIR="$dir/state" PR_GATE_ALLOWLIST="$dir/allowlist"
  export PR_GATE_CHECK_APPEAR_TIMEOUT=0 PR_GATE_QUIESCE=0 PR_GATE_CI_TIMEOUT=30
  printf 'example/example\n' >"$PR_GATE_ALLOWLIST"

  hookinput() { printf '{"cwd":"%s","session_id":"%s"}' "$repo" "${1:-selftest-sid}"; }

  echo "対象外(完全沈黙):"

  other="$dir/other-repo"
  mkdir -p "$other"
  git -C "$other" init -q
  git -C "$other" remote add origin https://github.com/other/other.git
  rc=0
  out="$(PATH="$stub_path" CLAUDE_PROJECT_DIR="$other" bash "$self" stop <<<'{"cwd":"'"$other"'"}' 2>"$dir/err")" || rc=$?
  check "allowlist 外(stop): exit 0" 0 "$rc"
  check "allowlist 外(stop): stdout 空" "" "$out"
  check "allowlist 外(stop): stderr 空" "" "$(cat "$dir/err")"

  mkdir -p "$dir/nogh"
  for c in jq git awk sed grep basename dirname cat wc tr stat sleep date timeout mktemp find chmod mkdir; do
    ln -sf "$(command -v "$c")" "$dir/nogh/$c" 2>/dev/null || true
  done
  rc=0
  out="$(PATH="$dir/nogh" CLAUDE_PROJECT_DIR="$repo" "$BASH" "$self" stop <<<"$(hookinput)" 2>"$dir/err")" || rc=$?
  check "gh 不在(stop): exit 0" 0 "$rc"
  check "gh 不在(stop): 出力なし" "" "$out$(cat "$dir/err")"

  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" stop <<<"$(hookinput)" 2>"$dir/err")" || rc=$?
  check "open PR 無し: exit 0" 0 "$rc"
  check "open PR 無し: 出力なし" "" "$out$(cat "$dir/err")"

  echo "G_push:"

  rc=0
  out="$(PR_GATE_STUB_HEAD_OID=0000000000000000000000000000000000000000 \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput)" 2>"$dir/err")" || rc=$?
  errtext="$(cat "$dir/err")"
  check "未push で block: exit 2" 2 "$rc"
  check_grep "未push で block: メッセージに '未 push'" "未 push" "$errtext"
  check_grep "未push で block: --no-verify の注意" "--no-verify" "$errtext"

  echo "G_CI (PASS 経路の到達可能性 — #26 の回帰対象):"

  jq -n '[{context:"job-a"},{context:"job-b"}]' >/dev/null 2>&1 || true # jq 動作確認
  jq -n '[{type:"required_status_checks",
    parameters:{required_status_checks:[{context:"job-a"},{context:"job-b"}]}}]' \
    >"$dir/rules-2.json"
  jq -n '[{name:"job-a",bucket:"pass",link:"https://x/runs/1/job/11",workflow:"CI"},
          {name:"job-b",bucket:"pass",link:"https://x/runs/1/job/12",workflow:"CI"}]' \
    >"$dir/checks-2pass.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-2pass.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput pass-sid)" 2>"$dir/err")" || rc=$?
  check "push 済み + 全 required pass: 素通り(exit 0)" 0 "$rc"

  echo "G_CI (揃っていない集合を緑と読まない — R1/R2 の回帰対象):"

  jq -n '[]' >"$dir/checks-empty.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_CHECKS_FILE="$dir/checks-empty.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput empty-sid)" 2>"$dir/err")" || rc=$?
  check "チェック0件: exit 2" 2 "$rc"
  check_grep "チェック0件: メッセージ" "1 件も報告されていません" "$(cat "$dir/err")"

  jq -n '[{name:"job-a",bucket:"pass",link:"https://x/runs/1/job/11",workflow:"CI"}]' \
    >"$dir/checks-1of2.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-1of2.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput missing-sid)" 2>"$dir/err")" || rc=$?
  check "2件中1件しか出現していない: exit 2" 2 "$rc"
  check_grep "未出現の job-b が列挙される" "job-b" "$(cat "$dir/err")"

  # R2-C-1 / R2-C-2 の核心: 期待集合は揃って(出現待ちは通過)いるが、--watch が
  # exit 0 を返しても、判定に使う --json では job-b がまだ pending。
  # 判定が --watch の exit code に依存していたらこのケースは素通りしてしまう。
  jq -n '[{name:"job-a",bucket:"pass",link:"https://x/runs/1/job/11",workflow:"CI"},
          {name:"job-b",bucket:"pending",link:"https://x/runs/1/job/12",workflow:"CI"}]' \
    >"$dir/checks-partial-pending.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-partial-pending.json" PR_GATE_STUB_WATCH_RC=0 \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput partial-sid)" 2>"$dir/err")" || rc=$?
  check "--watch が exit 0 でも部分 pending は block される(--watch 非依存の検査)" 2 "$rc"
  check_grep "pending メッセージ" "pending" "$(cat "$dir/err")"

  jq -n '[{name:"job-a",bucket:"pass",link:"https://x/runs/1/job/11",workflow:"CI"},
          {name:"job-b",bucket:"fail",link:"https://x/runs/1/job/12",workflow:"CI"}]' \
    >"$dir/checks-partial-fail.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-partial-fail.json" PR_GATE_STUB_WATCH_RC=0 \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput failsid)" 2>"$dir/err")" || rc=$?
  check "job-b が fail: exit 2" 2 "$rc"
  check_grep "失敗ジョブ名が出る" "job-b" "$(cat "$dir/err")"
  check_grep "gh run view コマンドが出る" "gh run view 1 --log-failed --job 12" "$(cat "$dir/err")"

  echo "G_CI (stacked PR: required 0件 → quiesce フォールバック):"

  jq -n '[]' >"$dir/rules-empty.json"
  jq -n '[{name:"Shell script validation",bucket:"pass",link:"https://x/runs/2/job/21",workflow:"CI"}]' \
    >"$dir/checks-quiesce-pass.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-empty.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-quiesce-pass.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput quiesce-pass-sid)" 2>"$dir/err")" || rc=$?
  check "stacked PR + 全チェック pass: 素通り(exit 0)" 0 "$rc"
  check_grep "stacked PR: 注記が出る" "ruleset の対象外" "$(cat "$dir/err")"

  jq -n '[{name:"Shell script validation",bucket:"fail",link:"https://x/runs/2/job/21",workflow:"CI"}]' \
    >"$dir/checks-quiesce-fail.json"
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-empty.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-quiesce-fail.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput quiesce-fail-sid)" 2>"$dir/err")" || rc=$?
  check "stacked PR + 1件 fail: exit 2" 2 "$rc"

  echo "escalate (独自カウンタ + 上限):"

  n=0
  while [[ $n -lt 3 ]]; do
    rc=0
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput esc-sid)" \
      >"$dir/out" 2>"$dir/err" || rc=$?
    n=$((n + 1))
  done
  check "3 回目で escalate 文言" 1 "$(grep -Fc 'AskUserQuestion' "$dir/err")"
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput esc-sid)" \
    >"$dir/out" 2>"$dir/err" || rc=$?
  check "escalated 後は素通り(exit 0)" 0 "$rc"
  check "escalated 後は出力なし" "" "$(cat "$dir/out")$(cat "$dir/err")"

  echo "SessionStart:"

  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-2pass.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" session-start <<<"$(hookinput ss-sid)")" || rc=$?
  check "session-start: exit 0" 0 "$rc"
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check_grep "session-start: PR 番号が出る" "PR #37" "$ctx"
  check_grep "session-start: base 追従の ahead が出る(2 commit 進めた)" "ahead 2" "$ctx"
  check_grep "session-start: 未 push 0 件" "未 push: 0 件" "$ctx"

  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput ss-nopr-sid)")" || rc=$?
  check "session-start: PR 無しは完全沈黙" 0 "$rc"
  check "session-start: PR 無しは stdout 空" "" "$out"

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- dispatch --------------------------------------------------------------------

case "${1:-}" in
  session-start)
    cmd_session_start
    ;;
  stop)
    cmd_stop
    ;;
  *)
    exit 0
    ;;
esac
