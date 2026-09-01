#!/usr/bin/env bash
# worktree-fresh-base.sh — pristine な worktree を SessionStart で
# origin/<base> へ黙って fast-forward する。
#
# 設計と根拠: docs/claude/worktree-fresh-base.md
#
# herdr の Workspace Fork は親チェックアウトの HEAD を fetch なしで使うため、
# 新しい worktree が古い base から生まれることがある(親を長時間開いたまま
# 何度も fork すると顕著)。既存の pr-gate.sh はこれを advisory(base 追従の
# 警告)で伝えるだけで、ブランチそのものを動かすことはしない — 履行歴を
# 持つブランチを無断で動かすのは事故なので、これは意図した非対称である。
#
# この hook が動かしてよいのは「まだ何も積んでいない、正真正銘そのままの
# worktree」だけ: 作業ツリーがクリーン かつ ahead==0(自分のコミットが 1 つも
# 無い)かつ behind>0(origin より遅れている)。この条件下でだけ
# `git merge --ff-only` を実行する — ff-only は前提条件を原子的に再強制する
# (fetch とチェックの間に何か積まれても黒歴史を作らず黙って失敗する)ので、
# `reset --hard` のような破壊的操作は使わない。
#
# 順序について: Claude Code は同一イベントの hook を並列実行するため、
# 「この hook → pr-gate.sh SessionStart」の逐次実行は保証されない
# (home/modules/claude.nix の plan-view の項に同じ注記がある)。レースは
# 許容する — 動かした場合だけ additionalContext でその旨を報告し、pr-gate の
# base 追従表示が古い状態を見ている可能性を明示する。
#
# 使い方:
#   hook として: settings.json の SessionStart から stdin JSON で呼ばれる
#   自己検査:   worktree-fresh-base.sh --selftest
#
# 縮退: 全ての失敗経路は fail-open(黙って exit 0)。動かせなかった理由を
# ユーザーに問い合わせることはしない — advisory は既存 pr-gate.sh に任せる。
set -euo pipefail

FETCH_TTL="${WORKTREE_FRESH_BASE_FETCH_TTL:-600}"

# 自身の絶対パス。symlink は辿らない — pr-gate.sh / issue-index.sh と同じ前例。
self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

git_common_dir() {
  git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null
}

# default branch を refs/remotes/origin/HEAD から読み、origin/ 接頭辞を剥がす。
# pr-gate.sh:default_branch() と同一式の複製 — 変更時は両方揃えること
# (剥がし忘れると "origin/origin/<base>" という存在しない ref を参照して
# ff 判定が常に失敗する)。
default_branch() {
  local ref
  ref="$(git -C "$1" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)" || return 0
  printf '%s' "${ref#origin/}"
}

run() {
  command -v git >/dev/null 2>&1 || exit 0
  command -v jq >/dev/null 2>&1 || exit 0

  local input project
  input="$(cat)"
  project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input" 2>/dev/null)}" || project=""
  [[ -n "$project" && -d "$project" ]] || exit 0
  git -C "$project" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0

  local branch
  branch="$(git -C "$project" branch --show-current 2>/dev/null)" || branch=""
  [[ -n "$branch" ]] || exit 0 # detached HEAD / rebase 中は触らない

  local base
  base="$(default_branch "$project")"
  [[ -n "$base" ]] || exit 0
  [[ "$branch" != "$base" ]] || exit 0 # 自分自身が base なら FF する意味がない

  [[ -z "$(git -C "$project" status --porcelain 2>/dev/null)" ]] || exit 0

  local common_dir
  common_dir="$(git_common_dir "$project")" || exit 0

  local do_fetch=1
  if [[ -f "$common_dir/FETCH_HEAD" ]]; then
    local mtime now
    mtime="$(stat -c %Y "$common_dir/FETCH_HEAD" 2>/dev/null)" || mtime=0
    now="$(date +%s)"
    ((now - mtime < FETCH_TTL)) && do_fetch=0
  fi
  if [[ "$do_fetch" == 1 ]]; then
    timeout 15 git -C "$project" fetch --quiet origin "$base" 2>/dev/null || exit 0
  fi
  git -C "$project" rev-parse --verify -q "origin/$base" >/dev/null 2>&1 || exit 0

  local ab ahead behind
  ab="$(git -C "$project" rev-list --left-right --count "HEAD...origin/$base" 2>/dev/null)" || exit 0
  ahead="${ab%%$'\t'*}"
  behind="${ab##*$'\t'}"
  [[ "$ahead" == "0" ]] || exit 0
  [[ "$behind" =~ ^[0-9]+$ ]] || exit 0
  ((behind > 0)) || exit 0

  local before after
  before="$(git -C "$project" rev-parse --short HEAD 2>/dev/null)" || before=""
  git -C "$project" merge --ff-only --quiet "origin/$base" 2>/dev/null || exit 0
  after="$(git -C "$project" rev-parse --short HEAD 2>/dev/null)" || after=""
  [[ -n "$before" && -n "$after" && "$before" != "$after" ]] || exit 0

  jq -n --arg ctx "[worktree-fresh-base] pristine worktree を origin/${base} へ ${behind} コミット fast-forward しました(${before} -> ${after})。他 hook の SessionStart advisory(pr-gate の base 追従など)は並列実行の都合でこの更新前の状態を見ている場合があります。" \
    '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
  exit 0
}

# --- サブコマンド: --selftest ---------------------------------------------------

if [[ "${1:-}" == "--selftest" ]]; then
  self="$(self_path)"
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  # #56/#59 の教訓: ホストの user/global git 設定(pre-commit hook・署名等)から
  # 隔離しないと selftest 自体がその場のマシン状態に汚染される。
  export GIT_CONFIG_GLOBAL=/dev/null
  export GIT_CONFIG_SYSTEM=/dev/null
  export WORKTREE_FRESH_BASE_FETCH_TTL=600

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

  # --- upstream + pristine worktree のペアを作る ---
  # upstream 側で 1 コミット進めてから worktree を切る(behind>0 の初期状態)。
  new_repo_pair() {
    local name="$1" upstream worktree
    upstream="$dir/$name-upstream"
    worktree="$dir/$name-worktree"
    git init -q -b main "$upstream"
    git -C "$upstream" -c user.email=t@example.com -c user.name=t \
      commit --allow-empty -q -m base
    git clone -q "$upstream" "$worktree"
    git -C "$worktree" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
    git -C "$worktree" checkout -q -b work
    git -C "$upstream" -c user.email=t@example.com -c user.name=t \
      commit --allow-empty -q -m ahead1
    printf '%s' "$worktree"
  }

  hookinput() { printf '{"cwd":"%s"}' "$1"; }

  echo "FF 成功(pristine + behind>0):"
  wt="$(new_repo_pair pristine)"
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  upstream_head="$(git -C "$dir/pristine-upstream" rev-parse HEAD)"
  check "FF 成功: HEAD が upstream に一致" "$upstream_head" "$after_head"
  check_grep "FF 成功: additionalContext が出る" "fast-forward" "$out"
  [[ "$before_head" != "$after_head" ]] && echo "ok   FF 成功: HEAD が動いた" || {
    echo "FAIL FF 成功: HEAD が動いていない" >&2
    fail=1
  }

  echo "dirty な worktree は無視:"
  wt="$(new_repo_pair dirty)"
  echo x >"$wt/untracked.txt"
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "dirty: HEAD 不変" "$before_head" "$after_head"
  check "dirty: 出力なし" "" "$out"

  echo "ahead>0(自分のコミットあり)は無視:"
  wt="$(new_repo_pair ahead)"
  git -C "$wt" -c user.email=t@example.com -c user.name=t \
    commit --allow-empty -q -m own-commit
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "ahead>0: HEAD 不変" "$before_head" "$after_head"
  check "ahead>0: 出力なし" "" "$out"

  echo "behind==0(すでに最新)は無視:"
  wt="$(new_repo_pair current)"
  git -C "$wt" fetch --quiet origin main
  git -C "$wt" merge --ff-only --quiet origin/main
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "behind==0: HEAD 不変" "$before_head" "$after_head"
  check "behind==0: 出力なし" "" "$out"

  echo "detached HEAD は無視:"
  wt="$(new_repo_pair detached)"
  git -C "$wt" checkout -q --detach HEAD
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "detached: HEAD 不変" "$before_head" "$after_head"
  check "detached: 出力なし" "" "$out"

  echo "origin/HEAD 未設定は無視:"
  wt="$(new_repo_pair noorigin)"
  git -C "$wt" symbolic-ref -d refs/remotes/origin/HEAD
  before_head="$(git -C "$wt" rev-parse HEAD)"
  out="$(CLAUDE_PROJECT_DIR="$wt" bash "$self" <<<"$(hookinput "$wt")" 2>"$dir/err")" || true
  after_head="$(git -C "$wt" rev-parse HEAD)"
  check "origin/HEAD 未設定: HEAD 不変" "$before_head" "$after_head"
  check "origin/HEAD 未設定: 出力なし" "" "$out"

  exit "$fail"
fi

run
