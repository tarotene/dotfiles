#!/usr/bin/env bash
# wrapup-stop-gate.sh — スコープ外の気づき(wrap-up inbox)を Issue 化させる Stop hook。
#
# 設計と根拠: docs/claude/wrapup-inbox.md(このリポジトリ内)
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
#   (slug は原則リポジトリ単位: `git remote get-url origin` を正規化した
#   <host>-<owner>-<repo>。remote なし・git repo 外はプロジェクト絶対パスの
#   '/' '.' → '-' 置換にフォールバックする。§ slug のリポジトリ単位化と
#   自己修復マージ、docs/claude/wrapup-inbox.md)
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
#   inbox パス解決: wrapup-stop-gate.sh --inbox-path <project-dir>
#                   (repo_slug 解決後の inbox パスを stdout に出すだけ。副作用なし)
#   自己修復移行:  wrapup-stop-gate.sh --migrate <project-dir>
#                   (旧・絶対パス slug の inbox が残っていれば新 slug へ
#                   append-only マージし、全行存在を確認してから旧を削除する。
#                   常に exit 0。session-start hook と Stop hook 本体が毎回呼ぶ)
#   セッション境界の刻印: wrapup-stop-gate.sh --stamp-feedback-session <session_id>
#                   (auto memory の feedback-issue-discipline 検査(#328)が
#                   使う「セッション開始」時刻の基準点を touch するだけ。
#                   wrapup-session-start.sh が毎回呼ぶ)
#   自己検査:     wrapup-stop-gate.sh --selftest
#
# フィードバックの Issue 化検査(#328、共有 AGENTS.md「ユーザーからの
# フィードバックは不可視なローカルメモに閉じ込めない」節): wrap-up inbox と
# 独立に、Stop 時点で auto memory(~/.claude/projects/*/memory/*.md、
# frontmatter に `type: feedback`)のうち、今セッション中に更新され `#N`
# (Issue 番号)参照を持たないものを検出して同じゲートで促す。inbox が
# 空でもこちらだけで exit 2 になりうる(既存の inbox 早期 return より手前で
# 判定する)。「今セッション」の境界は SessionStart で touch する stamp
# ファイルの mtime(`find -newer`、GNU/BSD 両対応で epoch 文字列を扱わない)。
set -euo pipefail

state_root() {
  printf '%s/claude/wrapup' "${WRAPUP_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}"
}

# --- フィードバックの Issue 化検査(#328) ---------------------------------------

FEEDBACK_STAMP_DIR="${WRAPUP_FEEDBACK_STAMP_DIR:-$HOME/.claude/wrapup-stop-gate/feedback-session}"
FEEDBACK_MEMORY_ROOT="${WRAPUP_FEEDBACK_MEMORY_DIR:-$HOME/.claude/projects}"

# $1=session_id -> セッション境界の stamp ファイルパス(stack-base-guard.sh の
# state_file() と同型のサニタイズ)。
feedback_stamp_file() {
  local sid="${1//[^A-Za-z0-9._-]/_}"
  printf '%s/%s.stamp' "$FEEDBACK_STAMP_DIR" "$sid"
}

# $1=session_id -> stdout: 今セッション中に更新され `#<数字>` の Issue 参照を
# 持たない type: feedback の auto memory ファイルを 1 行 1 パスで列挙する
# (無ければ何も出さない、常に exit 0)。stamp が無い(SessionStart 未実行・
# --selftest 等)場合は判定不能として何も出さない — 誤検出よりも黙って何も
# しない方向に倒す(ADR-0005 と同じ fail-open)。
unlinked_feedback_memories() {
  local sid="$1" stamp
  stamp="$(feedback_stamp_file "$sid")"
  [[ -f "$stamp" ]] || return 0
  [[ -d "$FEEDBACK_MEMORY_ROOT" ]] || return 0
  while IFS= read -r -d '' f; do
    grep -qE '^[[:space:]]*type:[[:space:]]*feedback[[:space:]]*$' "$f" 2>/dev/null || continue
    grep -qE '#[0-9]+' "$f" 2>/dev/null && continue
    printf '%s\n' "$f"
  done < <(find "$FEEDBACK_MEMORY_ROOT" -type f -path '*/memory/*.md' -newer "$stamp" -print0 2>/dev/null)
}

# 絶対パス slug(remote なし・git repo 外のフォールバック、および旧 inbox の
# 位置計算に使う)。
path_slug() {
  printf '%s' "$1" | tr '/.' '--'
}

# remote URL → リポジトリ単位 slug。正規化仕様:
#   - 末尾 '/' と '.git' suffix を除去
#   - scheme(https:// ssh:// git://)と認証情報の user@ を除去
#   - scp 形式 host:owner/repo の ':' を '/' に正規化
#   - 全体を小文字化(GitHub の owner/repo は大文字小文字非区別)
#   - '/' '.' → '-'
# 例: https://github.com/tarotene/dotfiles.git / git@github.com:Tarotene/dotfiles
#     / ssh://git@github.com/tarotene/dotfiles / HTTPS://GitHub.com/tarotene/dotfiles/
#     → いずれも github-com-tarotene-dotfiles
normalize_remote_url() {
  local u="$1"
  u="${u%/}"
  u="${u%.git}"
  u="${u#*://}"
  u="${u#*@}"
  u="${u/:/\/}"
  printf '%s' "$u" | tr '[:upper:]' '[:lower:]' | tr '/.' '--'
}

# exit 0 = repo_slug を stdout に出せた。exit 非 0 = フォールバック対象
# (git 不在・repo 外・remote 未設定)。
repo_slug() {
  command -v git >/dev/null 2>&1 || return 1
  local url
  url="$(git -C "$1" config --get remote.origin.url 2>/dev/null)" || return 1
  [[ -n "$url" ]] || return 1
  normalize_remote_url "$url"
}

inbox_for() {
  local s
  if s="$(repo_slug "$1" 2>/dev/null)"; then
    printf '%s/%s.jsonl' "$(state_root)" "$s"
  else
    printf '%s/%s.jsonl' "$(state_root)" "$(path_slug "$1")"
  fi
}

# 旧(絶対パス slug)の inbox が残っていれば、新(repo_slug)inbox へ
# append-only でマージし、全行の存在を確認してから旧を削除する。
#
# 安全性の要点(このリポジトリの inbox 整合性モデルを壊さないため):
#   - 使用中になり得る <inbox>.lock は削除・再作成しない(#297)。旧
#     ファイルだけを消し、lock ファイル自体は orphan のまま残す —
#     unlink すると、その lock を既に open/flock 待ちしている別プロセス
#     との相互排他が壊れる(unlink 後は同名で新しい inode の lock が
#     作られるため、旧 inode を握ったままの保持者と競合しなくなる)。
#     orphan lock は空ファイルのままなので実害はない。二重 flock を
#     取るのは本関数だけで、全呼び出し箇所が「新 → 旧」の同一順序で
#     取得するためデッドロックしない。
#   - 空ファイル判定もロック取得後に行う(空判定直後の並行 --add を
#     消さないため)。
#   - 新ファイルへは append のみ(truncate/rewrite しない)。dedup は
#     行全体の完全一致(この機構の行同一性モデルそのもの。ts+title 一致
#     だと detail 違いの行を落とすため使わない)。
#   - 旧の全行が新に存在することを確認できたときだけ旧を削除。1 行でも
#     欠ければ残して return し、次回呼び出し(次セッション)で再試行する
#     (冪等)。
migrate_legacy_inbox() {
  local project="$1" new legacy
  new="$(inbox_for "$project")"
  legacy="$(state_root)/$(path_slug "$project").jsonl"
  [[ "$new" == "$legacy" ]] && return 0
  [[ -f "$legacy" ]] || return 0

  mkdir -p "$(state_root)"
  (
    flock 9
    (
      flock 8
      if [[ ! -s "$legacy" ]]; then
        rm -f "$legacy"
        exit 0
      fi
      touch "$new"
      grep -Fvxf "$new" "$legacy" >>"$new" 2>/dev/null || true
      local all_present=1 line
      while IFS= read -r line; do
        grep -Fxq -- "$line" "$new" || { all_present=0; break; }
      done <"$legacy"
      if [[ "$all_present" == 1 ]]; then
        rm -f "$legacy"
      fi
    ) 8>>"$legacy.lock"
  ) 9>>"$new.lock"
  return 0
}

# 自身の絶対パス。symlink は辿らない — deployed 環境では ~/.claude/hooks/ 配下が
# nix store への symlink であり、指示文には世代を跨いで安定な symlink 側を出したい。
self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

# --- サブコマンド: --inbox-path <project-dir> ----------------------------------
if [[ "${1:-}" == "--inbox-path" ]]; then
  project="${2:?usage: wrapup-stop-gate.sh --inbox-path <project-dir>}"
  inbox_for "$project"
  exit 0
fi

# --- サブコマンド: --migrate <project-dir> --------------------------------------
if [[ "${1:-}" == "--migrate" ]]; then
  project="${2:?usage: wrapup-stop-gate.sh --migrate <project-dir>}"
  migrate_legacy_inbox "$project"
  exit 0
fi

# --- サブコマンド: --stamp-feedback-session <session_id> ------------------------
# セッション開始時刻の基準点を touch するだけ(#328)。失敗しても致命的では
# ない(stamp が無ければ unlinked_feedback_memories() は判定不能として何も
# しない側に倒れる)。
if [[ "${1:-}" == "--stamp-feedback-session" ]]; then
  session_id="${2:?usage: wrapup-stop-gate.sh --stamp-feedback-session <session_id>}"
  mkdir -p "$FEEDBACK_STAMP_DIR" 2>/dev/null || exit 0
  chmod 700 "$FEEDBACK_STAMP_DIR" 2>/dev/null || true
  : >"$(feedback_stamp_file "$session_id")" 2>/dev/null || true
  exit 0
fi

# --- サブコマンド: --add <inbox> <json> ---------------------------------------
# 入力が pretty-print(複数行)であっても jq -c で 1 行に強制コンパクト化してから
# 書き込む。--mark-filed は行全体の完全一致で削除するため、非コンパクトな行が
# 紛れ込むと二度と自動削除できなくなる(#248)。
if [[ "${1:-}" == "--add" ]]; then
  inbox="${2:?usage: wrapup-stop-gate.sh --add <inbox> <json>}"
  line="${3:?usage: wrapup-stop-gate.sh --add <inbox> <json>}"
  compact="$(jq -ce . <<<"$line")" || {
    echo "wrapup-stop-gate: --add: 不正な JSON です" >&2
    exit 64
  }
  mkdir -p "$(dirname "$inbox")"
  (
    flock 9
    printf '%s\n' "$compact" >>"$inbox"
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
# #297 の硬化: (a) 削除対象が無ければ rewrite 自体をしない(no-op 時の
# mtime/パーミッション変化と無意味な mv を避ける)。(b) 削除した行は
# "$inbox.filed.jsonl" に tombstone として退避してから mv する — 起票
# 済み・重複スキップいずれの削除も、無検証の一撃消去にしない(データ
# 喪失を不可逆にしない)。(c) mktemp の既定 0600 のまま mv すると inbox
# のパーミッションが変わってしまうため、mv 前に元ファイルへ揃える。
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
    if cmp -s "$tmp" "$inbox"; then
      rm -f "$tmp"
    else
      printf '%s\n' "$line" >>"$inbox.filed.jsonl"
      chmod --reference="$inbox" "$tmp" 2>/dev/null || true
      mv "$tmp" "$inbox"
    fi
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
  # #328: 実 $HOME を汚さないよう、選択したテストだけでなく selftest 全体を
  # 通して隔離する(FEEDBACK_STAMP_DIR/FEEDBACK_MEMORY_ROOT はスクリプト先頭で
  # 一度だけ評価されるグローバル変数なので、--selftest ブロック内で export
  # しても手遅れ — この時点(トップレベル実行前)で export する)。
  export WRAPUP_FEEDBACK_STAMP_DIR="$dir/feedback-stamp"
  export WRAPUP_FEEDBACK_MEMORY_DIR="$dir/feedback-memory"
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

  # #248: pretty-print(複数行)入力も 1 行にコンパクト化して書き込まれる
  # (以降の行数アサーションを崩さないよう、専用の別 inbox で検証する)
  pretty_inbox="$dir/pretty.jsonl"
  pretty='{
  "ts": "2026-08-25T00:00:00+09:00",
  "title": "pretty",
  "detail": "c"
}'
  bash "$self" --add "$pretty_inbox" "$pretty"
  check "--add は pretty-print 入力を 1 行に圧縮する" 1 "$(wc -l <"$pretty_inbox")"
  check "圧縮後の行は改行を含まない" 0 "$(tail -n1 "$pretty_inbox" | jq -e . >/dev/null 2>&1; echo $?)"

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

  # --- repo_slug: URL 表記違いの正規化同値性 ---
  scp_repo="$dir/scp_repo"
  mkdir -p "$scp_repo"
  git -C "$scp_repo" init -q
  git -C "$scp_repo" remote add origin git@github.com:Example/Example.git
  ssh_repo="$dir/ssh_repo"
  mkdir -p "$ssh_repo"
  git -C "$ssh_repo" init -q
  git -C "$ssh_repo" remote add origin ssh://git@github.com/example/example
  check "scp 形式・大文字表記が同一 inbox に正規化される" 0 \
    "$([[ "$(inbox_for "$scp_repo")" == "$inbox" ]]; echo $?)"
  check "ssh:// 形式も同一 inbox に正規化される" 0 \
    "$([[ "$(inbox_for "$ssh_repo")" == "$inbox" ]]; echo $?)"

  # --- --inbox-path: remote なし・repo 外はプロジェクト絶対パス slug のまま ---
  check "--inbox-path は remote なしで絶対パス slug を返す" 0 \
    "$(diff <(bash "$self" --inbox-path "$norepo") <(printf '%s' "$(inbox_for "$norepo")") >/dev/null; echo $?)"
  check "--inbox-path は git repo 外で絶対パス slug を返す" 0 \
    "$(diff <(bash "$self" --inbox-path "$plain") <(printf '%s' "$(inbox_for "$plain")") >/dev/null; echo $?)"

  # --- --migrate: 旧(絶対パス slug)inbox の自己修復マージ ---
  mig_repo="$dir/mig_repo"
  mkdir -p "$mig_repo"
  git -C "$mig_repo" init -q
  git -C "$mig_repo" remote add origin https://github.com/example/migrated.git
  mig_new="$(inbox_for "$mig_repo")"
  mig_legacy="$(state_root)/$(path_slug "$mig_repo").jsonl"
  mig_l1='{"ts":"2026-08-25T00:00:00+09:00","title":"legacy-only","detail":"a"}'
  mig_l2='{"ts":"2026-08-25T00:00:00+09:00","title":"shared","detail":"b"}'
  mkdir -p "$(dirname "$mig_legacy")"
  printf '%s\n%s\n' "$mig_l1" "$mig_l2" >"$mig_legacy"
  bash "$self" --add "$mig_new" "$mig_l2" # 新側に同一内容 1 行を先に用意(dedup 対象)
  bash "$self" --migrate "$mig_repo"
  check "--migrate 後、新 inbox は旧の全行を(重複排除して)含む" 0 \
    "$(grep -Fxq "$mig_l1" "$mig_new" && grep -cFx "$mig_l2" "$mig_new" | grep -qx 1; echo $?)"
  check "--migrate 後、旧 inbox は消滅する" 1 "$([[ -f "$mig_legacy" ]]; echo $?)"
  # #297: 旧 lock は unlink しない(使用中の flock 保持者との相互排他を壊さないため)。
  check "--migrate 後も旧 lock は orphan のまま残る" 0 "$([[ -f "$mig_legacy.lock" ]]; echo $?)"
  before_new="$(cat "$mig_new")"
  bash "$self" --migrate "$mig_repo" # 2 回目は無変化(冪等)
  check "--migrate は冪等(2 回目は無変化)" 0 "$([[ "$before_new" == "$(cat "$mig_new")" ]]; echo $?)"

  # --- --migrate: 旧が 0 バイトなら掃除のみ ---
  empty_repo="$dir/empty_repo"
  mkdir -p "$empty_repo"
  git -C "$empty_repo" init -q
  git -C "$empty_repo" remote add origin https://github.com/example/emptylegacy.git
  empty_legacy="$(state_root)/$(path_slug "$empty_repo").jsonl"
  : >"$empty_legacy"
  bash "$self" --migrate "$empty_repo"
  check "--migrate は 0 バイトの旧 inbox を削除する" 1 "$([[ -f "$empty_legacy" ]]; echo $?)"

  # --- --mark-filed: 同一内容 2 行 + 別内容 1 行から対象 1 行だけ削除 ---
  chmod 640 "$inbox"
  bash "$self" --add "$inbox" "$line1" # inbox: line1, line2, line1
  filed_inbox="${inbox}.filed.jsonl"
  bash "$self" --mark-filed "$inbox" "$line1"
  check "--mark-filed は先頭一致 1 行だけ削除" 2 "$(wc -l <"$inbox")"
  check "--mark-filed 後も同一内容のもう 1 行は残る" 0 \
    "$(grep -cFx "$line1" "$inbox" | grep -qx 1; echo $?)"
  # #297: 削除した行は tombstone(<inbox>.filed.jsonl)に退避される
  check "--mark-filed は削除行を tombstone に退避する" 0 \
    "$(grep -cFx "$line1" "$filed_inbox" | grep -qx 1; echo $?)"
  # #297: mv 前に元ファイルのパーミッションを引き継ぐ(mktemp 既定の 0600 化を防ぐ)
  check "--mark-filed 後も inbox のパーミッションは維持される" 0 \
    "$([[ "$(stat -c%a "$inbox")" == "640" ]]; echo $?)"
  before="$(cat "$inbox")"
  before_filed_lines="$(wc -l <"$filed_inbox")"
  bash "$self" --mark-filed "$inbox" '{"ts":"x","title":"nomatch","detail":"x"}'
  check "--mark-filed は不一致行では無変更" 0 "$([[ "$before" == "$(cat "$inbox")" ]]; echo $?)"
  # #297: no-op(削除対象が無い)呼び出しは tombstone にも追記しない
  check "--mark-filed は no-op 時に tombstone を増やさない" 0 \
    "$([[ "$before_filed_lines" == "$(wc -l <"$filed_inbox")" ]]; echo $?)"

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

  # --- #328: feedback 型 auto memory の未起票検査 ---
  fb_repo="$dir/fb_repo"
  mkdir -p "$fb_repo"
  git -C "$fb_repo" init -q
  # $repo とは別の remote(= 別の inbox slug)にする — 同じ URL だと
  # $repo で --add した既存 2 行を共有し、inbox が意図せず非空になる。
  git -C "$fb_repo" remote add origin https://github.com/example/feedback-test.git
  fb_hookinput='{"cwd":"'"$fb_repo"'","session_id":"fbsid","stop_hook_active":false}'

  # WRAPUP_FEEDBACK_STAMP_DIR / WRAPUP_FEEDBACK_MEMORY_DIR は selftest 冒頭で
  # 既に export 済み(実 $HOME を汚さないための隔離、上部コメント参照)。
  mkdir -p "$WRAPUP_FEEDBACK_MEMORY_DIR/proj1/memory"

  # stamp 未設定・空 inbox は判定不能扱いで素通り
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$fb_repo" bash "$self" <<<"$fb_hookinput" 2>/dev/null || rc=$?
  check "feedback: stamp 未設定・空 inbox は素通り" 0 "$rc"

  # --stamp-feedback-session でスタンプを打ち、既存メモリより確実に古くなる
  # よう 1 時間前にバックデートする(sleep を避ける、plan-view.sh の
  # `touch -d '40 days ago'` と同じ型)
  bash "$self" --stamp-feedback-session "fbsid"
  # feedback_stamp_file() はスクリプト先頭で一度だけ評価される
  # $FEEDBACK_STAMP_DIR を参照するグローバル関数なので、この selftest
  # プロセス自身の中で呼んでも上の export 前の値を見てしまう。サブプロセス
  # (--stamp-feedback-session)は新しい環境を評価し直すので正しいパスに
  # 書くが、ドライバ側の照合には exported な env var から直接組み立てる。
  fb_stamp="$WRAPUP_FEEDBACK_STAMP_DIR/fbsid.stamp"
  check "--stamp-feedback-session はスタンプファイルを作る" 0 "$([[ -f "$fb_stamp" ]]; echo $?)"
  touch -d '1 hour ago' "$fb_stamp"

  # スタンプ後(=スタンプより新しい mtime)に #N 無しの type: feedback メモリを作る
  cat >"$WRAPUP_FEEDBACK_MEMORY_DIR/proj1/memory/unlinked.md" <<'MEM'
---
name: unlinked
description: test
metadata:
  type: feedback
---

本文に Issue 番号が無い。
MEM

  rc=0
  errfile2="$dir/stderr2.txt"
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$fb_repo" bash "$self" <<<"$fb_hookinput" 2>"$errfile2" || rc=$?
  check "feedback: #N 無しはゲート発動(空 inbox でも)" 2 "$rc"
  check "feedback: メッセージが feedback-memory を含む" 0 \
    "$(grep -q 'feedback-memory' "$errfile2"; echo $?)"

  # #N を書き足すと素通りに戻る
  cat >"$WRAPUP_FEEDBACK_MEMORY_DIR/proj1/memory/unlinked.md" <<'MEM'
---
name: unlinked
description: test
metadata:
  type: feedback
---

対応: #123 起票済み。
MEM
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$fb_repo" bash "$self" <<<"$fb_hookinput" 2>/dev/null || rc=$?
  check "feedback: #N ありは素通り" 0 "$rc"

  # type: feedback でなければ #N 無しでも対象外
  cat >"$WRAPUP_FEEDBACK_MEMORY_DIR/proj1/memory/other.md" <<'MEM'
---
name: other
description: test
metadata:
  type: project
---

feedback ではない。
MEM
  rc=0
  PATH="$stub_path" CLAUDE_PROJECT_DIR="$fb_repo" bash "$self" <<<"$fb_hookinput" 2>/dev/null || rc=$?
  check "feedback: type!=feedback は対象外" 0 "$rc"

  unset WRAPUP_FEEDBACK_STAMP_DIR WRAPUP_FEEDBACK_MEMORY_DIR

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- hook 本体(Stop) ----------------------------------------------------------
command -v jq >/dev/null 2>&1 || exit 0
input="$(cat)"

[[ "$(jq -r '.stop_hook_active // false' <<<"$input")" == "true" ]] && exit 0

project="${CLAUDE_PROJECT_DIR:-$(jq -r '.cwd // empty' <<<"$input")}"
[[ -n "$project" ]] || exit 0

session_id="$(jq -r '.session_id // "unknown"' <<<"$input" 2>/dev/null)" || session_id="unknown"

migrate_legacy_inbox "$project"
inbox="$(inbox_for "$project")"

# #328: inbox が空でも feedback 型 auto memory の未起票が見つかれば単独で
# ゲートしうる — この判定は inbox の早期 return より手前(両方とも空なら
# ここで抜ける)。
unlinked="$(unlinked_feedback_memories "$session_id")"

if [[ ! -s "$inbox" && -z "$unlinked" ]]; then
  exit 0
fi

command -v gh >/dev/null 2>&1 || exit 0
git -C "$project" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0
git -C "$project" remote -v 2>/dev/null | grep -q 'github\.' || exit 0

self="$(self_path)"
msg=""

if [[ -s "$inbox" ]]; then
  count="$(wc -l <"$inbox")"
  msg+="$(cat <<EOF
[wrapup-inbox] 未起票の気づきが ${count} 件残っています: ${inbox}
各行(JSONL: ts/title/detail)を、このプロジェクトのリポジトリに次の手順で起票してください:
  1. bash '${self}' --check-dup "<title>" を実行する。
     exit 1 なら同名の open Issue が既にある(重複)。exit 3 なら判定不能 —
     その行は今回スキップして inbox に残す。
  2. 重複でなければ gh issue create --title "<title>" --body "<本文>" で起票する。
     本文は detail を会話の文脈で補って書き、末尾に次の 1 行を付ける
     (inbox 由来を後から grep で絞るための出自フッターが、
     attribution-guard.sh が要求する生成元表示を兼ねる):
       「🤖 Filed from [Claude Code](https://claude.com/claude-code) wrap-up inbox」
  3. 起票に成功した行、または重複でスキップした行だけを
     bash '${self}' --mark-filed '${inbox}' '<その行そのまま>' で削除する。
     gh issue create に失敗した行には --mark-filed を呼ばず、inbox に残す(次ターンで再試行)。
inbox を直接編集してはいけません(必ず --add / --mark-filed 経由)。
EOF
)"
fi

if [[ -n "$unlinked" ]]; then
  [[ -n "$msg" ]] && msg+=$'\n\n'
  fcount="$(wc -l <<<"$unlinked")"
  msg+="$(cat <<EOF
[feedback-memory] 今セッション中に更新された type: feedback の auto memory が
${fcount} 件、Issue 番号(#N)の参照を持たずに残っています:
$(sed 's/^/  - /' <<<"$unlinked")
汎用的な作業方針フィードバック(プロジェクト固有でなく、センシティブでないもの)は
既定で GitHub Issue として起票し、起票したら該当メモリファイルの本文に #N を
追記してください(共有 AGENTS.md「ユーザーからのフィードバックは不可視な
ローカルメモに閉じ込めない」節、config/claude/CLAUDE.md「フィードバックの
Issue 化」節)。プロジェクト固有で汎用化できない、またはセキュリティ・個人情報
等センシティブな内容はこの限りではありません。
EOF
)"
fi

printf '%s\n' "$msg" >&2
exit 2
