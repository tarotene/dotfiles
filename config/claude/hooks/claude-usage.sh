#!/bin/sh
# claude-usage.sh — Claude の rate limit(5h セッション窓 / 週間モデル別上限)を
# herdr のタブバー右端に常時表示する。
#
# これは Claude Code hook ではない。settings.json には一切登録しない。
# herdr の `ui.tab_bar_right` の command エントリ(config/herdr/config.toml)が
# `/bin/sh -lc` で interval 実行し、標準出力の最終行をそのまま描画する。
# 設計と根拠: docs/claude/claude-usage-tabbar.md(このリポジトリ内)。
#
# データ源は `/usage` コマンドが内部で使う非公開 API
# `GET https://api.anthropic.com/api/oauth/usage`(ヘッダ
# `Authorization: Bearer <~/.claude/.credentials.json の accessToken>`)のみ。
# statusline / hooks の入力 JSON には rate limit 情報が来ないため、これが唯一の
# 経路であり、ドキュメント化されていない点はリスクとして残る。壊れたときの症状は
# 「タブバーからこのセグメントが消えるだけ」に収束させる(herdr の command 仕様:
# 失敗・空出力・timeout は表示クリア)。
#
# 縮退(すべて空出力 + exit 0。stderr にも出さない):
#   jq/curl/date 不在、~/.claude/.credentials.json 不在・トークン空、
#   HTTP 失敗(401 含む)・ネットワーク断、レスポンスが不正 JSON、
#   limits[] から表示可能なエントリが 1 件も取れない
#
# トークンは curl の argv には載せない(`/proc/<pid>/cmdline` 対策)。
# `--config -` で stdin から Authorization ヘッダを渡す。state file にも
# トークン・生レスポンスは保存しない。
#
# 使い方:
#   herdr から: 引数なしで呼ばれる(このファイル自身が /bin/sh -lc の対象)
#   自己検査:   claude-usage.sh --selftest
#   内部専用:   claude-usage.sh __render <usage_json_file> <state_file> <now_epoch>
#               (fetch を挟まず、フィクスチャからレンダリングだけを行う。
#               --selftest がネットワーク非依存でロジックを検証するために使う。)
set -eu

# ---- jq プログラム(ヒアドキュメントの 'EOF' はシェル展開を止めるため) -------

# limits[] の各エントリから、日付変換前の基礎情報を抜き出す。
# percent か resets_at が無いエントリは丸ごと捨てる(表示不能なので沈黙)。
EXTRACT_JQ="$(cat <<'JQ'
[ (.limits // [])[]
  | select(.percent != null and .resets_at != null)
  | . as $l
  | ($l.kind // "unknown") as $kind
  | {
      kind: $kind,
      percent: $l.percent,
      resets_at: $l.resets_at,
      exceeded: (
        ($l.percent >= 100)
        or ( (($l.severity // "") | ascii_downcase) | test("exceed|block|critical") )
      ),
      series_key: (
        if $kind == "weekly_scoped"
        then "weekly_scoped:" + ( ($l.scope.model.display_name) // "null" )
        else $kind
        end
      ),
      label: (
        if $kind == "session" then "5h"
        elif $kind == "weekly_scoped" then ( ($l.scope.model.display_name) // "wk" )
        elif $kind == "weekly" then "wk"
        else ($l.group // $kind)
        end
      )
    }
]
JQ
)"

# 燃焼率予測・表示行の組み立て・series(サンプル履歴)の更新。
# 純関数(日時変換は済んだ状態の $items を受け取る)なので selftest から
# ネットワーク・credentials 非依存に叩ける。
CORE_JQ="$(cat <<'JQ'
def min_span_for(k):
  if k == "session" then 300
  elif (k == "weekly_scoped" or k == "weekly") then 1800
  else 300 end;

def retention_for(k):
  if k == "session" then 5400
  elif (k == "weekly_scoped" or k == "weekly") then 21600
  else 5400 end;

def fmt_percent(p): (p | round | tostring);

def fmt_eta(sec):
  if sec >= 3600 then
    ((((sec / 3600 * 10) | round) / 10) | tostring) + "h"
  else
    ((sec / 60 | floor | tostring)) + "m"
  end;

[ $items[] | . as $it |
  ($prev_series[$it.series_key] // {resets_at: null, samples: []}) as $old |
  ( ($old.resets_at != null) and ($old.resets_at != $it.resets_at) ) as $reset_changed |
  ( (($old.samples | length) > 0) and ( ($old.samples[-1][1] - $it.percent) > 1 ) ) as $percent_dropped |
  ( if ($reset_changed or $percent_dropped) then [] else $old.samples end ) as $base |
  ( $base + [[$now, $it.percent]] ) as $with_new |
  ( [ $with_new[] | select( ($now - .[0]) <= retention_for($it.kind) ) ] ) as $pruned |
  ( $pruned[0] ) as $first |
  ( $pruned[-1] ) as $last |
  ( (($pruned | length) >= 2) and ( ($last[0] - $first[0]) >= min_span_for($it.kind) ) ) as $enough |
  ( if $enough then ( ($last[1] - $first[1]) / ($last[0] - $first[0]) ) else null end ) as $slope |
  ( if ($slope != null and $slope > 0) then ( (100 - $it.percent) / $slope ) else null end ) as $eta |
  ( if ($eta != null and ($now + $eta) < $it.reset_epoch) then $eta else null end ) as $eta_shown |
  ( $it.label + " " + fmt_percent($it.percent) + "%→" + $it.reset_display ) as $base_seg |
  ( if $it.exceeded then
      "!" + $base_seg
    elif $eta_shown != null then
      $base_seg + " (~" + fmt_eta($eta_shown) + ")"
    else
      $base_seg
    end
  ) as $segment |
  { key: $it.series_key, entry: {resets_at: $it.resets_at, samples: $pruned}, segment: $segment }
] as $r |
{ line: ([ $r[].segment ] | join(" · ")),
  series: (reduce $r[] as $x ({}; .[$x.key] = $x.entry)) }
JQ
)"

# ---- 日時変換(date に依存する唯一の箇所) ----------------------------------

# $1 = EXTRACT_JQ 適用済みの基礎 items(JSON 配列) $2 = now(epoch)
# resets_at ごとに reset_epoch / reset_display(24h 以内なら HH:MM、それより遠い
# なら M/D)を付与した配列を stdout に JSON で出す。date が解釈できないエントリ
# は黙って捨てる(表示不能なだけで、他のエントリの表示は妨げない)。
augment_items() {
  base="$1"
  now="$2"
  items_file="$(mktemp "${TMPDIR:-/tmp}/claude-usage-items.XXXXXX")" || {
    printf '[]'
    return 0
  }
  : >"$items_file"
  printf '%s' "$base" | jq -c '.[]' 2>/dev/null | while IFS= read -r item; do
    [ -n "$item" ] || continue
    resets_at="$(printf '%s' "$item" | jq -r '.resets_at' 2>/dev/null)" || continue
    reset_epoch="$(date -d "$resets_at" +%s 2>/dev/null)" || continue
    case "$reset_epoch" in '' | *[!0-9-]*) continue ;; esac
    if [ $((reset_epoch - now)) -le 86400 ]; then
      reset_display="$(date -d "$resets_at" +%H:%M 2>/dev/null)" || continue
    else
      reset_display="$(date -d "$resets_at" +%-m/%-d 2>/dev/null)" || continue
    fi
    printf '%s' "$item" |
      jq -c --argjson e "$reset_epoch" --arg d "$reset_display" \
        '. + {reset_epoch: $e, reset_display: $d}' 2>/dev/null >>"$items_file"
  done
  jq -s -c '.' "$items_file" 2>/dev/null || printf '[]'
  rm -f "$items_file"
}

# ---- レンダリング本体(fetch は挟まない、__render / 通常経路の共通コア) ----

# $1 = usage JSON が入ったファイル $2 = state file $3 = now(epoch)
# 成功時は表示行があれば stdout に 1 行、state file を atomic に更新する。
# 失敗・空結果は何もしない(呼び出し側は常に exit 0 で終える)。
render_from_files() {
  usage_file="$1"
  state_file="$2"
  now="$3"

  usage_json="$(cat "$usage_file" 2>/dev/null)" || return 0
  printf '%s' "$usage_json" | jq -e . >/dev/null 2>&1 || return 0

  prev_json='{}'
  if [ -s "$state_file" ]; then
    cand="$(cat "$state_file" 2>/dev/null)" || cand=''
    if printf '%s' "$cand" | jq -e . >/dev/null 2>&1; then
      prev_json="$cand"
    fi
  fi

  base_items="$(printf '%s' "$usage_json" | jq -c "$EXTRACT_JQ" 2>/dev/null)" || base_items='[]'
  [ -n "$base_items" ] || base_items='[]'

  augmented="$(augment_items "$base_items" "$now")"
  [ -n "$augmented" ] || augmented='[]'

  prev_series="$(printf '%s' "$prev_json" | jq -c '.series // {}' 2>/dev/null)" || prev_series='{}'

  result="$(
    jq -n -c \
      --argjson items "$augmented" \
      --argjson prev_series "$prev_series" \
      --argjson now "$now" \
      "$CORE_JQ" 2>/dev/null
  )" || return 0
  [ -n "$result" ] || return 0

  line="$(printf '%s' "$result" | jq -r '.line // empty' 2>/dev/null)" || line=''
  new_series="$(printf '%s' "$result" | jq -c '.series // {}' 2>/dev/null)" || new_series='{}'

  new_state="$(
    jq -n -c --argjson now "$now" --arg line "$line" --argjson series "$new_series" \
      '{last_fetch: $now, last_line: $line, series: $series}' 2>/dev/null
  )" || return 0

  state_dir="$(dirname "$state_file")"
  tmp_state="$(mktemp "$state_dir/claude-usage-tabbar.XXXXXX" 2>/dev/null)" || return 0
  printf '%s' "$new_state" >"$tmp_state" && mv -f "$tmp_state" "$state_file"

  [ -n "$line" ] && printf '%s\n' "$line"
  return 0
}

# ---- __render(内部専用。selftest がフィクスチャで叩く) ---------------------

if [ "${1:-}" = "__render" ]; then
  render_from_files "${2:-}" "${3:-}" "${4:-}"
  exit 0
fi

# ---- --selftest -------------------------------------------------------------

if [ "${1:-}" = "--selftest" ]; then
  self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
  # 日時変換をホストのローカル TZ に依存させず決定的にする。実運用では herdr の
  # command がホストの TZ を継承する(→ の表示はユーザーのローカル時刻になる)。
  TZ=UTC
  export TZ
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  check() { # check <名前> <期待> <実際>
    if [ "$2" = "$3" ]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected [$2], got [$3])" >&2
      fail=1
    fi
  }

  # 固定 epoch(実行時刻に依存しない)。
  NOW=1700000000

  mkusage() { # mkusage <file> <jq-array-literal of limits>
    printf '{"limits":%s}' "$2" >"$1"
  }

  hm_at() { date -u -d "@$1" +%H:%M; }
  md_at() { date -u -d "@$1" +%-m/%-d; }
  iso_at() { date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ; }

  # --- 通常表示(予測なし・2 limits) ------------------------------------
  r_session=$((NOW + 3600))
  r_weekly=$((NOW + 3 * 86400))
  usage_f="$dir/u1.json"
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":21,"severity":"normal","resets_at":"%s","scope":null},{"kind":"weekly_scoped","group":"weekly","percent":48,"severity":"normal","resets_at":"%s","scope":{"model":{"id":null,"display_name":"Fable"}}}]' \
      "$(iso_at "$r_session")" "$(iso_at "$r_weekly")"
  )"
  state_f="$dir/s1.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 21%→$(hm_at "$r_session") · Fable 48%→$(md_at "$r_weekly")"
  check "通常表示: 2 limits" "$expected" "$out"

  # --- 予測表示(履歴 2 点、リセット前に 100% 到達見込み) -----------------
  usage_f="$dir/u2.json"
  r_session2=$((NOW + 3600 * 5))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":52,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_session2")"
  )"
  state_f="$dir/s2.json"
  printf '{"last_fetch":0,"last_line":"","series":{"session":{"resets_at":"%s","samples":[[%s,40]]}}}' \
    "$(iso_at "$r_session2")" "$((NOW - 600))" >"$state_f"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  # slope=(52-40)/600=0.02%/s → eta=(100-52)/0.02=2400s=40m
  expected="5h 52%→$(hm_at "$r_session2") (~40m)"
  check "予測表示: リセット前に到達見込み" "$expected" "$out"

  # --- 予測なし(到達見込みがリセットより後) -----------------------------
  usage_f="$dir/u3.json"
  r_session3=$((NOW + 500))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":52,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_session3")"
  )"
  state_f="$dir/s3.json"
  printf '{"last_fetch":0,"last_line":"","series":{"session":{"resets_at":"%s","samples":[[%s,40]]}}}' \
    "$(iso_at "$r_session3")" "$((NOW - 600))" >"$state_f"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 52%→$(hm_at "$r_session3")"
  check "予測なし: 到達見込みがリセットより後" "$expected" "$out"

  # --- 予測なし(傾きが負) -----------------------------------------------
  usage_f="$dir/u4.json"
  r_session4=$((NOW + 3600 * 5))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":54,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_session4")"
  )"
  state_f="$dir/s4.json"
  printf '{"last_fetch":0,"last_line":"","series":{"session":{"resets_at":"%s","samples":[[%s,55]]}}}' \
    "$(iso_at "$r_session4")" "$((NOW - 600))" >"$state_f"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 54%→$(hm_at "$r_session4")"
  check "予測なし: 傾きが負(1pt 以内の低下は履歴継続)" "$expected" "$out"
  samples_len="$(jq -r '.series.session.samples | length' "$state_f")"
  check "1pt 以内の低下: 履歴は消えず 2 点になる" "2" "$samples_len"

  # --- percent が 1pt 超低下 → 履歴クリア ---------------------------------
  usage_f="$dir/u5.json"
  r_session5=$((NOW + 3600 * 5))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":30,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_session5")"
  )"
  state_f="$dir/s5.json"
  printf '{"last_fetch":0,"last_line":"","series":{"session":{"resets_at":"%s","samples":[[%s,60]]}}}' \
    "$(iso_at "$r_session5")" "$((NOW - 600))" >"$state_f"
  "$self" __render "$usage_f" "$state_f" "$NOW" >/dev/null
  samples_len="$(jq -r '.series.session.samples | length' "$state_f")"
  check "percent 1pt 超低下: 履歴クリアされ 1 点になる" "1" "$samples_len"

  # --- resets_at が変わった → 履歴クリア(percent は増えていても) -------
  usage_f="$dir/u6.json"
  r_old=$((NOW - 100))
  r_new=$((NOW + 3600 * 5))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":8,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_new")"
  )"
  state_f="$dir/s6.json"
  printf '{"last_fetch":0,"last_line":"","series":{"session":{"resets_at":"%s","samples":[[%s,5]]}}}' \
    "$(iso_at "$r_old")" "$((NOW - 600))" >"$state_f"
  "$self" __render "$usage_f" "$state_f" "$NOW" >/dev/null
  samples_len="$(jq -r '.series.session.samples | length' "$state_f")"
  check "resets_at 変化: 履歴クリアされ 1 点になる" "1" "$samples_len"

  # --- 上限到達(percent>=100) --------------------------------------------
  usage_f="$dir/u7.json"
  r7=$((NOW + 3600))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":100,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r7")"
  )"
  state_f="$dir/s7.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="!5h 100%→$(hm_at "$r7")"
  check "上限到達(percent>=100): ! 表示・予測なし" "$expected" "$out"

  # --- 上限到達(severity ベース、percent<100) -----------------------------
  usage_f="$dir/u8.json"
  r8=$((NOW + 3600))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":95,"severity":"blocked","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r8")"
  )"
  state_f="$dir/s8.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="!5h 95%→$(hm_at "$r8")"
  check "上限到達(severity=blocked): ! 表示" "$expected" "$out"

  # --- weekly ラベルのフォールバック(display_name 欠落) ------------------
  usage_f="$dir/u9.json"
  r9=$((NOW + 3 * 86400))
  mkusage "$usage_f" "$(
    printf '[{"kind":"weekly_scoped","group":"weekly","percent":10,"severity":"normal","resets_at":"%s","scope":{"model":{"display_name":null}}}]' \
      "$(iso_at "$r9")"
  )"
  state_f="$dir/s9.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="wk 10%→$(md_at "$r9")"
  check "weekly_scoped: display_name 欠落は wk にフォールバック" "$expected" "$out"

  # --- 未知 kind: group にフォールバックしつつ描画を継続 -------------------
  usage_f="$dir/u10.json"
  r10=$((NOW + 3 * 86400))
  mkusage "$usage_f" "$(
    printf '[{"kind":"seven_day_opus","group":"opus_weekly","percent":33,"severity":"normal","resets_at":"%s"}]' \
      "$(iso_at "$r10")"
  )"
  state_f="$dir/s10.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="opus_weekly 33%→$(md_at "$r10")"
  check "未知 kind: group ラベルで描画される" "$expected" "$out"

  # --- percent/resets_at 欠落エントリはスキップ、他は描画継続 --------------
  usage_f="$dir/u11.json"
  r11=$((NOW + 3600))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":null,"resets_at":"%s"},{"kind":"weekly","group":"weekly","percent":5,"severity":"normal","resets_at":"%s"}]' \
      "$(iso_at "$r11")" "$(iso_at "$r11")"
  )"
  state_f="$dir/s11.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="wk 5%→$(hm_at "$r11")"
  check "percent 欠落エントリはスキップ・他は描画" "$expected" "$out"

  # --- limits 空・欠落 → 空出力 --------------------------------------------
  usage_f="$dir/u12.json"
  printf '{"limits":[]}' >"$usage_f"
  state_f="$dir/s12.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "limits 空: 空出力" "" "$out"

  usage_f="$dir/u12b.json"
  printf '{}' >"$usage_f"
  state_f="$dir/s12b.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "limits キー欠落: 空出力" "" "$out"

  # --- 不正 JSON → 空出力、state file は変更しない -------------------------
  usage_f="$dir/u13.json"
  printf '{not valid json' >"$usage_f"
  state_f="$dir/s13.json"
  printf 'SENTINEL' >"$state_f"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "不正 JSON: 空出力" "" "$out"
  check "不正 JSON: state file は書き換えない" "SENTINEL" "$(cat "$state_f")"

  # --- トークン非漏えい + 30 秒ガード(通常経路をスタブで通す) -------------
  mkdir -p "$dir/bin" "$dir/home/.claude" "$dir/xdg"
  token="TESTTOKEN-$$-marker"
  printf '{"claudeAiOauth":{"accessToken":"%s"}}' "$token" >"$dir/home/.claude/.credentials.json"

  r_stub=$((NOW + 3600))
  fixture="$dir/fixture.json"
  mkusage "$fixture" "$(
    printf '[{"kind":"session","group":"session","percent":21,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r_stub")"
  )"

  cat >"$dir/bin/curl" <<STUB
#!/bin/sh
outfile=""
prev=""
for a in "\$@"; do
  case "\$a" in *Bearer*) exit 9 ;; esac
  if [ "\$prev" = "-o" ]; then outfile="\$a"; fi
  prev="\$a"
done
cfg="\$(cat)"
case "\$cfg" in
  *"Authorization: Bearer $token"*) : ;;
  *) exit 9 ;;
esac
echo call >>"$dir/curl-calls.log"
if [ -n "\$outfile" ]; then
  cat "$fixture" >"\$outfile"
else
  cat "$fixture"
fi
STUB
  chmod +x "$dir/bin/curl"

  out1="$(HOME="$dir/home" XDG_RUNTIME_DIR="$dir/xdg" PATH="$dir/bin:$PATH" "$self" 2>"$dir/err1")"
  check "通常経路: 実行 exit 0" "0" "$?"
  case "$out1" in
    *"$token"*)
      echo "FAIL トークン非漏えい: stdout にトークンが含まれる" >&2
      fail=1
      ;;
    *) echo "ok   トークン非漏えい: stdout にトークンなし" ;;
  esac
  state_content="$(cat "$dir/xdg/claude-usage-tabbar.json" 2>/dev/null || true)"
  case "$state_content" in
    *"$token"*)
      echo "FAIL トークン非漏えい: state file にトークンが含まれる" >&2
      fail=1
      ;;
    *) echo "ok   トークン非漏えい: state file にトークンなし" ;;
  esac
  check "通常経路: stderr 空" "" "$(cat "$dir/err1")"

  out2="$(HOME="$dir/home" XDG_RUNTIME_DIR="$dir/xdg" PATH="$dir/bin:$PATH" "$self")"
  check "30 秒ガード: 直後の再実行は同じ行" "$out1" "$out2"
  calls="$(wc -l <"$dir/curl-calls.log" | tr -d ' ')"
  check "30 秒ガード: curl は 1 回しか呼ばれない" "1" "$calls"

  exit "$fail"
fi

# ---- 通常経路(herdr が interval 実行する本体) -------------------------------

command -v jq >/dev/null 2>&1 || exit 0
command -v curl >/dev/null 2>&1 || exit 0
command -v date >/dev/null 2>&1 || exit 0

STATE_DIR="${XDG_RUNTIME_DIR:-/tmp}"
mkdir -p "$STATE_DIR" 2>/dev/null || exit 0
STATE_FILE="$STATE_DIR/claude-usage-tabbar.json"

now="$(date +%s 2>/dev/null)" || exit 0
case "$now" in '' | *[!0-9]*) exit 0 ;; esac

# 再取得ガード: herdr の interval とは独立に、reload-config 直後の即時実行
# ストーム等で 30 秒未満の再フェッチが起きないようにする。
if [ -s "$STATE_FILE" ]; then
  prev_candidate="$(cat "$STATE_FILE" 2>/dev/null)" || prev_candidate=''
  if printf '%s' "$prev_candidate" | jq -e . >/dev/null 2>&1; then
    last_fetch="$(printf '%s' "$prev_candidate" | jq -r '.last_fetch // 0' 2>/dev/null)" || last_fetch=0
    case "$last_fetch" in '' | *[!0-9]*) last_fetch=0 ;; esac
    if [ $((now - last_fetch)) -lt 30 ]; then
      last_line="$(printf '%s' "$prev_candidate" | jq -r '.last_line // empty' 2>/dev/null)" || last_line=''
      [ -n "$last_line" ] && printf '%s\n' "$last_line"
      exit 0
    fi
  fi
fi

CRED_FILE="${HOME:-}/.claude/.credentials.json"
[ -r "$CRED_FILE" ] || exit 0
token="$(jq -r '.claudeAiOauth.accessToken // empty' "$CRED_FILE" 2>/dev/null)" || exit 0
[ -n "$token" ] || exit 0

usage_file="$(mktemp "${TMPDIR:-/tmp}/claude-usage-fetch.XXXXXX")" || exit 0
trap 'rm -f "$usage_file"' EXIT HUP INT TERM

if ! curl -s --fail --max-time 5 --config - -o "$usage_file" 2>/dev/null <<CURLCFG
url = "https://api.anthropic.com/api/oauth/usage"
header = "Authorization: Bearer ${token}"
header = "anthropic-beta: oauth-2025-04-20"
CURLCFG
then
  exit 0
fi
unset token

render_from_files "$usage_file" "$STATE_FILE" "$now"
exit 0
