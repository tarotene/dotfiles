#!/usr/bin/env bash
# selftest.sh — routines-plan.sh の自己検査本体。ネットワーク・実 API には
# 一切依存しない。宣言・live trigger のフィクスチャはすべてこの中で作る
# プレースホルダ値(owner/repo-a 等)で、実在の repo 名は書かない
# (ADR-0034)。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLAN="$SCRIPT_DIR/routines-plan.sh"

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

TMPDIR_T="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_T"' EXIT

# --- 共通フィクスチャ -------------------------------------------------

DECL_BASE='{
  "name": "nightly-check",
  "id": "trig_TEST0001",
  "state": "enabled",
  "cron_utc": "0 3 * * *",
  "model": "claude-sonnet-5",
  "environment_id": "env_EXAMPLE",
  "home_repo": "owner/repo-a",
  "sources": ["owner/repo-a"],
  "allowed_tools": ["Bash"],
  "role": "worker"
}'
MD_BASE='Do the nightly check. See routines/nightly-check.md for details.'
MD_CHANGED='Do the nightly check (v2). See routines/nightly-check.md for details.'

decl_file="$TMPDIR_T/decl.json"
md_file="$TMPDIR_T/prompt.md"
printf '%s' "$DECL_BASE" > "$decl_file"
printf '%s' "$MD_BASE" > "$md_file"

# --- 1) build-body: 基本形と routine-spec 注記行 ------------------------

body="$(bash "$PLAN" build-body --declaration "$decl_file" --md "$md_file")"
check "1a build-body.name" "owner/repo-a:nightly-check" \
  "$(jq -r '.name' <<< "$body")"
check "1b build-body.cron_expression" "0 3 * * *" \
  "$(jq -r '.cron_expression' <<< "$body")"
check "1c build-body.enabled" "true" \
  "$(jq -r '.enabled' <<< "$body")"
content="$(jq -r '.job_config.ccr.events[0].data.message.content' <<< "$body")"
check "1d build-body prompt ends with routine-spec" "true" \
  "$([[ "$content" =~ routine-spec:\ [0-9a-f]{64}$ ]] && echo true || echo false)"
hash1="$(grep -o '[0-9a-f]\{64\}$' <<< "$content")"

# --- 2) live=in-sync: build-body の出力をそのまま live trigger に化けさせる

live_in_sync="$(jq -n --argjson body "$body" \
  '{id:"trig_TEST0001", name:$body.name, cron_expression:$body.cron_expression,
    enabled:$body.enabled, job_config:$body.job_config,
    suspension_reason:"", ended_reason:""}')"
live_file="$TMPDIR_T/live.json"
printf '%s' "$live_in_sync" > "$live_file"

check "2 classify in-sync" "in-sync" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_file" --live "$live_file" | head -1)"

# --- 3) declared-ahead: 宣言(md)だけ更新し、live は古いまま -------------

md_changed_file="$TMPDIR_T/prompt-v2.md"
printf '%s' "$MD_CHANGED" > "$md_changed_file"

out="$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_changed_file" --live "$live_file")"
check "3a classify declared-ahead" "declared-ahead" "$(head -1 <<< "$out")"
check "3b declared-ahead 2行目は新しい update body" "true" \
  "$(tail -n +2 <<< "$out" | jq -e '.job_config.ccr.events[0].data.message.content | test("v2")' > /dev/null && echo true || echo false)"

# --- 4) live-drift: 宣言は元のまま、live の本文だけ UI 編集された想定 ----

live_drifted="$(jq --arg h "$hash1" \
  '.job_config.ccr.events[0].data.message.content = "someone edited this in the UI\n\nroutine-spec: " + $h' \
  <<< "$live_in_sync")"
live_drift_file="$TMPDIR_T/live-drift.json"
printf '%s' "$live_drifted" > "$live_drift_file"

check "4 classify live-drift" "live-drift" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_file" --live "$live_drift_file" | head -1)"

# --- 5) conflict: 宣言(md v2)と live の本文、両方が注記から乖離 --------

check "5 classify conflict" "conflict" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_changed_file" --live "$live_drift_file" | head -1)"

# --- 6) suspended: suspension_reason が非空 -----------------------------

live_suspended="$(jq '.suspension_reason = "quota_exceeded"' <<< "$live_in_sync")"
live_suspended_file="$TMPDIR_T/live-suspended.json"
printf '%s' "$live_suspended" > "$live_suspended_file"

check "6 classify suspended" "suspended" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_file" --live "$live_suspended_file" | head -1)"

# --- 7) refuse: routine-spec 注記が無い ---------------------------------

live_no_annotation="$(jq '.job_config.ccr.events[0].data.message.content = "no annotation here"' <<< "$live_in_sync")"
live_no_annotation_file="$TMPDIR_T/live-no-annotation.json"
printf '%s' "$live_no_annotation" > "$live_no_annotation_file"

check "7 classify refuse(注記なし)" "refuse" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_file" --live "$live_no_annotation_file" | head -1)"

# --- 8) refuse: live.name が宣言の名前空間キーと不一致 -------------------

live_name_mismatch="$(jq '.name = "someone-elses-routine"' <<< "$live_in_sync")"
live_name_mismatch_file="$TMPDIR_T/live-name-mismatch.json"
printf '%s' "$live_name_mismatch" > "$live_name_mismatch_file"

check "8 classify refuse(name不一致)" "refuse" \
  "$(bash "$PLAN" classify --declaration "$decl_file" --md "$md_file" --live "$live_name_mismatch_file" | head -1)"

# --- 9) new: 宣言に id が無い -------------------------------------------

decl_no_id="$(jq 'del(.id)' <<< "$DECL_BASE")"
decl_no_id_file="$TMPDIR_T/decl-no-id.json"
printf '%s' "$decl_no_id" > "$decl_no_id_file"
null_file="$TMPDIR_T/null.json"
printf 'null' > "$null_file"

check "9 classify new" "new" \
  "$(bash "$PLAN" classify --declaration "$decl_no_id_file" --md "$md_file" --live "$null_file" | head -1)"

# --- 10) build-body は cron_utc が無い宣言を拒否する(D11) --------------

decl_no_cron="$(jq 'del(.cron_utc)' <<< "$DECL_BASE")"
decl_no_cron_file="$TMPDIR_T/decl-no-cron.json"
printf '%s' "$decl_no_cron" > "$decl_no_cron_file"

set +e
out="$(bash "$PLAN" build-body --declaration "$decl_no_cron_file" --md "$md_file" 2>&1)"
rc=$?
set -e
check_error_contains "10 build-body rejects missing cron_utc" "run-once/webhook" "$rc" "$out"

# --- 11) classify-unmanaged ----------------------------------------------

live_list='{
  "data": [
    {"id": "trig_UNMANAGED", "name": "someone-elses-cron", "cron_expression": "0 4 * * *",
     "job_config": {"ccr": {"events": [{"data": {"message": {"content": "no annotation"}}}]}}},
    {"id": "trig_MANAGED", "name": "owner/repo-a:nightly-check", "cron_expression": "0 3 * * *",
     "job_config": {"ccr": {"events": [{"data": {"message": {"content": "has one\n\nroutine-spec: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}}]}}},
    {"id": "trig_RUNONCE", "name": "some run-once probe", "cron_expression": "",
     "job_config": {"ccr": {"events": [{"data": {"message": {"content": "no annotation, but not cron"}}}]}}}
  ]
}'
live_list_file="$TMPDIR_T/live-list.json"
printf '%s' "$live_list" > "$live_list_file"

unmanaged_out="$(bash "$PLAN" classify-unmanaged --live-list "$live_list_file")"
check "11a classify-unmanaged件数" "1" "$(wc -l <<< "$unmanaged_out" | tr -d ' ')"
check "11b classify-unmanaged id" "trig_UNMANAGED" "$(jq -r '.id' <<< "$unmanaged_out")"

# --- 結果 -----------------------------------------------------------------

echo "routines-plan.sh selftest: pass=$pass fail=$fail" >&2
if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
