#!/usr/bin/env bash
# slot-hit.sh — 候補日程一覧(調整さん等、人から提示される空き日程の
# 問い合わせ全般)と Google Calendar の予定を突き合わせ、朝・昼・夜の3コマ
# 単位で当たり判定(○/△/×)を出す判定コア。bash + yq(TOML→JSON 変換)+ jq
# のみで動く(ADR-0010 D13、private な person-state リポジトリ の判定)。旧 slot-hit.py
# (Python)の移植。判定ロジック自体(classify_events/judge/build_plan)は
# judge.jq、裁定後の確定化(finalize)は finalize.jq、共通関数は lib.jq に
# 持つ — このファイルは TOML→JSON 変換・events-dir の読み込み・引数パース
# のみを担う薄いオーケストレーション層。
#
# ネットワークアクセスもファイル書き込み(出力先を除く)も行わない。
# Google Calendar の読み書きも、候補日程一覧の取得元固有の読み取り・回答
# 書き込みも呼び出し側(Claude)が担う。詳細は SKILL.md を参照。
#
# 設定ファイル(TOML)のスキーマは docs/claude/slot-availability.md を参照
# (元の slot-hit.py の docstring と同一)。
#
# 候補日程一覧の入力契約(judge/finalize の --candidates/--decided)は
# 構造化 JSON: `[{"date": "2026-10-18", "time": "18:00"}, ...]`。
#
# 終日イベントのうち soft_day_prefixes に一致するものは、同じ日に時間指定
# の確定予定が1件も無いとエラー終了する(judge.jq の validate_soft_days)。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for cmd in yq jq; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: $cmd が見つかりません(PATH を確認してください)" >&2
    exit 2
  fi
done

usage() {
  cat >&2 <<'EOF'
使い方:
  slot-hit.sh --selftest
  slot-hit.sh judge --config <path> --candidates <path> --events-dir <dir>
              [--event-title <str>] [--source-url <str>] [--out-plan <path>]
  slot-hit.sh infer-year --month <N> --day <N> --weekday <字> --today <YYYY-MM-DD>
  slot-hit.sh enumerate --config <path> --from <YYYY-MM-DD> --to <YYYY-MM-DD>
              [--weekdays <0-6,...>] [--slots <名前,...>] [--out <path>]
  slot-hit.sh finalize --config <path> --decided <path> --markers <path>
              --event-title <str> [--source-url <str>] --today <YYYY-MM-DD>
              [--venue <str>] [--out-plan <path>]
EOF
}

# IANA タイムゾーン名(例 "Asia/Tokyo")の実データファイルを探し、そこへの
# 絶対パスで `TZ=":<path>" date +%z` を呼んで UTC オフセットを解決する。
#
# `TZ="Asia/Tokyo" date +%z`(ゾーン名を直接渡す一般的な書き方)は、glibc
# のデフォルト TZDIR 検索パスに依存する。nix でビルドされた coreutils の
# `date` はこの検索パスにシステムの `/usr/share/zoneinfo` を含まないため、
# システムに tzdata が入っていてもゾーン名が解決できず、**エラーにも
# ならずに黙って UTC(+0000)へフォールバックする**(実測 2026-09-27、
# nix profile 由来の coreutils 9.11)。これは検出すらできない誤判定の
# 温床になるため、tzdata ファイルの実在を自分で確認してから絶対パス指定
# (`TZ=":<絶対パス>"`)で呼ぶ方式に倒した — 見つからなければ即エラー
# 終了し、黙った UTC フォールバックを許さない。
resolve_tz_offset() {
  local timezone="$1"
  local candidates=("/usr/share/zoneinfo" "/etc/zoneinfo")
  [[ -n "${TZDIR:-}" ]] && candidates+=("$TZDIR")
  local d
  for d in /nix/store/*-tzdata-*/share/zoneinfo "$HOME/.nix-profile/share/zoneinfo" /run/current-system/sw/share/zoneinfo; do
    [[ -d "$d" ]] && candidates+=("$d")
  done
  local dir
  for dir in "${candidates[@]}"; do
    if [[ -n "$dir" && -f "$dir/$timezone" ]]; then
      TZ=":${dir}/${timezone}" date +%z
      return 0
    fi
  done
  echo "error: タイムゾーン '${timezone}' の tzdata ファイルが見つかりません" \
    "(探索先: ${candidates[*]})" >&2
  return 1
}

# config.toml -> JSON。timezone フィールドから tz_offset("+09:00")と
# tz_offset_seconds(整数)を一度だけ resolve_tz_offset で解決して埋め込む
# (以降の日時計算は全て jq 側の純粋な算術で完結させる — lib.jq 参照)。
load_config_json() {
  local config_path="$1"
  local raw
  raw="$(yq -p toml -o json "$config_path")"
  local timezone
  timezone="$(jq -r '.timezone' <<<"$raw")"
  local tz_offset_raw
  tz_offset_raw="$(resolve_tz_offset "$timezone")" || exit 2
  local sign="${tz_offset_raw:0:1}" hh="${tz_offset_raw:1:2}" mm="${tz_offset_raw:3:2}"
  local tz_offset_seconds=$(( 10#$hh * 3600 + 10#$mm * 60 ))
  if [[ "$sign" == "-" ]]; then tz_offset_seconds=$(( -tz_offset_seconds )); fi
  local tz_offset="${sign}${hh}:${mm}"
  jq --arg tzoff "$tz_offset" --argjson tzoffsec "$tz_offset_seconds" \
    '. + {tz_offset: $tzoff, tz_offset_seconds: $tzoffsec}' <<<"$raw"
}

# events-dir 配下の <calendar-id>.json(list_events の生レスポンス)を
# config.calendars の allowlist ぶんだけ読み、`active = false` のエントリは
# 読まずに飛ばす(エントリ自体は設定に残る。参加・不参加が流動的な
# カレンダーを allowlist から消さずに判定から外すための、有効/無効の切り
# 替え)。`active` を省略したエントリは有効。boolean 以外は誤記(例:
# "false" の文字列は `!= false` を素通りして有効扱いになる)なのでエラー
# にする。
# [{"calendar_id","calendar_summary","ev": <生イベント>}, ...] にフラット化
# する。見つからないカレンダーは警告してスキップする(slot-hit.py と同じ
# 挙動)。
load_events_json() {
  local config_json="$1" events_dir="$2"
  local acc="[]"
  local cal cal_id cal_summary f cal_events
  if ! jq -e '[.calendars[]? | .active | select(. != null and type != "boolean")] | length == 0' \
    <<<"$config_json" >/dev/null; then
    echo "error: [[calendars]] の active は true/false(boolean)で指定してください" >&2
    exit 2
  fi
  while IFS= read -r cal; do
    [[ -z "$cal" ]] && continue
    cal_id="$(jq -r '.id' <<<"$cal")"
    cal_summary="$(jq -r '.summary // .id' <<<"$cal")"
    f="$events_dir/$cal_id.json"
    if [[ ! -f "$f" ]]; then
      echo "warning: ${cal_id}(${cal_summary}) の events ファイルが見つかりません(${f})。このカレンダーは判定から除外されます。" >&2
      continue
    fi
    cal_events="$(jq --arg cid "$cal_id" --arg cs "$cal_summary" \
      '(.events // []) | map({calendar_id: $cid, calendar_summary: $cs, ev: .})' "$f")"
    acc="$(jq -n --argjson acc "$acc" --argjson add "$cal_events" '$acc + $add')"
  done < <(jq -c '.calendars[]? | select(.active != false)' <<<"$config_json")
  printf '%s' "$acc"
}

cmd_judge() {
  local config="" candidates="" events_dir="" event_title="" source_url="" out_plan=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --config) config="$2"; shift 2 ;;
      --candidates) candidates="$2"; shift 2 ;;
      --events-dir) events_dir="$2"; shift 2 ;;
      --event-title) event_title="$2"; shift 2 ;;
      --source-url) source_url="$2"; shift 2 ;;
      --out-plan) out_plan="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$config" || -z "$candidates" || -z "$events_dir" ]]; then
    echo "error: judge には --config --candidates --events-dir が必須です" >&2
    exit 2
  fi

  local config_json events_json candidates_json result
  config_json="$(load_config_json "$config")"
  events_json="$(load_events_json "$config_json" "$events_dir")"
  candidates_json="$(cat "$candidates")"

  if ! result="$(jq -n -L"$SCRIPT_DIR" \
    --argjson config "$config_json" \
    --argjson events "$events_json" \
    --argjson candidates "$candidates_json" \
    --arg event_title "$event_title" \
    --arg source_url "$source_url" \
    -f "$SCRIPT_DIR/judge.jq" 2>&1)"; then
    echo "error: ${result#*error: }" >&2
    exit 2
  fi

  if [[ -n "$out_plan" ]]; then
    jq '.plan' <<<"$result" > "$out_plan"
  fi
  jq -r '.table' <<<"$result"
  echo ""
  echo "---"
  echo "plan.json:"
  jq '.plan' <<<"$result"
}

cmd_infer_year() {
  local month="" day="" weekday="" today=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --month) month="$2"; shift 2 ;;
      --day) day="$2"; shift 2 ;;
      --weekday) weekday="$2"; shift 2 ;;
      --today) today="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$month" || -z "$day" || -z "$weekday" || -z "$today" ]]; then
    echo "error: infer-year には --month --day --weekday --today が必須です" >&2
    exit 2
  fi

  local out
  if ! out="$(jq -n -L"$SCRIPT_DIR" \
    --argjson month "$month" --argjson day "$day" --arg weekday "$weekday" --arg today "$today" \
    'include "lib"; resolve_year($month; $day; $weekday; $today)' 2>&1)"; then
    echo "error: ${out#*error: }" >&2
    exit 2
  fi
  echo "$out"
}

# 期間 × 設定の全コマを、judge の入力契約の候補 JSON として列挙する。
# 候補一覧が無いとき(「いつ空いてる?」への応答)に、判定は既存の judge に
# 任せたまま候補集合だけを機械生成するための入口。判定は行わない。
cmd_enumerate() {
  local config="" from="" to="" weekdays="" slots="" out=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --config) config="$2"; shift 2 ;;
      --from) from="$2"; shift 2 ;;
      --to) to="$2"; shift 2 ;;
      --weekdays) weekdays="$2"; shift 2 ;;
      --slots) slots="$2"; shift 2 ;;
      --out) out="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$config" || -z "$from" || -z "$to" ]]; then
    echo "error: enumerate には --config --from --to が必須です" >&2
    exit 2
  fi

  local config_json weekdays_json slots_json result
  config_json="$(load_config_json "$config")"
  weekdays_json="$(jq -cn --arg s "$weekdays" '$s | split(",") | map(select(length > 0) | tonumber? // "invalid")')"
  slots_json="$(jq -cn --arg s "$slots" '$s | split(",") | map(select(length > 0))')"

  if ! result="$(jq -n -L"$SCRIPT_DIR" \
    --argjson config "$config_json" --arg from "$from" --arg to "$to" \
    --argjson weekdays "$weekdays_json" --argjson slots "$slots_json" \
    'include "lib"; enumerate_candidates($config; $from; $to; $weekdays; $slots)' 2>&1)"; then
    echo "error: ${result#*error: }" >&2
    exit 2
  fi

  if [[ -n "$out" ]]; then
    printf '%s\n' "$result" > "$out"
  else
    printf '%s\n' "$result"
  fi
}

cmd_finalize() {
  local config="" decided="" markers="" event_title="" source_url="" today="" venue="未定" out_plan=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --config) config="$2"; shift 2 ;;
      --decided) decided="$2"; shift 2 ;;
      --markers) markers="$2"; shift 2 ;;
      --event-title) event_title="$2"; shift 2 ;;
      --source-url) source_url="$2"; shift 2 ;;
      --today) today="$2"; shift 2 ;;
      --venue) venue="$2"; shift 2 ;;
      --out-plan) out_plan="$2"; shift 2 ;;
      *) echo "error: 不明なオプション: $1" >&2; usage; exit 2 ;;
    esac
  done
  if [[ -z "$config" || -z "$decided" || -z "$markers" || -z "$event_title" || -z "$today" ]]; then
    echo "error: finalize には --config --decided --markers --event-title --today が必須です" >&2
    exit 2
  fi

  local config_json decided_json markers_json result
  config_json="$(load_config_json "$config")"
  decided_json="$(cat "$decided")"
  markers_json="$(cat "$markers")"

  if ! result="$(jq -n -L"$SCRIPT_DIR" \
    --argjson config "$config_json" \
    --argjson decided "$decided_json" \
    --argjson markers "$markers_json" \
    --arg event_title "$event_title" \
    --arg source_url "$source_url" \
    --arg today "$today" \
    --arg venue "$venue" \
    -f "$SCRIPT_DIR/finalize.jq" 2>&1)"; then
    echo "error: ${result#*error: }" >&2
    exit 2
  fi

  if [[ -n "$out_plan" ]]; then
    jq '.plan' <<<"$result" > "$out_plan"
  fi
  jq -r '.table' <<<"$result"
  echo ""
  echo "---"
  echo "plan.json:"
  jq '.plan' <<<"$result"
}

main() {
  if [[ "${1:-}" == "--selftest" ]]; then
    exec bash "$SCRIPT_DIR/selftest.sh"
  fi

  local command="${1:-}"
  shift || true

  case "$command" in
    judge) cmd_judge "$@" ;;
    infer-year) cmd_infer_year "$@" ;;
    enumerate) cmd_enumerate "$@" ;;
    finalize) cmd_finalize "$@" ;;
    *)
      echo "error: --selftest か、judge/infer-year/enumerate/finalize いずれかのサブコマンドを指定してください" >&2
      usage
      exit 2
      ;;
  esac
}

main "$@"
