#!/usr/bin/env bash
# routines-plan.sh — Claude Code routines(claude.ai の scheduled cloud
# agent)を as-code で扱うための差分コア。宣言(各家リポジトリの
# `.claude/routines/<name>.json` + `<name>.md`)と live trigger
# (`RemoteTrigger`/meta connector の get/list が返す生 JSON)を突き合わせ、
# 分類(in-sync/declared-ahead/live-drift/conflict/suspended/new/refuse/
# unmanaged)と、送信すべき create/update body を決定的に計算する。
# bash + jq のみで動く(ADR-519-routines-declaration-in-repo D5、
# config/claude/skills/slot-availability/scripts/slot-hit.sh と同型)。
#
# ネットワークアクセスは一切行わない。RemoteTrigger/meta connector の
# 呼び出しはこのスクリプトの外(SKILL.md/auditor.md の手順)が担う —
# このスクリプトは入力 JSON から出力 JSON/分類名を計算するだけ。
#
# 正規化(decl_projection/live_projection)と body 組み立て(build_body)は
# lib.jq、分類ロジックはこのファイル(bash 側)に持つ。sha256 計算は
# jq に組み込みが無いため coreutils の sha256sum に委ねる。
#
# 宣言スキーマは docs/claude/claude-routines.md を参照。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for cmd in jq sha256sum; do
  if ! command -v "$cmd" > /dev/null 2>&1; then
    echo "error: $cmd が見つかりません(PATH を確認してください)" >&2
    exit 2
  fi
done

usage() {
  cat >&2 << 'EOF'
使い方:
  routines-plan.sh --selftest
  routines-plan.sh build-body --declaration <path> --md <path>
  routines-plan.sh classify --declaration <path> --md <path> --live <path>
              (--live には get の生 JSON、または decl.id が無い新規宣言なら
               中身が `null` の JSON ファイルを渡す)
  routines-plan.sh classify-unmanaged --live-list <path>
              (--live-list には list の生 JSON。{"data":[...]} 形式でも
               裸の配列でも可)
EOF
}

decl_projection_json() { # $1=decl_json(文字列) $2=md_body(文字列)
  jq -ncS -L"$SCRIPT_DIR" --argjson decl "$1" --arg md "$2" \
    'include "lib"; $decl | decl_projection($md)'
}

live_projection_json() { # $1=live_json(文字列、"null" 可)
  jq -ncS -L"$SCRIPT_DIR" --argjson live "$1" \
    'include "lib"; $live | live_projection'
}

live_annotation_of() { # $1=live_json(文字列、"null" 可)
  jq -nr -L"$SCRIPT_DIR" --argjson live "$1" \
    'include "lib"; ($live | live_annotation) // "null"'
}

sha256_of() { # $1=正規化 JSON 文字列(改行なし)
  printf '%s' "$1" | sha256sum | cut -d' ' -f1
}

require_cron_utc() { # $1=decl_json(文字列)
  local cron_utc
  cron_utc="$(jq -r '.cron_utc // empty' <<< "$1")"
  if [[ -z "$cron_utc" ]]; then
    echo "error: declaration.cron_utc is required — run-once/webhook trigger は宣言できません(D11)" >&2
    exit 2
  fi
}

cmd_build_body() {
  local declaration="" md=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --declaration) declaration="$2"; shift 2 ;;
      --md) md="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$declaration" || -z "$md" ]]; then
    echo "error: build-body には --declaration --md が必須です" >&2
    exit 2
  fi

  local decl_json md_body hash
  decl_json="$(cat "$declaration")"
  md_body="$(cat "$md")"
  require_cron_utc "$decl_json"
  hash="$(sha256_of "$(decl_projection_json "$decl_json" "$md_body")")"

  jq -n -L"$SCRIPT_DIR" --argjson decl "$decl_json" --arg md "$md_body" --arg hash "$hash" \
    'include "lib"; $decl | build_body($md; $hash)'
}

# 分類名を1行 stdout に出す。declared-ahead のときは2行目に update body
# (build-body と同じ形)を続けて出す。
cmd_classify() {
  local declaration="" md="" live=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --declaration) declaration="$2"; shift 2 ;;
      --md) md="$2"; shift 2 ;;
      --live) live="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$declaration" || -z "$md" || -z "$live" ]]; then
    echo "error: classify には --declaration --md --live が必須です" >&2
    exit 2
  fi

  local decl_json md_body live_json decl_id
  decl_json="$(cat "$declaration")"
  md_body="$(cat "$md")"
  live_json="$(cat "$live")"
  decl_id="$(jq -r '.id // empty' <<< "$decl_json")"

  if [[ -z "$decl_id" ]]; then
    if [[ "$live_json" != "null" ]]; then
      echo "error: declaration に id が無いのに --live が null ではありません(adopt するなら id を宣言に書いてください)" >&2
      exit 2
    fi
    echo "new"
    return 0
  fi

  if [[ "$live_json" == "null" ]]; then
    echo "refuse"
    return 0
  fi

  require_cron_utc "$decl_json"

  local annotation live_name expected_name suspension
  annotation="$(live_annotation_of "$live_json")"
  live_name="$(jq -r '.name // ""' <<< "$live_json")"
  expected_name="$(jq -r '.home_repo + ":" + .name' <<< "$decl_json")"

  if [[ "$annotation" == "null" || "$live_name" != "$expected_name" ]]; then
    echo "refuse"
    return 0
  fi

  suspension="$(jq -r '(.suspension_reason // "") + (.ended_reason // "")' <<< "$live_json")"
  if [[ -n "$suspension" ]]; then
    echo "suspended"
    return 0
  fi

  local decl_hash live_hash
  decl_hash="$(sha256_of "$(decl_projection_json "$decl_json" "$md_body")")"
  live_hash="$(sha256_of "$(live_projection_json "$live_json")")"

  if [[ "$decl_hash" == "$annotation" && "$live_hash" == "$annotation" ]]; then
    echo "in-sync"
  elif [[ "$decl_hash" != "$annotation" && "$live_hash" == "$annotation" ]]; then
    echo "declared-ahead"
    jq -n -L"$SCRIPT_DIR" --argjson decl "$decl_json" --arg md "$md_body" --arg hash "$decl_hash" \
      'include "lib"; $decl | build_body($md; $hash)'
  elif [[ "$decl_hash" == "$annotation" && "$live_hash" != "$annotation" ]]; then
    echo "live-drift"
  else
    echo "conflict"
  fi
}

cmd_classify_unmanaged() {
  local live_list=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --live-list) live_list="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$live_list" ]]; then
    echo "error: classify-unmanaged には --live-list が必須です" >&2
    exit 2
  fi

  local raw items
  raw="$(cat "$live_list")"
  items="$(jq -c 'if type == "object" then (.data // []) else . end' <<< "$raw")"

  jq -nc -L"$SCRIPT_DIR" --argjson items "$items" '
    include "lib";
    $items[]
    | select(is_cron)
    | select((live_annotation) == null)
    | {id, name, cron_expression}
  '
}

main() {
  if [[ "${1:-}" == "--selftest" ]]; then
    exec bash "$SCRIPT_DIR/selftest.sh"
  fi

  local command="${1:-}"
  shift || true

  case "$command" in
    build-body) cmd_build_body "$@" ;;
    classify) cmd_classify "$@" ;;
    classify-unmanaged) cmd_classify_unmanaged "$@" ;;
    *)
      echo "error: --selftest か、build-body/classify/classify-unmanaged いずれかのサブコマンドを指定してください" >&2
      usage
      exit 2
      ;;
  esac
}

main "$@"
