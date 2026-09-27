#!/usr/bin/env bash
# selftest.sh — slot-hit.sh の自己検査本体(旧 Python 版 slot-hit.py の
# selftest dispatcher が持っていた12項目を bash + yq + jq へ移植したもの、
# ADR-0010 D13)。slot-hit.sh の selftest サブコマンドから起動される。
# ネットワーク・外部ファイルには一切依存しない。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pass=0
fail=0

check() {
  local name="$1" expected="$2" actual="$3"
  if [[ "$actual" == "$expected" ]]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $name" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
  fi
}

check_error_contains() {
  local name="$1" needle="$2" rc="$3" output="$4"
  if [[ "$rc" -ne 0 && "$output" == *"$needle"* ]]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $name (rc=$rc)" >&2
    echo "  output: $output" >&2
  fi
}

CONFIG='{
  "timezone": "Asia/Tokyo", "tz_offset": "+09:00", "tz_offset_seconds": 32400,
  "buffer_minutes": 60, "marker_prefix": "【調整中】", "marker_calendar": "primary",
  "slots": {
    "morning": {"start":"09:00","end":"12:00"},
    "noon": {"start":"13:00","end":"17:00"},
    "evening": {"start":"18:00","end":"21:00"}
  },
  "ignore_prefixes": ["【勉強期間】"], "soft_day_prefixes": ["【試験本番】"]
}'

run_judge() {
  local events="$1" cands="$2" src="${3:-}"
  jq -n -L"$SCRIPT_DIR" --argjson config "$CONFIG" --argjson events "$events" --argjson candidates "$cands" \
    --arg event_title "テスト" --arg source_url "$src" -f "$SCRIPT_DIR/judge.jq"
}

# 1) match_slot 境界: 19:00 は evening [18:00,21:00) に含まれる。
check "1a match_slot evening" "evening" \
  "$(jq -rn -L"$SCRIPT_DIR" --argjson config "$CONFIG" 'include "lib"; "19:00" | hm_to_minutes | match_slot($config.slots)')"
check "1b match_slot morning" "morning" \
  "$(jq -rn -L"$SCRIPT_DIR" --argjson config "$CONFIG" 'include "lib"; "09:00" | hm_to_minutes | match_slot($config.slots)')"
check "1c match_slot noon" "noon" \
  "$(jq -rn -L"$SCRIPT_DIR" --argjson config "$CONFIG" 'include "lib"; "13:00" | hm_to_minutes | match_slot($config.slots)')"

# 2) 年跨ぎ: 2026-09-25 以降で最初に 2027-01-10 の曜日と一致する年。
check "2 resolve_year" "2027" \
  "$(jq -n -L"$SCRIPT_DIR" 'include "lib"; resolve_year(1; 10; "日"; "2026-09-25")')"

# 3) バッファ境界: gap 60分 -> △、gap 61分 -> ○。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-10-25T09:00:00+09:00"},"end":{"dateTime":"2026-10-25T17:00:00+09:00"},"summary":"全奏"}}]' \
  '[{"date":"2026-10-25","time":"19:00"}]')
check "3a gap60->△" "△" "$(jq -r '.plan.answers[0]' <<<"$out")"
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-10-25T09:00:00+09:00"},"end":{"dateTime":"2026-10-25T16:59:00+09:00"},"summary":"全奏"}}]' \
  '[{"date":"2026-10-25","time":"19:00"}]')
check "3b gap61->○" "○" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 4) 直接重なり -> ×。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-10-18T06:00:00+09:00"},"end":{"dateTime":"2026-10-18T21:00:00+09:00"},"summary":"全奏"}}]' \
  '[{"date":"2026-10-18","time":"19:00"}]')
check "4 overlap->×" "×" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 5) soft_day で時間指定の確定予定が無ければエラー終了。
set +e
err=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"date":"2026-11-15"},"end":{"date":"2026-11-16"},"summary":"【試験本番】テスト"}}]' \
  '[{"date":"2026-11-15","time":"19:00"}]' 2>&1)
rc=$?
set -e
check_error_contains "5 soft_day without timed event errors" "時間指定の確定予定がありません" "$rc" "$err"

# 6) soft_day 上限: 直接重複も近接もない離れたコマは △ に丸める。soft_day
#    でない日の同条件なら ○(上限が掛からない)。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"date":"2026-11-15"},"end":{"date":"2026-11-16"},"summary":"【試験本番】テスト"}},
    {"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-11-15T09:40:00+09:00"},"end":{"dateTime":"2026-11-15T12:00:00+09:00"},"summary":"電力・管理"}},
    {"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-11-15T13:00:00+09:00"},"end":{"dateTime":"2026-11-15T14:20:00+09:00"},"summary":"機械・制御"}}]' \
  '[{"date":"2026-11-15","time":"19:00"},{"date":"2026-11-16","time":"19:00"}]')
check "6a soft_day day->△" "△" "$(jq -r '.plan.answers[0]' <<<"$out")"
check "6b non-soft_day day->○" "○" "$(jq -r '.plan.answers[1]' <<<"$out")"

# 7) 終日 ignore_prefixes は無視される(候補判定に影響しない)。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"date":"2026-09-28"},"end":{"date":"2026-10-31"},"summary":"【勉強期間】令和8年度 技術士第一次試験"}}]' \
  '[{"date":"2026-10-05","time":"19:00"}]')
check "7 ignore_prefix->○" "○" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 8) 自分のマーカー(同一 source_url)は無視、別 URL のマーカーは △。
url="https://chouseisan.com/s?h=example"
out=$(run_judge \
  "[{\"calendar_id\":\"primary\",\"calendar_summary\":\"本体\",\"ev\":{\"status\":\"confirmed\",\"start\":{\"dateTime\":\"2026-10-18T18:00:00+09:00\"},\"end\":{\"dateTime\":\"2026-10-18T21:00:00+09:00\"},\"summary\":\"【調整中】テスト\",\"description\":\"候補日程一覧: $url\"}}]" \
  '[{"date":"2026-10-18","time":"19:00"}]' "$url")
check "8a own marker skipped->○" "○" "$(jq -r '.plan.answers[0]' <<<"$out")"
out=$(run_judge \
  "[{\"calendar_id\":\"primary\",\"calendar_summary\":\"本体\",\"ev\":{\"status\":\"confirmed\",\"start\":{\"dateTime\":\"2026-10-18T18:00:00+09:00\"},\"end\":{\"dateTime\":\"2026-10-18T21:00:00+09:00\"},\"summary\":\"【調整中】別件\",\"description\":\"候補日程一覧: https://chouseisan.com/s?h=other\"}}]" \
  '[{"date":"2026-10-18","time":"19:00"}]' "$url")
check "8b foreign marker->△" "△" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 9) transparent な時間指定予定は無視。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-10-18T18:00:00+09:00"},"end":{"dateTime":"2026-10-18T21:00:00+09:00"},"summary":"自由","transparency":"transparent"}}]' \
  '[{"date":"2026-10-18","time":"19:00"}]')
check "9 transparent ignored->○" "○" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 10) MCP コネクタが終日イベントの date を YYYY-MM-DDT00:00:00Z 形式で
#     返す実測フォーマットを許容する。
out=$(run_judge \
  '[{"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","summary":"【試験本番】テスト","start":{"date":"2026-11-15T00:00:00Z"},"end":{"date":"2026-11-16T00:00:00Z"}}},
    {"calendar_id":"primary","calendar_summary":"本体","ev":{"status":"confirmed","start":{"dateTime":"2026-11-15T09:00:00+09:00"},"end":{"dateTime":"2026-11-15T10:00:00+09:00"},"summary":"何か"}}]' \
  '[{"date":"2026-11-15","time":"19:00"}]')
check "10 quirky all-day date format->△" "△" "$(jq -r '.plan.answers[0]' <<<"$out")"

# 11) 構造化 JSON の候補パース(判定コアの入力契約)。正常系は date/time
#     から slot_name まで解決できる。異常系(必須キー欠落・不正な日付書式)
#     はエラー終了する。
check "11a valid candidate resolves slot" "evening" \
  "$(jq -rn -L"$SCRIPT_DIR" 'include "lib"; validate_candidates([{"date":"2026-10-18","time":"19:00"}])[0].time | hm_to_minutes | match_slot({"morning":{"start":"09:00","end":"12:00"},"noon":{"start":"13:00","end":"17:00"},"evening":{"start":"18:00","end":"21:00"}})')"
for bad in '[{"date":"2026-10-18"}]' '[{"date":"not-a-date","time":"19:00"}]' '[{"date":"2026-10-18","time":"19h00"}]' '[{"date":"2026-02-30","time":"19:00"}]'; do
  set +e
  err=$(jq -n -L"$SCRIPT_DIR" --argjson items "$bad" 'include "lib"; validate_candidates($items)' 2>&1)
  rc=$?
  set -e
  check_error_contains "11b invalid candidate rejected ($bad)" "候補日程の形式が想定外です" "$rc" "$err"
done

# 12) finalize: 一致マーカーの update / 候補外枠の create / 時刻ズレの
#     update / 非対応マーカーの delete / 他イベント(接頭辞不一致)は
#     一切触らない、の5点を1シナリオでまとめて検証する。
url2="https://chouseisan.com/s?h=example"
markers=$(cat <<EOF
{"events":[
  {"id":"m1-match","status":"confirmed","start":{"dateTime":"2026-11-15T13:00:00+09:00"},"end":{"dateTime":"2026-11-15T17:00:00+09:00"},"summary":"【調整中】Oboe concerto合わせ","description":"候補日程一覧: $url2"},
  {"id":"m2-shifted","status":"confirmed","start":{"dateTime":"2026-12-19T19:00:00+09:00"},"end":{"dateTime":"2026-12-19T21:00:00+09:00"},"summary":"【調整中】Oboe concerto合わせ","description":"候補日程一覧: $url2"},
  {"id":"m3-unmatched","status":"confirmed","start":{"dateTime":"2026-10-18T18:00:00+09:00"},"end":{"dateTime":"2026-10-18T21:00:00+09:00"},"summary":"【調整中】Oboe concerto合わせ","description":"候補日程一覧: $url2"},
  {"id":"m4-foreign","status":"confirmed","start":{"dateTime":"2026-11-15T09:00:00+09:00"},"end":{"dateTime":"2026-11-15T12:00:00+09:00"},"summary":"別件の予定","description":"関係ない説明"}
]}
EOF
)
decided='[{"date":"2026-11-15","time":"13:00"},{"date":"2026-12-19","time":"18:00"},{"date":"2026-12-05","time":"18:00"}]'
out=$(jq -n -L"$SCRIPT_DIR" --argjson config "$CONFIG" --argjson decided "$decided" --argjson markers "$markers" \
  --arg event_title "Oboe concerto合わせ" --arg source_url "$url2" --arg today "2026-09-26" --arg venue "未定" \
  -f "$SCRIPT_DIR/finalize.jq")
check "12a update ids" '["m1-match","m2-shifted"]' "$(jq -c '[.plan.update[].id] | sort' <<<"$out")"
check "12b m2 shifted start" "2026-12-19T18:00:00+09:00" "$(jq -r '.plan.update[] | select(.id=="m2-shifted") | .start' <<<"$out")"
check "12c m2 title stripped" "Oboe concerto合わせ" "$(jq -r '.plan.update[] | select(.id=="m2-shifted") | .title' <<<"$out")"
check "12d create count" "1" "$(jq '.plan.create | length' <<<"$out")"
check "12e create start" "2026-12-05T18:00:00+09:00" "$(jq -r '.plan.create[0].start' <<<"$out")"
check "12f delete ids" '["m3-unmatched"]' "$(jq -c '[.plan.delete[].id] | sort' <<<"$out")"
check "12g m4-foreign untouched" "true" "$(jq '([.plan.update, .plan.create] | flatten | map(.id) | index("m4-foreign")) == null' <<<"$out")"
check "12h description has venue/decision" "true" \
  "$(jq '([.plan.update, .plan.create] | flatten | all(.description | contains("場所: 未定(2026-09-26 時点)") and contains("裁定: 2026-09-26(主催者側で確定)") and contains("'"$url2"'")))' <<<"$out")"

echo ""
echo "=== $pass passed, $fail failed ==="
if [[ "$fail" -eq 0 ]]; then
  echo "OK: slot-hit.sh 自己検査 $((pass)) 項目すべて通過"
  exit 0
fi
exit 1
