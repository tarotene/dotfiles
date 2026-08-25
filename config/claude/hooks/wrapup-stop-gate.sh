#!/usr/bin/env bash
# wrapup-stop-gate.sh — スコープ外の気づき(wrap-up inbox)を Issue 化させる Stop hook。
#
# 設計と根拠: docs/wrapup-inbox.md(このリポジトリ内)
#
# SessionEnd での起票は decision control なし・時間予算・報告先なしの三重苦で成立
# しないため、収集と起票を分離する:
#
#   収集 : wrapup-session-start.sh が「気づきは inbox(JSONL)に追記せよ」と注入
#   起票 : この hook が inbox 非空なら exit 2 + stderr 指示でゲートし、
#          フルコンテキストを持つ本体 Claude に gh issue create させる
#
# inbox は作業ツリーを汚さないよう state 領域に置く:
#   ${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/<slug>.jsonl
#   (slug はプロジェクトパスの '/' '.' → '-' 置換)
#
# 1 行スキーマ: {"ts": "<ISO8601>", "title": "...", "detail": "..."}
# ts は一意でない(削除キーには使わない)。行の同一性は行全体の完全一致。
#
# inbox への書き込みは LLM に直接させず、この script のサブコマンド経由に限定する。
# 追記(--add)と削除(--mark-filed)は flock で排他し、並行セッションの
# 追記 vs tmp+mv 置換の競合(行の消失・復活)を防ぐ。
#
# 縮退(すべて黙って exit 0、ADR-0005 の binary-existence gating に倣う):
#   - jq 不在 / stop_hook_active(無限ループガード) / inbox 不在・空
#   - gh 不在 / git repo 外 / GitHub remote なし(remote URL の静的検査のみ。
#     hook 内でネットワークには出ない)
#
# 使い方:
#   hook として:  settings.json の Stop から stdin JSON で呼ばれる
#   追記:         wrapup-stop-gate.sh --add <inbox> '<json1行>'
#   重複判定:     wrapup-stop-gate.sh --check-dup "<title>"
#                   exit 0 = 重複なし / 1 = 同名 open Issue あり / 3 = 判定不能(gh 失敗)
#   起票済み削除: wrapup-stop-gate.sh --mark-filed <inbox> '<json1行>'
#                   (行全体の完全一致で先頭の 1 行だけ削除)
#   自己検査:     wrapup-stop-gate.sh --selftest
set -euo pipefail

state_root() {
  printf '%s/claude/wrapup' "${WRAPUP_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}"
}

slug() {
  printf '%s' "$1" | tr '/.' '--'
}

inbox_for() {
  printf '%s/%s.jsonl' "$(state_root)" "$(slug "$1")"
}

# 自身の絶対パス。symlink は辿らない — deployed 環境では ~/.claude/hooks/ 配下が
# nix store への symlink であり、指示文には世代を跨いで安定な symlink 側を出したい。
self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

# --- サブコマンド: --add <inbox> <json1行> ------------------------------------
if [[ "${1:-}" == "--add" ]]; then
  inbox="${2:?usage: wrapup-stop-gate.sh --add <inbox> <json>}"
  line="${3:?usage: wrapup-stop-gate.sh --add <inbox> <json>}"
  jq -e . >/dev/null <<<"$line" || {
    echo "wrapup-stop-gate: --add: 不正な JSON です" >&2
    exit 64
  }
  mkdir -p "$(dirname "$inbox")"
  (
    flock 9
    printf '%s\n' "$line" >>"$inbox"
  ) 9>>"$inbox.lock"
  exit 0
fi

# --- サブコマンド: --check-dup <title> ----------------------------------------
if [[ "${1:-}" == "--check-dup" ]]; then
  title="${2:?usage: wrapup-stop-gate.sh --check-dup <title>}"
  if ! json="$(gh issue list --state open --search "in:title $title" --json title 2>/dev/null)"; then
    exit 3
  fi
  if jq -e --arg t "$title" 'any(.[]; .title == $t)' >/dev/null <<<"$json"; then
    exit 1
  fi
  exit 0
fi

# --- サブコマンド: --mark-filed <inbox> <json1行> ------------------------------
if [[ "${1:-}" == "--mark-filed" ]]; then
  inbox="${2:?usage: wrapup-stop-gate.sh --mark-filed <inbox> <json>}"
  line="${3:?usage: wrapup-stop-gate.sh --mark-filed <inbox> <json>}"
  [[ -f "$inbox" ]] || exit 0
  (
    flock 9
    tmp="$(mktemp "$inbox.XXXXXX")"
    TARGET="$line" awk '
      BEGIN { done = 0 }
      !done && $0 == ENVIRON["TARGET"] { done = 1; next }
      { print }
    ' "$inbox" >"$tmp"
    mv "$tmp" "$inbox"
  ) 9>>"$inbox.lock"
  exit 0
fi

# --- サブコマンド: --selftest --------------------------------------------------
if [[ "${1:-}" == "--selftest" ]]; then
  self="$(self_path)"
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  check() { # check <名前> <期待exit> <実exit>
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected exit $2, got $3)" >&2
      fail=1
    fi
  }

  # 実験環境: github remote 付きの git repo と、それに対応する inbox
  export WRAPUP_STATE_DIR="$dir/state"
  repo="$dir/repo"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" remote add origin https://github.com/example/example.git
  export CLAUDE_PROJECT_DIR="$repo"
  inbox="$(inbox_for "$repo")"

  # gh スタブ: WRAPUP_STUB_DUP=1 なら同名 Issue ヒットを返す
  mkdir -p "$dir/bin"
  cat >"$dir/bin/gh" <<'STUB'
#!/usr/bin/env bash
if [[ "${WRAPUP_STUB_DUP:-0}" == "1" ]]; then
  echo '[{"title":"dup title"}]'
else
  echo '[]'
fi
STUB
  chmod +x "$dir/bin/gh"
  stub_path="$dir/bin:$PATH"

  hookinput='{"cwd":"'"$repo"'","stop_hook_active":false}'

  # --- ゲート判定 ---
  rc=0
  PATH="$stub_path" bash "$self" <<<'{"cwd":"","stop_hook_active":true}' 2>/dev/null || rc=$?
  check "stop_hook_active で素通り" 0 "$rc"

  rc=0
  PATH="$stub_path" bash "$self" <<<"$hookinput" 2>/dev/null || rc=$?
  check "inbox 不在で素通り" 0 "$rc"

  # --add: 親ディレクトリ不在からの追記と行数
  line1='{"ts":"2026-08-25T00:00:00+09:00","title":"dup title","detail":"a"}'
  line2='{"ts":"2026-08-25T00:00:00+09:00","title":"other","detail":"b"}'
  bash "$self" --add "$inbox" "$line1"
  bash "$self" --add "$inbox" "$line2"
  check "--add で 2 行になる" 2 "$(wc -l <"$inbox")"
  rc=0
  bash "$self" --add "$inbox" 'not-json' 2>/dev/null || rc=$?
  check "--add は不正 JSON を拒否" 64 "$rc"

  # 非空 inbox + 条件充足 → exit 2 + stderr 非空
  rc=0
  errfile="$dir/stderr.txt"
  PATH="$stub_path" bash "$self" <<<"$hookinput" 2>"$errfile" || rc=$?
  check "非空 inbox でゲート発動" 2 "$rc"
  check "ゲートは stderr に指示を出す" 0 "$([[ -s "$errfile" ]]; echo $?)"

  # gh 不在 → 素通り(gh だけを欠いた最小 PATH を合成する)
  mkdir -p "$dir/nogh"
  for c in jq git grep wc tr dirname basename cat; do
    ln -s "$(command -v "$c")" "$dir/nogh/$c"
  done
  rc=0
  PATH="$dir/nogh" "$BASH" "$self" <<<"$hookinput" 2>/dev/null || rc=$?
  check "gh 不在で素通り" 0 "$rc"

  # GitHub remote なし → 素通り
  norepo="$dir/norepo"
  mkdir -p "$norepo"
  git -C "$norepo" init -q
  bash "$self" --add "$(inbox_for "$norepo")" "$line1"
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$norepo" \
    bash "$self" <<<'{"cwd":"'"$norepo"'","stop_hook_active":false}' 2>/dev/null || rc=$?
  check "GitHub remote なしで素通り" 0 "$rc"

  # git repo 外 → 素通り
  plain="$dir/plain"
  mkdir -p "$plain"
  bash "$self" --add "$(inbox_for "$plain")" "$line1"
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$plain" \
    bash "$self" <<<'{"cwd":"'"$plain"'","stop_hook_active":false}' 2>/dev/null || rc=$?
  check "git repo 外で素通り" 0 "$rc"

  # --- --mark-filed: 同一内容 2 行 + 別内容 1 行から対象 1 行だけ削除 ---
  bash "$self" --add "$inbox" "$line1" # inbox: line1, line2, line1
  bash "$self" --mark-filed "$inbox" "$line1"
  check "--mark-filed は先頭一致 1 行だけ削除" 2 "$(wc -l <"$inbox")"
  check "--mark-filed 後も同一内容のもう 1 行は残る" 0 \
    "$(grep -cFx "$line1" "$inbox" | grep -qx 1; echo $?)"
  before="$(cat "$inbox")"
  bash "$self" --mark-filed "$inbox" '{"ts":"x","title":"nomatch","detail":"x"}'
  check "--mark-filed は不一致行では無変更" 0 "$([[ "$before" == "$(cat "$inbox")" ]]; echo $?)"

  # --- --check-dup: スタブ gh でヒット/非ヒット ---
  rc=0
  PATH="$stub_path" WRAPUP_STUB_DUP=1 bash "$self" --check-dup "dup title" || rc=$?
  check "--check-dup はヒット時 exit 1" 1 "$rc"
  rc=0
  PATH="$stub_path" bash "$self" --check-dup "dup title" || rc=$?
  check "--check-dup は非ヒット時 exit 0" 0 "$rc"

  # --- SessionStart hook: 注入 JSON と未処理件数 ---
  ss="$(dirname "$self")/wrapup-session-start.sh"
  out="$(CLAUDE_PROJECT_DIR="$repo" bash "$ss" <<<"$hookinput")"
  check "session-start は additionalContext を返す" 0 \
    "$(jq -e '.hookSpecificOutput.additionalContext | length > 0' >/dev/null <<<"$out"; echo $?)"
  check "session-start は未処理件数を報告する" 0 \
    "$(jq -r '.hookSpecificOutput.additionalContext' <<<"$out" | grep -q "未処理 2 件"; echo $?)"

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- hook 本体(Stop) ----------------------------------------------------------
command -v jq >/dev/null 2>&1 || exit 0
input="$(cat)"

[[ "$(jq -r '.stop_hook_active // false' <<<"$input")" == "true" ]] && exit 0

project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input")}"
[[ -n "$project" ]] || exit 0

inbox="$(inbox_for "$project")"
[[ -s "$inbox" ]] || exit 0

command -v gh >/dev/null 2>&1 || exit 0
git -C "$project" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0
git -C "$project" remote -v 2>/dev/null | grep -q 'github\.' || exit 0

count="$(wc -l <"$inbox")"
self="$(self_path)"

cat >&2 <<EOF
[wrapup-inbox] 未起票の気づきが ${count} 件残っています: ${inbox}
各行(JSONL: ts/title/detail)を、このプロジェクトのリポジトリに次の手順で起票してください:
  1. bash '${self}' --check-dup "<title>" を実行する。
     exit 1 なら同名の open Issue が既にある(重複)。exit 3 なら判定不能 —
     その行は今回スキップして inbox に残す。
  2. 重複でなければ gh issue create --title "<title>" --body "<本文>" で起票する。
     本文は detail を会話の文脈で補って書き、末尾に出自フッター
     「🤖 Filed from Claude Code wrap-up inbox」を付ける。
  3. 起票に成功した行、または重複でスキップした行だけを
     bash '${self}' --mark-filed '${inbox}' '<その行そのまま>' で削除する。
     gh issue create に失敗した行には --mark-filed を呼ばず、inbox に残す(次ターンで再試行)。
inbox を直接編集してはいけません(必ず --add / --mark-filed 経由)。
EOF
exit 2
