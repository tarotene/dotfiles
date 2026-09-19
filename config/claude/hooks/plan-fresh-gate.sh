#!/usr/bin/env bash
# plan-fresh-gate.sh — ExitPlanMode 直前に origin/<base> の進行を検出し、
# プランが参照しているファイルと交差するときだけ deny する。
#
# 設計と根拠: docs/claude/plan-fresh-gate.md
#
# herdr worktree を並行 Plan モードでパイプライン駆動する運用(片方を Exit
# Plan して走らせ、終わったら次を Exit Plan する)では、先発の PR が merge
# された後も後発のエージェントはセッション開始時点の古いコードベースを見た
# ままプランを承認してしまう。worktree-fresh-base.sh は SessionStart 限定の
# pristine ff-only 追従しか持たず、長い Plan セッション中の drift はノーガード
# だった。この hook はその隙間を ExitPlanMode の承認点そのもので塞ぐ。
#
# 判定は 2 段:
#   1) 移動: worktree-fresh-base.sh と同じ pristine 5 条件(branch 非空・
#      branch != base・clean・ahead==0・behind>0)を満たすときだけ
#      `git merge --ff-only` で追従する。branch==base のときや非 pristine の
#      ときは動かさない(base ブランチを hook が無断で動かす事故を避ける、
#      worktree-fresh-base.sh と同じ方針)。
#   2) deny 判定: 移動の可否とは独立に、常に fetch し、
#      origin/<base> の進行分(from..to)が変更したファイルと、プラン本文が
#      参照しているファイル(フルパス部分一致 or basename 部分一致)が交差
#      するかを判定する。交差があれば deny — 移動できたかどうかに関わらず、
#      「エージェントの記憶にあるプラン」と「承認直前の実際のコード」が
#      食い違っている事実を毎回突きつける。pr-gate.sh の G_base は同種の
#      drift を advisory に留めているが、その根拠(block が rebase →
#      force-push ループを誘発する)はここでは成立しない — この gate が
#      要求するのは履歴改変ではなく「変更ファイルの再読 + 再 ExitPlanMode」
#      だけであり、次項の SHA 記録で有限回に収束する。
#
# 収束保証: deny した時点の origin/<base> の SHA をセッション単位の state
# file に記録する。次回判定はこの記録 SHA から新しい origin/<base> への
# 増分だけを見る(記録 SHA がまだ origin/<base> の祖先なら)。同じ SHA への
# 再 ExitPlanMode は無条件 allow — 非 pristine で ff できない場合でも、
# 「一度確認した差分」を毎回 deny し続けるスタベーションを避ける。
#
# 交差判定はファイル数に上限を設けない(表示件数だけ 50 件に丸める) — 内部
# 判定を打ち切ると、上限の外にある交差を見逃したまま ff し、次回は
# merge-base が進んで再検出の機会も失われる。ループ処理ではなく
# `grep -F -o -f` 1 回 + `awk` 1 回でパターン集合をまとめて照合し、
# ファイル数に対して線形の外部プロセス起動を避ける。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: ExitPlanMode)から
#                stdin JSON で呼ばれる
#   自己検査:   plan-fresh-gate.sh --selftest
#
# スキップ手段:
#   touch ~/.claude/plan-fresh-gate/skip   または   SKIP_PLAN_FRESH_GATE=1
#
# 縮退: git/jq 不在、fetch 失敗、origin/HEAD 未設定、プラン本文が取得できない
# 等はすべて fail-open(交差判定ができないので pass_through、advisory のみ)。
set -u

GATE_DIR="${CLAUDE_PLAN_FRESH_GATE_DIR:-$HOME/.claude/plan-fresh-gate}"
MAX_DENY_DISPLAY=50

self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

# ---------------------------------------------------------------------------
# hook 出力(plan-scope-gate.sh と同じ契約)
# ---------------------------------------------------------------------------

pass_through() { # $1=通知メッセージ(省略可、systemMessage として届く)
  local msg="${1:-}"
  [[ -n "$msg" ]] && jq -n --arg m "$msg" '{systemMessage: $m}'
  exit 0
}

deny_with() { # $1=EVENT $2=理由
  local event="$1" reason="$2"
  if [[ "$event" == "PermissionRequest" ]]; then
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PermissionRequest", decision: {behavior: "deny", message: $m}}}'
  else
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
  fi
  exit 0
}

# default branch を refs/remotes/origin/HEAD から読み、origin/ 接頭辞を剥がす。
# pr-gate.sh:default_branch() / worktree-fresh-base.sh:default_branch() と
# 同一式の複製 — 変更時は3箇所とも揃えること(docs/claude/worktree-fresh-base.md
# が明示的に許容している既存の重複パターン)。
default_branch() {
  local ref
  ref="$(git -C "$1" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)" || return 0
  printf '%s' "${ref#origin/}"
}

# state file: セッションごとに最後に deny した origin/<base> の SHA を記録する。
state_file() { # $1=session_id
  local sid="$1"
  sid="${sid//[^A-Za-z0-9._-]/_}"
  printf '%s/%s.denied_sha' "$GATE_DIR" "$sid"
}

# $1=changed_files(改行区切り、リポ相対パス) $2=plan_body
# 交差するファイルを改行区切りで出力(交差なしなら何も出さない)。
# grep -F -o -f 1回 + awk 1回でファイル数に対して線形の外部プロセス起動を避ける。
intersect_files() {
  local changed="$1" plan="$2" patterns matched
  [[ -n "$changed" ]] || return 0

  # パターン集合 = 各変更ファイルのフルパス + basename(重複除去)。
  patterns="$(awk -F/ '{print; print $NF}' <<< "$changed" | sort -u)"
  [[ -n "$patterns" ]] || return 0

  # プラン本文中に部分文字列として現れたパターンだけを抽出。
  matched="$(grep -F -o -f <(printf '%s\n' "$patterns") <<< "$plan" 2> /dev/null | sort -u)"
  [[ -n "$matched" ]] || return 0

  awk -F/ -v changed_list="$changed" '
    BEGIN {
      n = split(changed_list, arr, "\n")
    }
    { seen[$0] = 1 }
    END {
      for (i = 1; i <= n; i++) {
        path = arr[i]
        if (path == "") continue
        base = path
        sub(/.*\//, "", base)
        if (seen[path] || seen[base]) print path
      }
    }
  ' <<< "$matched"
}

run() {
  command -v git > /dev/null 2>&1 || exit 0
  command -v jq > /dev/null 2>&1 || exit 0

  if [[ -e "$GATE_DIR/skip" || "${SKIP_PLAN_FRESH_GATE:-0}" == "1" ]]; then
    exit 0
  fi

  local input event session_id project
  input="$(cat)"
  event="$(jq -r '.hook_event_name // "PreToolUse"' <<< "$input" 2> /dev/null)" || event="PreToolUse"
  session_id="$(jq -r '.session_id // "unknown"' <<< "$input" 2> /dev/null)" || session_id="unknown"
  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<< "$input" 2> /dev/null)}" || project=""
  [[ -n "$project" && -d "$project" ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

  local branch
  branch="$(git -C "$project" branch --show-current 2> /dev/null)" || branch=""
  [[ -n "$branch" ]] || exit 0 # detached HEAD / rebase 中は触らない

  local base
  base="$(default_branch "$project")"
  [[ -n "$base" ]] || exit 0
  local is_base_branch=0
  [[ "$branch" == "$base" ]] && is_base_branch=1

  # fetch は TTL なしで常に実行する。ExitPlanMode はセッションに稀なイベント
  # なので、SessionStart 系の 600s TTL(pr-gate.sh / worktree-fresh-base.sh)を
  # 尊重すると並行マージ直後の drift を見逃す。
  timeout 15 git -C "$project" fetch --quiet origin "$base" 2> /dev/null || true
  git -C "$project" rev-parse --verify -q "origin/$base" > /dev/null 2>&1 || exit 0

  local to_sha
  to_sha="$(git -C "$project" rev-parse "origin/$base" 2> /dev/null)" || exit 0

  mkdir -p "$GATE_DIR" 2> /dev/null
  chmod 700 "$GATE_DIR" 2> /dev/null || true
  local sfile from_sha
  sfile="$(state_file "$session_id")"
  if [[ -f "$sfile" ]]; then
    from_sha="$(cat "$sfile" 2> /dev/null)" || from_sha=""
    if [[ -n "$from_sha" ]] \
      && ! git -C "$project" merge-base --is-ancestor "$from_sha" "$to_sha" 2> /dev/null; then
      from_sha="" # 記録 SHA が origin/<base> の祖先でなくなっている(force-push 等) → 破棄
    fi
  fi
  if [[ -z "${from_sha:-}" ]]; then
    from_sha="$(git -C "$project" merge-base HEAD "origin/$base" 2> /dev/null)" || exit 0
  fi

  if [[ "$from_sha" == "$to_sha" ]]; then
    rm -f "$sfile" 2> /dev/null
    exit 0 # 追いつき済み(または前回の deny が既に確認済み) — 何も言わず allow
  fi

  local changed
  changed="$(git -C "$project" diff --name-only "$from_sha" "$to_sha" -- 2> /dev/null)" || exit 0
  if [[ -z "$changed" ]]; then
    exit 0 # 実質的な差分なし(マージコミット等)
  fi

  # --- 移動(pristine のときだけ) ---
  local did_ff=0 before after
  if ((is_base_branch == 0)) \
    && [[ -z "$(git -C "$project" status --porcelain 2> /dev/null)" ]]; then
    local ab ahead behind
    ab="$(git -C "$project" rev-list --left-right --count "HEAD...origin/$base" 2> /dev/null)" || ab=""
    ahead="${ab%%$'\t'*}"
    behind="${ab##*$'\t'}"
    if [[ "$ahead" == "0" && "$behind" =~ ^[0-9]+$ ]] && ((behind > 0)); then
      before="$(git -C "$project" rev-parse --short HEAD 2> /dev/null)" || before=""
      if git -C "$project" merge --ff-only --quiet "origin/$base" 2> /dev/null; then
        after="$(git -C "$project" rev-parse --short HEAD 2> /dev/null)" || after=""
        [[ -n "$before" && -n "$after" && "$before" != "$after" ]] && did_ff=1
      fi
    fi
  fi

  # --- プラン本文の取得: tool_input.plan → planFilePath → 最新 ~/.claude/plans/*.md
  # (plan-scope-gate.sh と同じ 3 段フォールバック)
  local plan_text plan_path plan_body=""
  plan_text="$(jq -r '.tool_input.plan // empty' <<< "$input" 2> /dev/null)"
  plan_path="$(jq -r '.tool_input.planFilePath // empty' <<< "$input" 2> /dev/null)"
  if [[ -n "$plan_text" ]]; then
    plan_body="$plan_text"
  elif [[ -n "$plan_path" && -f "$plan_path" ]]; then
    plan_body="$(cat "$plan_path")"
  else
    local latest_plan
    latest_plan="$(ls -t "$HOME/.claude/plans/"*.md 2> /dev/null | head -1)"
    if [[ -n "$latest_plan" ]]; then
      plan_body="$(cat "$latest_plan")"
    fi
  fi
  # プラン本文が取れなければ交差判定ができない — fail-open で advisory のみ。
  if [[ -z "$plan_body" ]]; then
    pass_through "[plan-fresh-gate] origin/${base} が進行していますが(${from_sha:0:7}..${to_sha:0:7})、プラン本文を取得できず交差判定をスキップしました。"
  fi

  local intersecting
  intersecting="$(intersect_files "$changed" "$plan_body")"

  if [[ -z "$intersecting" ]]; then
    local note
    if ((did_ff == 1)); then
      note="[plan-fresh-gate] origin/${base} へ ${before} -> ${after} まで fast-forward しました。プラン参照ファイルとの交差はありません。"
    else
      note="[plan-fresh-gate] origin/${base} が進行しています(${from_sha:0:7}..${to_sha:0:7})。プラン参照ファイルとの交差はありません。"
    fi
    pass_through "$note"
  fi

  # --- 交差あり: deny ---
  local -a lines=()
  lines+=("プラン作成後に origin/${base} が進行し、以下のプラン参照ファイルが変更されました。再読してプランが依然成立するか確認し、必要なら修正のうえ再度 ExitPlanMode してください。")
  lines+=("")
  local total shown
  total="$(wc -l <<< "$intersecting" | tr -d ' ')"
  shown="$(head -n "$MAX_DENY_DISPLAY" <<< "$intersecting")"
  while IFS= read -r f; do
    [[ -n "$f" ]] && lines+=("  - $f")
  done <<< "$shown"
  if ((total > MAX_DENY_DISPLAY)); then
    lines+=("  ...他 $((total - MAX_DENY_DISPLAY)) 件")
  fi
  lines+=("")
  lines+=("diffstat:")
  lines+=("$(git -C "$project" diff --stat "$from_sha" "$to_sha" -- $(printf '%s\n' "$shown") 2> /dev/null)")
  if ((did_ff == 1)); then
    lines+=("")
    lines+=("worktree は origin/${base} へ fast-forward 済みです(${before} -> ${after})。")
  else
    lines+=("")
    lines+=("worktree は動かしていません(作業中のコミットがある、または main 直上のため)。origin/${base} 側の内容は \`git show origin/${base}:<path>\` または \`git diff HEAD...origin/${base} -- <path>\` で確認してください。rebase は人間に依頼してください。")
  fi

  printf '%s' "$to_sha" > "$sfile" 2> /dev/null

  local msg
  msg="$(printf '%s\n' "${lines[@]}")"
  deny_with "$event" "$msg"
}

# --- サブコマンド: --selftest ---------------------------------------------------

if [[ "${1:-}" == "--selftest" ]]; then
  self="$(self_path)"
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  export GIT_CONFIG_GLOBAL=/dev/null
  export GIT_CONFIG_SYSTEM=/dev/null
  export CLAUDE_PLAN_FRESH_GATE_DIR="$dir/state"

  check() { # check <名前> <期待> <実際>
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected [$2], got [$3])" >&2
      fail=1
    fi
  }
  check_grep() { # check_grep <名前> <パターン> <対象文字列>
    if grep -qF -- "$2" <<< "$3"; then
      echo "ok   $1"
    else
      echo "FAIL $1 (pattern [$2] not found in [$3])" >&2
      fail=1
    fi
  }
  check_not_grep() {
    if grep -qF -- "$2" <<< "$3"; then
      echo "FAIL $1 (pattern [$2] unexpectedly found)" >&2
      fail=1
    else
      echo "ok   $1"
    fi
  }
  is_deny() { # $1=hook 出力json
    jq -e '.hookSpecificOutput.permissionDecision == "deny"' <<< "$1" > /dev/null 2>&1
  }

  new_repo_pair() { # $1=name $2=touch_file_in_ahead_commit(省略可)
    local name="$1" extra_file="${2:-}" upstream worktree
    upstream="$dir/$name-upstream"
    worktree="$dir/$name-worktree"
    git init -q -b main "$upstream"
    git -C "$upstream" -c user.email=t@example.com -c user.name=t \
      commit --allow-empty -q -m base
    git clone -q "$upstream" "$worktree"
    git -C "$worktree" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
    git -C "$worktree" checkout -q -b work
    if [[ -n "$extra_file" ]]; then
      mkdir -p "$(dirname "$upstream/$extra_file")"
      echo x > "$upstream/$extra_file"
      git -C "$upstream" add "$extra_file"
    fi
    git -C "$upstream" -c user.email=t@example.com -c user.name=t \
      commit --allow-empty -q -m ahead1
    printf '%s' "$worktree"
  }

  hookinput() { # $1=cwd $2=session_id $3=plan_text
    jq -n --arg c "$1" --arg s "$2" --arg p "$3" \
      '{cwd: $c, session_id: $s, hook_event_name: "PreToolUse", tool_input: {plan: $p}}'
  }

  echo "pristine + behind + 交差あり: ff + deny + state 記録:"
  wt="$(new_repo_pair case1 config/foo.nix)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid1 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && echo "ok   case1: deny" || {
    echo "FAIL case1: deny じゃない ($out)" >&2
    fail=1
  }
  upstream_head="$(git -C "$dir/case1-upstream" rev-parse HEAD)"
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "case1: ff された" "$upstream_head" "$after_head"
  [[ -f "$CLAUDE_PLAN_FRESH_GATE_DIR/sid1.denied_sha" ]] && echo "ok   case1: state 記録あり" || {
    echo "FAIL case1: state ファイルが無い" >&2
    fail=1
  }

  echo "pristine + behind + 交差なし: ff + allow:"
  wt="$(new_repo_pair case2 config/foo.nix)"
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid2 '無関係な docs/bar.md を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && {
    echo "FAIL case2: deny された (不要)" >&2
    fail=1
  }
  after_head="$(git -C "$wt" rev-parse HEAD)"
  upstream_head="$(git -C "$dir/case2-upstream" rev-parse HEAD)"
  check "case2: ff された" "$upstream_head" "$after_head"
  [[ "$before_head" != "$after_head" ]] && echo "ok   case2: HEAD が動いた" || {
    echo "FAIL case2: HEAD が動いていない" >&2
    fail=1
  }

  echo "dirty + behind + 交差あり: 未ff + deny(付記あり):"
  wt="$(new_repo_pair case3 config/foo.nix)"
  echo dirty > "$wt/untracked.txt"
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid3 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && echo "ok   case3: deny" || {
    echo "FAIL case3: deny じゃない ($out)" >&2
    fail=1
  }
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "case3: HEAD 不変(未 ff)" "$before_head" "$after_head"
  reason="$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<< "$out")"
  check_grep "case3: rebase は人間に依頼、の付記あり" "rebase は人間に依頼" "$reason"

  echo "deny 後・同一 base SHA: allow(state 収束):"
  wt="$(new_repo_pair case4 config/foo.nix)"
  echo dirty > "$wt/untracked.txt"
  out1="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid4 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out1" || {
    echo "FAIL case4: 1回目が deny でない" >&2
    fail=1
  }
  out2="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid4 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out2" && {
    echo "FAIL case4: 2回目も deny された(収束していない)" >&2
    fail=1
  } || echo "ok   case4: 2回目は allow"

  echo "deny 後・base さらに進行し新規差分が交差: 増分のみで再 deny:"
  wt="$(new_repo_pair case5 config/foo.nix)"
  echo dirty > "$wt/untracked.txt"
  out1="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid5 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out1" || {
    echo "FAIL case5: 1回目が deny でない" >&2
    fail=1
  }
  mkdir -p "$dir/case5-upstream/config"
  echo y > "$dir/case5-upstream/config/bar.nix"
  git -C "$dir/case5-upstream" add config/bar.nix
  git -C "$dir/case5-upstream" -c user.email=t@example.com -c user.name=t commit -q -m ahead2
  out2="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid5 'config/foo.nix と config/bar.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out2" && echo "ok   case5: 増分にも交差があり再 deny" || {
    echo "FAIL case5: 再 deny されなかった ($out2)" >&2
    fail=1
  }
  reason2="$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<< "$out2")"
  check_not_grep "case5: 確認済みの foo.nix は再掲されない" "foo.nix" "$reason2"
  check_grep "case5: 新規差分の bar.nix は挙げられる" "bar.nix" "$reason2"

  echo "behind==0(すでに最新)は allow:"
  wt="$(new_repo_pair case6 config/foo.nix)"
  git -C "$wt" fetch --quiet origin main
  git -C "$wt" merge --ff-only --quiet origin/main
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid6 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && {
    echo "FAIL case6: deny された(不要)" >&2
    fail=1
  } || echo "ok   case6: allow"

  echo "branch==base + 交差あり: 移動なしで deny:"
  wt="$(new_repo_pair case7 config/foo.nix)"
  git -C "$wt" checkout -q main
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid7 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && echo "ok   case7: deny" || {
    echo "FAIL case7: deny じゃない ($out)" >&2
    fail=1
  }
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "case7: HEAD 不変(base 自身は動かさない)" "$before_head" "$after_head"

  echo "basename のみ一致: 交差扱い:"
  wt="$(new_repo_pair case8 deep/nested/path/unique-name.txt)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid8 'unique-name.txt を直す計画(パスは省略)')" 2> "$dir/err")" || true
  is_deny "$out" && echo "ok   case8: basename 一致で deny" || {
    echo "FAIL case8: deny じゃない ($out)" >&2
    fail=1
  }

  echo "origin/HEAD 未設定は fail-open:"
  wt="$(new_repo_pair case9 config/foo.nix)"
  git -C "$wt" symbolic-ref -d refs/remotes/origin/HEAD
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid9 'config/foo.nix を編集する計画')" 2> "$dir/err")" || true
  check "case9: 出力なし" "" "$out"
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "case9: HEAD 不変" "$before_head" "$after_head"

  echo "変更200件超・交差ファイルが末尾側: 上限に関わらず deny(判定に上限がないことの回帰):"
  wt="$(new_repo_pair case10)"
  upstream="$dir/case10-upstream"
  i=1
  while ((i <= 250)); do
    printf 'x' > "$upstream/file_$(printf '%04d' "$i").txt"
    i=$((i + 1))
  done
  mkdir -p "$upstream/config"
  echo needle > "$upstream/config/needle.nix"
  git -C "$upstream" add -A
  git -C "$upstream" -c user.email=t@example.com -c user.name=t commit -q -m manyfiles
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<< "$(hookinput "$wt" sid10 'config/needle.nix を編集する計画')" 2> "$dir/err")" || true
  is_deny "$out" && echo "ok   case10: 上限外の交差も検出して deny" || {
    echo "FAIL case10: deny されなかった(上限で見逃した) ($out)" >&2
    fail=1
  }

  exit "$fail"
fi

run
