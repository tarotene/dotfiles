#!/usr/bin/env bash
# pr-gate.sh — PR の完了を待つ Stop hook（+ 現在状態を運ぶ SessionStart hook）。
#
# 設計と根拠: docs/claude/pr-gate.md（このリポジトリ内）
#
# 「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」「PR を Issue に
# 繋がないまま終わる」という 3 種の事故を、Stop の 1 点だけで hard gate する。
# base 鮮度・未コミット変更は advisory
# (block するときだけ相乗りで伝える。それ単独では終了を止めない)。
#
#   G_push     : ローカル HEAD == PR の headRefOid              → block
#   G_unpushed : PR が無いときの「push すべきコミットが 0 件か」→ block
#   G_link     : PR 本文に closing keyword または No-Issue:     → block
#   G_CI       : 期待される check がすべて pass/skipping        → block
#   G_base     : origin/<base> に対する ahead/behind            → advisory
#   G_wt       : 未コミット件数                                 → advisory
#
# G_unpushed は G_push が届かない領域を塞ぐ: G_push は比較対象の headRefOid を
# open PR から取るので、PR がまだ無いセッションでは何も見ずに完全沈黙していた
# (実測: コミットを 3 つ積んで push せずに終わっても pr-gate は無言だった)。
# PR を作るべきかには踏み込まず、「積んだコミットが 1 個もリモートに無い」と
# いう事実だけを見る。解消は git push 1 回で済み、履歴改変は伴わない。
#
# SessionStart は同じ理由で「PR が無ければ完全沈黙」だったため、base が何コミット
# 遅れていても [gone] ブランチが何本溜まっていても一切表示されなかった。PR の
# 有無を問わず stale_base_line / gone_branches_line / stale_worktrees_line を計算し、
# 材料があれば単独で advisory を出す(こちらは Stop の advisory と違い、
# 「他の block への相乗り」に制約されない — SessionStart 自体には block/pass の
# 概念が無いので、単独発火のコストは無い)。
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
# G_link が塞ぐのは「マージが Issue に伝播せず、解決済みの Issue が open のまま
# 残る」事故。merged PR 21 本のうち closing keyword を持つのは 4 本(19%)で、
# #28/#29 は実質解決から数か月 open のまま残っていた。コストが出るのは PR の
# 時点ではなく、後日の再トリアージ時 — 遅延して現れる沈黙なので advisory では
# 構造がそのまま再生産される。よって block し、対応 Issue が本当に無い場合は
# 本文に `No-Issue: <理由>` と書いて明示的に抜ける。沈黙を決定に変えるのが狙いで、
# `No-Issue:` は grep 可能なので後から監査できる(advisory の警告と違う点)。
#
# G_link は「言及があるのに keyword が無い」だけを見る形にはしない。#28/#29 を
# 実質解決した PR は Issue に一切言及していなかった — 観測された失敗そのものを
# 通す判定には意味がない。
#
# closing keyword は default branch へのマージでのみ発火する。base がそれ以外の
# stacked PR では本文に書いてもマージで閉じないが、stacked 自体は正当な運用なので
# block ではなく advisory にする。default branch は refs/remotes/origin/HEAD から
# 読む — API 呼び出しを増やさない(取れなければこの advisory を出さない)。
#
# 参照先 Issue の実在確認はしない。捕まえられるのは「存在しない番号」だけで、
# より起きやすい「存在するが別の Issue」は捕まえられない一方、API 呼び出しが
# 1 本増えて下の縮退表が太る。利得が釣り合わない。
#
# 発火範囲: allowlist ファイル(既定 ~/.claude/pr-gate-repos)に nwo が無ければ
# 完全沈黙。全ホストに無条件配備する前提(会社リポジトリでは既定で沈黙する)。
#
# 縮退:
#   完全沈黙(exit 0, 出力なし) → allowlist 外 / git repo でない / GitHub remote
#     でない / jq・gh・git 不在 / skip / (PR が無く、かつ hygiene の材料も無い)
#   警告 1 行 + fail-open      → gh 未ログイン・API 失敗・fetch 失敗
#   hard block(exit 2)         → G_push 不一致 / G_unpushed(PR 無し + 未push
#     commit あり) / G_link 欠落 / G_CI が揃わない・失敗・pending
#
# `stop_hook_active` は見ない。wrapup-stop-gate.sh と同じ即 exit 0 にすると、
# G_push で 1 回 block した直後の再呼び出しが CI 判定に到達しない
# (docs/claude/copilot-plan-review.md の「第二次の非収束」と同型)。上限は独自カウンタ:
# state/<sid>.count が ${PR_GATE_MAX_BLOCKS:-4} に達したら 1 回だけ escalate し、
# touch state/<sid>.escalated。以後そのセッションは無条件で素通る(escalated の
# チェックは上限判定より前 — docs/claude/copilot-plan-review.md の closer と同じ置き方)。
# 上限が 3 でなく 4 なのは G_link を足したから: 最悪の連鎖(push → 本文修正 →
# CI 待ち)が正当に 3 回 block しうるので、3 のままだと最後の 1 回が escalate に
# 化ける。なお G_link の指摘は単独 block になる前に G_push / G_CI の block へ
# 相乗りするので、この最悪連鎖は実際には起きにくい。
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
MAX_BLOCKS="${PR_GATE_MAX_BLOCKS:-4}"

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

# --- hygiene advisories(事故①古い base / 事故④残骸): PR の有無を問わず計算できる ---
# 以下は API を叩かず git だけで求まる。SessionStart は「PR が無ければ完全沈黙」
# だったため、この worktree 自身が origin/main から複数コミット遅れていても、
# [gone] ブランチが溜まっていても一切表示されなかった。PR の有無に関わらず
# 単独の SessionStart advisory として出す(cmd_stop の advisory は既存どおり
# 「他の block に相乗り、単独では終了を止めない」のままにする — 履歴改変を
# 自動化しない判断は変えない)。

# stale base 行。$2(base)が空、origin/$2 が手元に無い、behind が 0 のいずれか
# なら空文字(呼び出し側は空なら行を足さない — 判定できないことを断定に変えない)。
stale_base_line() {
  local project="$1" base="$2" ab behind
  [[ -n "$base" ]] || return 0
  ab="$(compute_ahead_behind "$project" "$base")"
  behind="${ab##*$'\t'}"
  [[ "$behind" =~ ^[0-9]+$ ]] || return 0
  [[ "$behind" -gt 0 ]] || return 0
  printf 'base 追従: origin/%s から %s コミット遅れています(git pull --ff-only 等で追従してください)' \
    "$base" "$behind"
}

# [gone] ブランチ本数。0 なら空文字。
gone_branches_line() {
  local project="$1" n
  n="$(git -C "$project" for-each-ref --format='%(upstream:track)' refs/heads 2>/dev/null \
    | grep -Fxc '[gone]')" || n=0
  [[ "$n" =~ ^[0-9]+$ ]] || n=0
  [[ "$n" -gt 0 ]] || return 0
  printf '残骸ブランチ: [gone] が %s 本あります(git prune-branches で確認・削除)' "$n"
}

# [gone] かつ未変更の worktree 数。0 なら空文字。dirty な worktree は本物の
# 作業中の可能性があるので数えない(false positive を出さない側に倒す)。
stale_worktrees_line() {
  local project="$1" n=0 wt br track dirty
  while IFS= read -r wt; do
    [[ -n "$wt" ]] || continue
    br="$(git -C "$wt" branch --show-current 2>/dev/null)" || br=""
    [[ -n "$br" ]] || continue
    track="$(git -C "$project" for-each-ref --format='%(upstream:track)' "refs/heads/$br" 2>/dev/null)" || track=""
    [[ "$track" == "[gone]" ]] || continue
    dirty="$(uncommitted_count "$wt")"
    [[ "$dirty" == "0" ]] || continue
    n=$((n + 1))
  done < <(git -C "$project" worktree list --porcelain 2>/dev/null | sed -n 's#^worktree ##p')
  [[ "$n" -gt 0 ]] || return 0
  printf '残骸 worktree: [gone] かつ未変更の worktree が %s 個あります' "$n"
}

# 上の行(空文字は無視)を改行区切りで連結する。
join_hygiene_lines() {
  local line out=""
  for line in "$@"; do
    [[ -n "$line" ]] || continue
    if [[ -n "$out" ]]; then
      out="${out}
${line}"
    else
      out="$line"
    fi
  done
  printf '%s' "$out"
}

# --- G_unpushed(PR 未作成時の push 忘れ): 事故③のうち G_push が届かない領域 -----
# G_push は open PR の headRefOid と比較するので、PR がまだ無いセッションでは
# 何も見ずに完全沈黙していた(cmd_stop の `[[ -n "$pr_num" ]] || exit 0` がそれ)。
# PR を作るべきかには踏み込まず、「積んだコミットが 1 個もリモートに無い」事実
# だけを見る。解消は git push 1 回(push.autoSetupRemote が upstream を自動で
# 張るので --set-upstream の指定は要らない) — 履歴改変は伴わない。
unpushed_count() {
  local project="$1" branch="$2" upstream n
  upstream="$(git -C "$project" rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null)" || upstream=""
  if [[ -z "$upstream" ]]; then
    if git -C "$project" rev-parse --verify -q "origin/$branch" >/dev/null 2>&1; then
      upstream="origin/$branch"
    else
      local default_br
      default_br="$(default_branch "$project")"
      if [[ -n "$default_br" ]] && git -C "$project" rev-parse --verify -q "origin/$default_br" >/dev/null 2>&1; then
        upstream="origin/$default_br"
      fi
    fi
  fi
  if [[ -z "$upstream" ]]; then
    printf '?'
    return 0
  fi
  n="$(git -C "$project" rev-list --count "${upstream}..HEAD" 2>/dev/null)" || n="?"
  [[ -n "$n" ]] || n="?"
  printf '%s' "$n"
}

# --- G_link: PR 本文が Issue を閉じるか ---------------------------------------

# GitHub が解釈する closing keyword は close/closes/closed, fix/fixes/fixed,
# resolve/resolves/resolved の 9 語。参照形式は同一リポジトリの `#N` と
# クロスリポジトリの `owner/repo#N`。keyword は大文字でもよく、コロンを伴っても
# よい(`Closes: #10` も公式に解釈される)ので、いずれも受理する。
# 出典: docs.github.com「Linking a pull request to an issue」
#
# issue URL 形式は上記の表には無いが、書かれていれば意図は明白で、これを
# MISSING と判定すると gate が誤って止める側に倒れる。受理する。
CLOSING_KEYWORD_RE='(close[sd]?|fix(e[sd])?|resolve[sd]?)[[:space:]]*:?[[:space:]]+(#[0-9]+|[A-Za-z0-9._-]+/[A-Za-z0-9._-]+#[0-9]+|https://github\.com/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+/issues/[0-9]+)'

# `No-Issue:` は理由を伴って初めて成立する。空の `No-Issue:` を通すと、沈黙を
# 決定に変えるという G_link の目的が失われ、ただのおまじないになる。
NO_ISSUE_RE='^[[:space:]]*No-Issue:[[:space:]]*[^[:space:]]'

# GitHub は **コード内の closing keyword を解釈しない**。fenced code block の中も、
# `Closes #30` のようなインラインのコードスパンの中も無視される。判定前に両方
# 落としておかないと、ゲートが「閉じないのに LINKED」と読む — つまり G_link が
# 防ごうとしているまさにその事故(マージしても Issue が open のまま)を、ゲート自身
# が見逃す側に倒れる。
#
# これは机上の懸念ではない。この判定を入れた PR #46 の本文が
# 「本文に `Closes #30 / #33 / …` を明記し」と実例をコードスパンで引用しており、
# 初回の実地検証で `gh pr view --json closingIssuesReferences` が空を返して発覚した。
# 規約や設計を説明する PR ほど keyword を引用するので、踏む確率は低くない。
#
# 判定基準は「GitHub がどう読むか」であって「人がどう書いたつもりか」ではない。
# ここでは GitHub の parser に寄せる。
strip_code_spans() {
  awk '
    BEGIN { fence = 0 }
    {
      line = $0
      if (line ~ /^[[:space:]]*(```|~~~)/) { fence = 1 - fence; next }
      if (fence) next
      gsub(/`[^`]*`/, " ", line)
      print line
    }
  ' <<<"$1"
}

judge_link() { # judge_link <body> ; echo LINKED|NO_ISSUE|MISSING
  local body
  body="$(strip_code_spans "$1")"
  if grep -Eiq -- "$CLOSING_KEYWORD_RE" <<<"$body"; then
    printf 'LINKED'
  elif grep -Eiq -- "$NO_ISSUE_RE" <<<"$body"; then
    printf 'NO_ISSUE'
  else
    printf 'MISSING'
  fi
}

# default branch を refs/remotes/origin/HEAD から読む。API を叩かない代わりに、
# origin/HEAD が未設定のクローンでは空を返す — 呼び出し側はその場合 advisory を
# 出さない(判定できないことを断定に変えない)。
default_branch() {
  local ref
  ref="$(git -C "$1" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)" || return 0
  printf '%s' "${ref#origin/}"
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

  # hygiene advisory(事故①④): PR の有無を問わず計算できる。API は叩かない。
  local gone_line stale_wt_line
  gone_line="$(gone_branches_line "$project")"
  stale_wt_line="$(stale_worktrees_line "$project")"

  local pr_json pr_num
  pr_json="$(gh pr list -R "$nwo" --head "$branch" --state open --limit 1 \
    --json number,baseRefName,headRefOid,body 2>/dev/null)" || pr_json=""
  [[ -n "$pr_json" ]] || pr_json='[]'
  pr_num="$(jq -r '.[0].number // empty' <<<"$pr_json")"

  if [[ -z "$pr_num" ]]; then
    # 以前はここで無条件 exit 0 していたため、PR を作る前のセッションでは
    # base がどれだけ遅れていても [gone] が何本あっても一切表示されなかった
    # (この worktree 自身が origin/main から 4 コミット遅れていて実際に無音
    # だったケース)。PR 有無自体は issue-index が示すので、ここでは
    # hygiene の材料が無ければやはり無音にする。
    local default_br stale_line hygiene
    default_br="$(default_branch "$project")"
    stale_line="$(stale_base_line "$project" "$default_br")"
    hygiene="$(join_hygiene_lines "$stale_line" "$gone_line" "$stale_wt_line")"
    [[ -n "$hygiene" ]] || exit 0
    jq -n --arg ctx "[pr-gate] ${hygiene}" \
      '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
    exit 0
  fi

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

  local link_state
  case "$(judge_link "$(jq -r '.[0].body // ""' <<<"$pr_json")")" in
    LINKED) link_state="closing keyword あり" ;;
    NO_ISSUE) link_state="No-Issue: 宣言あり" ;;
    *) link_state="なし — Closes #N か No-Issue: が要ります" ;;
  esac

  local ctx
  ctx="[pr-gate] PR #${pr_num} (base: ${base}) — CI: ${bucket_summary}${fetch_note}
Issue リンク: ${link_state}
base 追従: ahead ${ahead} / behind ${behind}
未 push: ${unpushed} 件"

  # PR がある場合の base 追従は上の ahead/behind 行がすでに単独で運んでいるので
  # stale_base_line は重ねない。gone/worktree 残骸は PR の有無と無関係な情報
  # なので、こちらには追加する。
  local extra_hygiene
  extra_hygiene="$(join_hygiene_lines "$gone_line" "$stale_wt_line")"
  if [[ -n "$extra_hygiene" ]]; then
    ctx="${ctx}
${extra_hygiene}"
  fi

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
    --json number,baseRefName,headRefOid,body 2>/dev/null)" || pr_json=""
  [[ -n "$pr_json" ]] || pr_json='[]'
  pr_num="$(jq -r '.[0].number // empty' <<<"$pr_json")"

  if [[ -z "$pr_num" ]]; then
    # G_push は open PR の headRefOid が無いと判定できない。以前はここで
    # 無条件に完全沈黙していたため、コミットを積んで push せずに終わる
    # セッションを何も止めなかった。PR を作るべきかには踏み込まず、
    # 「積んだコミットが 1 個もリモートに無い」事実だけを見る(G_unpushed)。
    local unpushed
    unpushed="$(unpushed_count "$project" "$branch")"
    if [[ "$unpushed" =~ ^[0-9]+$ ]] && [[ "$unpushed" -gt 0 ]]; then
      local default_br hygiene
      default_br="$(default_branch "$project")"
      hygiene="$(join_hygiene_lines \
        "$(stale_base_line "$project" "$default_br")" \
        "未コミット: $(uncommitted_count "$project") ファイル")"
      block_or_escalate "$sid" "未 push の commit が ${unpushed} 件あります。PR はまだありません。

push してから終了してください(push.autoSetupRemote が upstream を自動で
設定するので --set-upstream の指定は要りません)。

${hygiene}"
    fi
    exit 0 # 未 push が無ければ運ぶ情報が無い(PR 有無自体は issue-index が示す)
  fi

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

  # G_link — 判定だけ先に済ませ、block は最後に回す。G_push / G_CI が止める場面
  # では、その block メッセージに相乗りさせる($rider)。本文の修正は CI を待たずに
  # 済むので、単独で 1 往復を消費させる理由がない。
  local body link_verdict link_msg="" link_blocks=0 default_br
  body="$(jq -r '.[0].body // ""' <<<"$pr_json")"
  link_verdict="$(judge_link "$body")"
  default_br="$(default_branch "$project")"

  if [[ "$link_verdict" == "MISSING" ]]; then
    link_blocks=1
    link_msg="PR #${pr_num} の本文が Issue を閉じません(closing keyword なし)。

このままマージしても Issue は open のまま残り、後日の再トリアージまで
誰も気づきません。次のどちらかを本文に入れてください:

  Closes #<番号>          — 対応する Issue がある場合(複数なら各行に)
  No-Issue: <理由>        — 対応する Issue が本当に無い場合

  gh pr edit ${pr_num} --body-file <file>

open な Issue の一覧は SessionStart の issue-index が注入しています。"
  elif [[ "$link_verdict" == "LINKED" && -n "$default_br" && "$base" != "$default_br" ]]; then
    # 公式仕様: closing keyword は default branch を狙う PR でのみ解釈される。
    # stacked PR 自体は正当なので断定せず advisory に留める。
    advisory="${advisory}
注意: base が ${base} で default branch(${default_br})ではないため、本文の
      closing keyword はマージしても発火しません。"
  fi

  local rider="$advisory"
  [[ -n "$link_msg" ]] && rider="${link_msg}

${advisory}"

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
注: herdr worktree からの push は pre-push の例外です(#39)。手動で切った
    worktree の場合のみ阻まれるので、その場合は --no-verify を使う前に
    ユーザーに確認してください。

${rider}"
  fi

  # G_CI
  if ! run_g_ci "$nwo" "$pr_num" "$base"; then
    case "$G_CI_STATUS" in
      EMPTY)
        block_or_escalate "$sid" "CI のチェックがまだ 1 件も報告されていません。
gh pr checks で確認してから終わってください。

${rider}"
        ;;
      MISSING)
        block_or_escalate "$sid" "CI のチェックが揃っていません。未出現: ${G_CI_DETAIL}
gh pr checks --watch で待ってから終わってください。

${rider}"
        ;;
      FAILED)
        block_or_escalate "$sid" "CI が赤です。PR #${pr_num} (head ${head_oid:0:7})

$(render_failed_checks "$pr_num" "$nwo")
修正して push してから終わってください。

${rider}"
        ;;
      *)
        block_or_escalate "$sid" "CI がまだ pending です。gh pr checks --watch で待ってから終わってください。

${rider}"
        ;;
    esac
  fi

  if [[ "$G_CI_STATUS" == "PASS" && -n "$G_CI_DETAIL" ]]; then
    advisory="${advisory}
${G_CI_DETAIL}"
  fi

  # G_link を単独で block するのはここ — push も CI も通っている、つまり
  # 「あとは終わるだけ」の一点。まさにリンクが忘れられる瞬間なので、この位置で
  # 止める。上の block 経路を通った場合は既に $rider として伝えてある。
  if ((link_blocks)); then
    block_or_escalate "$sid" "${link_msg}

${advisory}"
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
  # PR_GATE_STUB_PR_BODY            : gh pr list が返す PR 本文
  #   既定は "Closes #1" — G_link を足す前から在った緑パスのケースを緑のまま
  #   保つため。G_link 自体の検査は下でこの変数を明示的に上書きして行う。
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
            --arg body "${PR_GATE_STUB_PR_BODY-Closes #1}" \
            '[{number:($num|tonumber), baseRefName:$base, headRefOid:$head, body:$body}]'
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

  # 「PR 無し」は以前は無条件で完全沈黙していたが、G_unpushed を足したので
  # そうではなくなった。専用のセクションで両方の分岐(未 push あり/無し)を
  # 検査する(下の「G_unpushed」セクション)。

  echo "G_push:"

  rc=0
  out="$(PR_GATE_STUB_HEAD_OID=0000000000000000000000000000000000000000 \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput)" 2>"$dir/err")" || rc=$?
  errtext="$(cat "$dir/err")"
  check "未push で block: exit 2" 2 "$rc"
  check_grep "未push で block: メッセージに '未 push'" "未 push" "$errtext"
  check_grep "未push で block: --no-verify の注意" "--no-verify" "$errtext"

  echo "G_unpushed (PR がまだ無いときの push 忘れ):"

  # 専用の追跡 ref を使う — origin/main は他のテスト(SessionStart の "ahead 2"
  # 等)が古い位置のままであることを前提にしているので、ここでは触らない。
  cur_branch="$(git -C "$repo" branch --show-current)"
  git -C "$repo" update-ref refs/remotes/origin/gate-test-upstream "$(git -C "$repo" rev-parse HEAD~2)"
  git -C "$repo" config "branch.${cur_branch}.remote" origin
  git -C "$repo" config "branch.${cur_branch}.merge" refs/heads/gate-test-upstream

  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" stop <<<"$(hookinput gu-behind-sid)" 2>"$dir/err")" || rc=$?
  check "PR 無し + 未push commit あり: exit 2" 2 "$rc"
  check_grep "PR 無し + 未push: 件数が出る" "未 push の commit が 2 件" "$(cat "$dir/err")"
  check_grep "PR 無し + 未push: push を促す" "push してから終了してください" "$(cat "$dir/err")"

  git -C "$repo" update-ref refs/remotes/origin/gate-test-upstream "$real_head"
  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" stop <<<"$(hookinput gu-clean-sid)" 2>"$dir/err")" || rc=$?
  check "PR 無し + 未push commit 0 件: exit 0" 0 "$rc"
  check "PR 無し + 未push commit 0 件: 出力なし" "" "$out$(cat "$dir/err")"

  # 後続の(PR ありを前提とする)テストに影響しないよう戻す。
  git -C "$repo" config --unset "branch.${cur_branch}.remote" || true
  git -C "$repo" config --unset "branch.${cur_branch}.merge" || true

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

  echo "G_link (PR 本文 → Issue のリンク):"

  # 以降の G_link ケースは push 済み + 全 required pass、つまり「あとは終わるだけ」
  # の状態で回す。リンクが忘れられるのがまさにこの一点だから。
  glink() { # glink <sid> <body>  → exit code を返し、stderr を $dir/err に置く
    local sid="$1" body="$2" rc=0
    PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
      PR_GATE_STUB_CHECKS_FILE="$dir/checks-2pass.json" PR_GATE_STUB_PR_BODY="$body" \
      PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop \
      <<<"$(hookinput "$sid")" >"$dir/out" 2>"$dir/err" || rc=$?
    printf '%s' "$rc"
  }

  rc="$(glink link-missing-sid "本文に Issue への言及が一切ない PR。")"
  check "closing keyword も No-Issue: も無い: exit 2" 2 "$rc"
  check_grep "block メッセージが Closes を教える" "Closes #<番号>" "$(cat "$dir/err")"
  check_grep "block メッセージが No-Issue: を教える" "No-Issue: <理由>" "$(cat "$dir/err")"

  # #28/#29 を実質解決した PR は Issue に一切言及していなかった。「言及はあるが
  # keyword が無い」だけを見る判定では、観測された失敗そのものを通してしまう。
  rc="$(glink link-mention-only-sid "関連: #30 の調査で見つけた問題を直す。")"
  check "番号への言及はあるが keyword が無い: exit 2" 2 "$rc"

  rc="$(glink link-closes-sid "本文。

Closes #30")"
  check "Closes #N: 素通り(exit 0)" 0 "$rc"

  rc="$(glink link-colon-sid "CLOSES: #30")"
  check "大文字 + コロン形式も GitHub の仕様どおり受理" 0 "$rc"

  rc="$(glink link-crossrepo-sid "Fixes octo-org/octo-repo#100")"
  check "クロスリポジトリ形式を受理" 0 "$rc"

  rc="$(glink link-url-sid "Resolves https://github.com/example/example/issues/7")"
  check "issue URL 形式を受理" 0 "$rc"

  rc="$(glink link-noissue-sid "No-Issue: セッション中に生まれた作業で対応 Issue が無い")"
  check "No-Issue: + 理由: 素通り(exit 0)" 0 "$rc"

  # 理由の無い `No-Issue:` を通すと、沈黙を決定に変えるという狙いが失われる。
  rc="$(glink link-noissue-bare-sid "No-Issue:")"
  check "理由の無い No-Issue: は逃がさない(exit 2)" 2 "$rc"

  # GitHub はコード内の keyword を解釈しない。ゲートがここで LINKED と読むと、
  # 「マージしても閉じない PR」を通してしまい、G_link の存在意義が消える。
  # PR #46 の実地検証で実際に踏んだ回帰(closingIssuesReferences が空だった)。
  rc="$(glink link-inline-code-sid "本文に \`Closes #30\` と書く規約を説明する PR。")"
  check "インラインのコードスパン内の keyword は数えない(exit 2)" 2 "$rc"

  rc="$(glink link-fence-sid "規約の例:

\`\`\`
Closes #30
\`\`\`

説明の続き。")"
  check "fenced code block 内の keyword は数えない(exit 2)" 2 "$rc"

  rc="$(glink link-fence-plus-real-sid "規約の例:

\`\`\`
Closes #99
\`\`\`

Closes #30")"
  check "コード外に本物があれば LINKED(exit 0)" 0 "$rc"

  rc="$(glink link-code-then-noissue-sid "\`Closes #99\` の書き方を説明する PR。

No-Issue: 規約を説明するだけで対応 Issue は無い")"
  check "コード内 keyword + No-Issue: は No-Issue: が効く(exit 0)" 0 "$rc"

  # G_push が止める場面では、本文の指摘は単独 block ではなく相乗りで伝える
  # (本文修正は CI を待たずに済むので 1 往復を消費させない)。
  rc=0
  PR_GATE_STUB_HEAD_OID=0000000000000000000000000000000000000000 \
    PR_GATE_STUB_PR_BODY="言及なし" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop \
    <<<"$(hookinput link-rider-sid)" >"$dir/out" 2>"$dir/err" || rc=$?
  errtext="$(cat "$dir/err")"
  check "G_push block 時: exit 2" 2 "$rc"
  check_grep "G_push block に G_link が相乗りする" "closing keyword なし" "$errtext"
  check_grep "G_push の指摘も残る" "未 push" "$errtext"

  # closing keyword は default branch へのマージでのみ発火する(GitHub 公式)。
  # stacked PR 自体は正当なので block ではなく advisory。
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  rc=0
  PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_BASE=stacked-base \
    PR_GATE_STUB_RULES_FILE="$dir/rules-empty.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-quiesce-pass.json" \
    PR_GATE_STUB_PR_BODY="Closes #30" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop \
    <<<"$(hookinput link-offbase-sid)" >"$dir/out" 2>"$dir/err" || rc=$?
  check "base が default branch でない: block はしない(exit 0)" 0 "$rc"
  check_grep "発火しない旨の advisory が出る" "発火しません" "$(cat "$dir/err")"

  # origin/HEAD が読めないクローンでは advisory を出さない(判定できないことを
  # 断定に変えない)。
  git -C "$repo" symbolic-ref --delete refs/remotes/origin/HEAD
  rc=0
  PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_BASE=stacked-base \
    PR_GATE_STUB_RULES_FILE="$dir/rules-empty.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-quiesce-pass.json" \
    PR_GATE_STUB_PR_BODY="Closes #30" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop \
    <<<"$(hookinput link-nohead-sid)" >"$dir/out" 2>"$dir/err" || rc=$?
  check "origin/HEAD 不明: exit 0" 0 "$rc"
  check "origin/HEAD 不明: 発火しない旨は出さない" 0 "$(grep -Fc '発火しません' "$dir/err")"

  echo "escalate (独自カウンタ + 上限):"

  n=0
  while [[ $n -lt 4 ]]; do
    rc=0
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" stop <<<"$(hookinput esc-sid)" \
      >"$dir/out" 2>"$dir/err" || rc=$?
    n=$((n + 1))
  done
  check "4 回目で escalate 文言" 1 "$(grep -Fc 'AskUserQuestion' "$dir/err")"
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
  check_grep "session-start: Issue リンクの状態が出る" "Issue リンク: closing keyword あり" "$ctx"

  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput ss-nopr-sid)")" || rc=$?
  check "session-start: PR 無しは完全沈黙" 0 "$rc"
  check "session-start: PR 無しは stdout 空" "" "$out"

  echo "hygiene advisories(事故①古い base / 事故④残骸):"

  # [gone] は本物の fetch --prune がなくても、branch.*.merge が指す remote-tracking
  # ref が存在しないだけで git 自身が判定してくれる(実測で確認済み)。
  git -C "$repo" branch hygiene-gone-branch >/dev/null
  git -C "$repo" config branch.hygiene-gone-branch.remote origin
  git -C "$repo" config branch.hygiene-gone-branch.merge refs/heads/hygiene-nonexistent

  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput hyg-gone-sid)")" || rc=$?
  check "hygiene: [gone] ありでも session-start exit 0" 0 "$rc"
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check_grep "hygiene: PR 無しでも [gone] 本数が出る" "残骸ブランチ: [gone] が 1 本" "$ctx"

  # 同じ [gone] ブランチを PR ありの経路(extra_hygiene)でも確認する。
  git -C "$repo" branch hygiene-gone-branch2 >/dev/null
  git -C "$repo" config branch.hygiene-gone-branch2.remote origin
  git -C "$repo" config branch.hygiene-gone-branch2.merge refs/heads/hygiene-nonexistent2
  rc=0
  out="$(PR_GATE_STUB_HEAD_OID="$real_head" PR_GATE_STUB_RULES_FILE="$dir/rules-2.json" \
    PR_GATE_STUB_CHECKS_FILE="$dir/checks-2pass.json" \
    PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" bash "$self" session-start <<<"$(hookinput hyg-withpr-sid)")" || rc=$?
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check_grep "hygiene: PR ありでも [gone] 本数が付く" "残骸ブランチ: [gone] が 2 本" "$ctx"
  git -C "$repo" branch -D hygiene-gone-branch2

  # 残骸 worktree: [gone] ブランチを別 worktree に checkout して clean のまま。
  wt="$dir/hygiene-wt"
  git -C "$repo" worktree add -q "$wt" hygiene-gone-branch
  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput hyg-wt-sid)")" || rc=$?
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check_grep "hygiene: 残骸 worktree 数が出る" "残骸 worktree: [gone] かつ未変更の worktree が 1 個" "$ctx"

  # dirty にすると数えない(false positive を出さない側に倒す)。
  touch "$wt/dirty.txt"
  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput hyg-wt-dirty-sid)")" || rc=$?
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check "hygiene: dirty な worktree は残骸として数えない" 0 "$(grep -Fc '残骸 worktree' <<<"$ctx")"

  git -C "$repo" worktree remove --force "$wt"
  git -C "$repo" branch -D hygiene-gone-branch

  echo "hygiene: stale base(origin/HEAD 設定時):"
  # 「ahead」(ローカルに未 push の commit がある)と「behind」(origin 側が
  # 進んでいて追従していない)は別の事象 — HEAD が origin/main の祖先のままで
  # なければ behind は生まれない。origin/main を HEAD と共通祖先を持つ別の
  # 1 commit へ付け替えて、本物の divergence(ahead 2 / behind 1)を作る。
  base_commit="$(git -C "$repo" rev-parse HEAD~2)"
  other_commit="$(git -C "$repo" -c user.email=t@example.com -c user.name=t \
    commit-tree "${base_commit}^{tree}" -p "$base_commit" -m other-team-commit)"
  git -C "$repo" update-ref refs/remotes/origin/main "$other_commit"
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  rc=0
  out="$(PR_GATE_STUB_NO_PR=1 PATH="$stub_path" CLAUDE_PROJECT_DIR="$repo" \
    bash "$self" session-start <<<"$(hookinput hyg-base-sid)")" || rc=$?
  ctx="$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" 2>/dev/null)" || ctx=""
  check_grep "hygiene: stale base 行が出る(behind 1)" "base 追従: origin/main から 1 コミット遅れています" "$ctx"
  git -C "$repo" symbolic-ref --delete refs/remotes/origin/HEAD

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
