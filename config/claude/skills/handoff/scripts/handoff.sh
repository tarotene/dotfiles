#!/usr/bin/env bash
# handoff.sh — handoff skill(中断ハンドオフ)が使う決定論的サブコマンド群。
#
# 設計と根拠: docs/claude/handoff.md
#
# 誤りやすい GitHub API 操作(ラベルの存在確認・新設、Issue dependencies の
# blocked_by 設定)を LLM の自由な gh 呼び出しに任せず、ここに寄せて selftest
# で回帰する(wrapup-stop-gate.sh の `--add`/`--mark-filed` と同じ方針)。
#
# サブコマンド:
#   labels-missing <owner/repo>
#     handoff:human / handoff:ai のうち、そのリポジトリに存在しないラベル名を
#     改行区切りで stdout に出す(両方存在すれば何も出さず exit 0)。
#   create-labels <owner/repo> <label>...
#     指定ラベル(handoff:human / handoff:ai のみ受理)を固定の色・説明で
#     作成する(--force なので既存でも上書きして冪等)。
#   block <owner/repo> <blocked番号> <blocker番号>
#     blocked 側の Issue に blocker 側を blocked_by として追加する。GitHub の
#     issue dependencies API は issue **id**(番号ではない)を要求するため、
#     ここで番号→id の解決も行う(取り違え防止 — docs/claude/handoff.md 参照)。
#
# 使い方:
#   skill から: bash config/claude/skills/handoff/scripts/handoff.sh <サブコマンド> ...
#   自己検査:   handoff.sh --selftest
set -euo pipefail

self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

label_meta() { # label_meta <label名> ; 色と説明を1行ずつ出す(未知なら非0)
  case "$1" in
    handoff:human)
      printf '%s\n%s\n' "C2E0C6" "中断ハンドオフ: 人手が要るタスク(GUI・認証・物理・判断・外部連絡)"
      ;;
    handoff:ai)
      printf '%s\n%s\n' "BFD4F2" "中断ハンドオフ: 後続セッションの AI が着手できるタスク"
      ;;
    *)
      return 1
      ;;
  esac
}

cmd_labels_missing() {
  local repo="${1:?repo required}"
  local existing want=(handoff:human handoff:ai) w
  existing="$(gh label list -R "$repo" --json name -q '.[].name' 2>/dev/null)" || {
    echo "labels-missing: gh label list に失敗しました(${repo})" >&2
    return 1
  }
  for w in "${want[@]}"; do
    grep -qxF -- "$w" <<<"$existing" || printf '%s\n' "$w"
  done
}

cmd_create_labels() {
  local repo="${1:?repo required}"
  shift
  (($# > 0)) || {
    echo "create-labels: ラベル名を 1 つ以上指定してください" >&2
    return 1
  }
  local name meta color desc
  for name in "$@"; do
    meta="$(label_meta "$name")" || {
      echo "create-labels: 未知のラベル名です(handoff:human / handoff:ai のみ受理): ${name}" >&2
      return 1
    }
    color="$(sed -n '1p' <<<"$meta")"
    desc="$(sed -n '2p' <<<"$meta")"
    gh label create "$name" -R "$repo" --color "$color" --description "$desc" --force || {
      echo "create-labels: ${name} の作成に失敗しました(${repo})" >&2
      return 1
    }
  done
}

cmd_block() {
  local repo="${1:?repo required}" blocked="${2:?blocked issue number required}" \
    blocker="${3:?blocker issue number required}"
  local blocker_id
  blocker_id="$(gh api "repos/${repo}/issues/${blocker}" --jq .id 2>/dev/null)" || {
    echo "block: blocker #${blocker} の id 取得に失敗しました(${repo})" >&2
    return 1
  }
  [[ "$blocker_id" =~ ^[0-9]+$ ]] || {
    echo "block: blocker #${blocker} の id が数値ではありません(${blocker_id})" >&2
    return 1
  }
  gh api "repos/${repo}/issues/${blocked}/dependencies/blocked_by" \
    -X POST -F "issue_id=${blocker_id}" >/dev/null || {
    echo "block: #${blocked} に #${blocker} を blocked_by として追加できませんでした(${repo})" >&2
    return 1
  }
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

  # gh スタブ: HANDOFF_STUB_{LABEL_LIST,LABEL_CREATE,ISSUE_ID,BLOCK}_* で制御。
  # LABEL_LIST_TEXT     : gh label list -q '.[].name' の応答(改行区切り、既定空)
  # LABEL_LIST_FAIL=1   : gh label list を非ゼロ終了させる
  # LABEL_CREATE_FAIL=1 : gh label create を非ゼロ終了させる
  # LABEL_CREATE_LOG    : gh label create の呼び出し引数を1行ずつ追記するファイル
  # ISSUE_ID            : repos/.../issues/<N> --jq .id の応答(既定 9001)
  # ISSUE_ID_FAIL=1     : 同呼び出しを非ゼロ終了させる
  # BLOCK_FAIL=1        : dependencies/blocked_by の POST を非ゼロ終了させる
  # BLOCK_LOG           : 同 POST の呼び出し引数を1行ずつ追記するファイル
  mkdir -p "$dir/bin"
  cat >"$dir/bin/gh" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  label)
    case "$2" in
      list)
        [[ "${HANDOFF_STUB_LABEL_LIST_FAIL:-0}" == "1" ]] && exit 1
        if [[ -n "${HANDOFF_STUB_LABEL_LIST_TEXT:-}" ]]; then
          printf '%s\n' "${HANDOFF_STUB_LABEL_LIST_TEXT}"
        fi
        ;;
      create)
        [[ "${HANDOFF_STUB_LABEL_CREATE_FAIL:-0}" == "1" ]] && exit 1
        if [[ -n "${HANDOFF_STUB_LABEL_CREATE_LOG:-}" ]]; then
          printf '%s\n' "$*" >>"${HANDOFF_STUB_LABEL_CREATE_LOG}"
        fi
        ;;
      *) exit 1 ;;
    esac
    ;;
  api)
    case "$*" in
      *dependencies/blocked_by*)
        [[ "${HANDOFF_STUB_BLOCK_FAIL:-0}" == "1" ]] && exit 1
        if [[ -n "${HANDOFF_STUB_BLOCK_LOG:-}" ]]; then
          printf '%s\n' "$*" >>"${HANDOFF_STUB_BLOCK_LOG}"
        fi
        ;;
      *issues/*)
        [[ "${HANDOFF_STUB_ISSUE_ID_FAIL:-0}" == "1" ]] && exit 1
        printf '%s' "${HANDOFF_STUB_ISSUE_ID:-9001}"
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$dir/bin/gh"
  stub_path="$dir/bin:$PATH"

  echo "labels-missing:"

  rc=0
  out="$(PATH="$stub_path" bash "$self" labels-missing example/example 2>"$dir/err")" || rc=$?
  check "両方欠落: exit 0" 0 "$rc"
  check "両方欠落: 2行(handoff:human)" 1 "$(grep -Fc 'handoff:human' <<<"$out")"
  check "両方欠落: 2行(handoff:ai)" 1 "$(grep -Fc 'handoff:ai' <<<"$out")"

  rc=0
  out="$(HANDOFF_STUB_LABEL_LIST_TEXT="handoff:human
bug" PATH="$stub_path" bash "$self" labels-missing example/example 2>"$dir/err")" || rc=$?
  check "片方存在: exit 0" 0 "$rc"
  check "片方存在: handoff:ai だけが出る" "handoff:ai" "$out"

  rc=0
  out="$(HANDOFF_STUB_LABEL_LIST_TEXT="handoff:human
handoff:ai" PATH="$stub_path" bash "$self" labels-missing example/example 2>"$dir/err")" || rc=$?
  check "両方存在: exit 0" 0 "$rc"
  check "両方存在: 出力なし" "" "$out"

  rc=0
  out="$(HANDOFF_STUB_LABEL_LIST_FAIL=1 PATH="$stub_path" bash "$self" labels-missing example/example 2>"$dir/err")" || rc=$?
  check "gh label list 失敗: exit 非0" 1 "$rc"
  check_grep "gh label list 失敗: エラーメッセージ" "gh label list に失敗" "$(cat "$dir/err")"

  echo "create-labels:"

  rc=0
  : >"$dir/create.log"
  HANDOFF_STUB_LABEL_CREATE_LOG="$dir/create.log" PATH="$stub_path" \
    bash "$self" create-labels example/example handoff:human handoff:ai 2>"$dir/err" || rc=$?
  check "両方作成: exit 0" 0 "$rc"
  check_grep "handoff:human の色・説明が渡る" "C2E0C6" "$(cat "$dir/create.log")"
  check_grep "handoff:ai の色・説明が渡る" "BFD4F2" "$(cat "$dir/create.log")"

  rc=0
  out="$(PATH="$stub_path" bash "$self" create-labels example/example bug 2>"$dir/err")" || rc=$?
  check "未知のラベル名: exit 非0" 1 "$rc"
  check_grep "未知のラベル名: エラーメッセージ" "未知のラベル名" "$(cat "$dir/err")"

  rc=0
  out="$(PATH="$stub_path" bash "$self" create-labels example/example 2>"$dir/err")" || rc=$?
  check "ラベル名 0個: exit 非0" 1 "$rc"

  rc=0
  out="$(HANDOFF_STUB_LABEL_CREATE_FAIL=1 PATH="$stub_path" bash "$self" create-labels example/example handoff:ai 2>"$dir/err")" || rc=$?
  check "gh label create 失敗: exit 非0" 1 "$rc"

  echo "block:"

  rc=0
  : >"$dir/block.log"
  HANDOFF_STUB_ISSUE_ID=4242 HANDOFF_STUB_BLOCK_LOG="$dir/block.log" \
    PATH="$stub_path" bash "$self" block example/example 10 20 2>"$dir/err" || rc=$?
  check "成功: exit 0" 0 "$rc"
  check_grep "blocker の id(4242)を issue_id として渡す" "issue_id=4242" "$(cat "$dir/block.log")"
  check_grep "blocked 側(#10)の dependencies エンドポイントを叩く" "issues/10/dependencies/blocked_by" \
    "$(cat "$dir/block.log")"

  rc=0
  : >"$dir/block2.log"
  HANDOFF_STUB_ISSUE_ID_FAIL=1 HANDOFF_STUB_BLOCK_LOG="$dir/block2.log" \
    PATH="$stub_path" bash "$self" block example/example 10 20 2>"$dir/err" || rc=$?
  check "id 取得失敗: exit 非0" 1 "$rc"
  check "id 取得失敗: block エンドポイントは叩かれない" "" "$(cat "$dir/block2.log")"

  rc=0
  HANDOFF_STUB_ISSUE_ID=4242 HANDOFF_STUB_BLOCK_FAIL=1 \
    PATH="$stub_path" bash "$self" block example/example 10 20 2>"$dir/err" || rc=$?
  check "POST 失敗: exit 非0" 1 "$rc"
  check_grep "POST 失敗: エラーメッセージ" "blocked_by として追加できません" "$(cat "$dir/err")"

  echo "usage:"

  rc=0
  PATH="$stub_path" bash "$self" 2>"$dir/err" || rc=$?
  check "サブコマンド無し: exit 非0" 1 "$([[ "$rc" -ne 0 ]] && echo 1 || echo "$rc")"

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- サブコマンド本体 ------------------------------------------------------------
case "${1:-}" in
  labels-missing)
    shift
    cmd_labels_missing "$@"
    ;;
  create-labels)
    shift
    cmd_create_labels "$@"
    ;;
  block)
    shift
    cmd_block "$@"
    ;;
  *)
    echo "usage: handoff.sh {labels-missing|create-labels|block} ... (または --selftest)" >&2
    exit 64
    ;;
esac
