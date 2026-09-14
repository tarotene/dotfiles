#!/bin/sh
# claude-usage.sh — Claude の rate limit(5h セッション窓 / 週間モデル別上限)を
# herdr のタブバー右端に常時表示する。
#
# これは Claude Code hook ではない。settings.json には一切登録しない。
# herdr の `ui.tab_bar_right` の command エントリ(config/herdr/config.toml)が
# `/bin/sh -lc` で interval 実行し、標準出力の最終行をそのまま描画する。
# 設計と根拠: docs/claude/claude-usage.md(このリポジトリ内)。
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

# ペース着地予測・表示行の組み立て。
# 「窓開始(resets_at − 窓長)からの平均ペースで使い続けたら、リセット時点で
# 何 % に着地するか」を出す(着地% = 使用% ÷ 経過%)。サンプル履歴は使わない
# 純関数(日時変換済みの $items と $now だけで決まる)なので、state file に
# 予測用の状態を持たず、再起動直後・初回呼び出しでも即座に表示できる。
# 純関数なので selftest からネットワーク・state 非依存に叩ける。
CORE_JQ="$(cat <<'JQ'
def fmt_percent(p): (p | round | tostring);

# kind ごとの窓の長さ(秒)。未知の kind は着地を予測しない(null)。
def window_for(k):
  if k == "session" then 18000
  elif (k == "weekly_scoped" or k == "weekly") then 604800
  else null end;

[ $items[] | . as $it |
  window_for($it.kind) as $win |
  # 評価順が重要: show を先に確定し、除算・丸めは show=true の分岐内だけで
  # 行う。jq は null や 0 での除算をエラーにし、CORE_JQ が失敗すると
  # render_from_files() が空出力で中断して他の正常なセグメントまで消える。
  ( (($it.exceeded | not))
    and ($win != null)
    and ($it.reset_epoch > $now)
    and ( ($now - ($it.reset_epoch - $win)) >= ($win * 0.05) )
  ) as $show |
  ( if $show then
      ( ($now - ($it.reset_epoch - $win)) / $win ) as $elapsed |
      ( ($it.percent / $elapsed) | round ) as $landing |
      ( " " + (if $landing >= 100 then "▲" else "▼" end) + ($landing | tostring) + "%" )
    else
      ""
    end
  ) as $suffix |
  ( $it.label + " " + fmt_percent($it.percent) + "%→" + $it.reset_display ) as $base_seg |
  ( if $it.exceeded then "!" + $base_seg else $base_seg + $suffix end )
] as $segments |
{ line: ($segments | join(" · ")) }
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
# state file は 30 秒再取得ガード(last_fetch / last_line)専用で、予測ロジック
# 用の履歴は持たない(CORE_JQ が純関数のため不要)。
render_from_files() {
  usage_file="$1"
  state_file="$2"
  now="$3"

  usage_json="$(cat "$usage_file" 2>/dev/null)" || return 0
  printf '%s' "$usage_json" | jq -e . >/dev/null 2>&1 || return 0

  base_items="$(printf '%s' "$usage_json" | jq -c "$EXTRACT_JQ" 2>/dev/null)" || base_items='[]'
  [ -n "$base_items" ] || base_items='[]'

  augmented="$(augment_items "$base_items" "$now")"
  [ -n "$augmented" ] || augmented='[]'

  result="$(
    jq -n -c \
      --argjson items "$augmented" \
      --argjson now "$now" \
      "$CORE_JQ" 2>/dev/null
  )" || return 0
  [ -n "$result" ] || return 0

  line="$(printf '%s' "$result" | jq -r '.line // empty' 2>/dev/null)" || line=''

  new_state="$(
    jq -n -c --argjson now "$now" --arg line "$line" \
      '{last_fetch: $now, last_line: $line}' 2>/dev/null
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

  # --- 通常表示(session/weekly とも経過が十分で着地予測が付く) -----------
  r_session=$((NOW + 3600))
  r_weekly=$((NOW + 3 * 86400))
  usage_f="$dir/u1.json"
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":21,"severity":"normal","resets_at":"%s","scope":null},{"kind":"weekly_scoped","group":"weekly","percent":48,"severity":"normal","resets_at":"%s","scope":{"model":{"id":null,"display_name":"Fable"}}}]' \
      "$(iso_at "$r_session")" "$(iso_at "$r_weekly")"
  )"
  state_f="$dir/s1.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  # session: 経過=(18000-14400)/18000=0.8 → 着地=21/0.8=26.25→round 26 → ▼26%
  # weekly:  経過=345600/604800=4/7 → 着地=48/(4/7)=84 → ▼84%
  expected="5h 21%→$(hm_at "$r_session") ▼26% · Fable 48%→$(md_at "$r_weekly") ▼84%"
  check "通常表示: 2 limits(着地予測あり)" "$expected" "$out"

  # --- 着地予測: 使いすぎ側(▲) ------------------------------------------
  usage_f="$dir/u2.json"
  r2=$((NOW + 13500)) # 経過 25%(session 窓 18000s)
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":30,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r2")"
  )"
  state_f="$dir/s2.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  # 着地=30/0.25=120 → ▲120%
  expected="5h 30%→$(hm_at "$r2") ▲120%"
  check "着地予測: 使いすぎ側(▲)" "$expected" "$out"

  # --- 着地予測: 余る側(▼) -----------------------------------------------
  usage_f="$dir/u3.json"
  r3=$((NOW + 302400)) # 経過 50%(weekly 窓 604800s)
  mkusage "$usage_f" "$(
    printf '[{"kind":"weekly_scoped","group":"weekly","percent":38,"severity":"normal","resets_at":"%s","scope":{"model":{"id":null,"display_name":"Fable"}}}]' \
      "$(iso_at "$r3")"
  )"
  state_f="$dir/s3.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  # 着地=38/0.5=76 → ▼76%
  expected="Fable 38%→$(md_at "$r3") ▼76%"
  check "着地予測: 余る側(▼)" "$expected" "$out"

  # --- 着地予測: 境界(ちょうど 100%) ------------------------------------
  usage_f="$dir/u4.json"
  r4=$((NOW + 9000)) # 経過 50%(session)
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":50,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r4")"
  )"
  state_f="$dir/s4.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 50%→$(hm_at "$r4") ▲100%"
  check "着地予測: 境界(ちょうど 100% は ▲)" "$expected" "$out"

  # --- 序盤ガード: 経過 5% 未満は着地を伏せる ------------------------------
  usage_f="$dir/u5.json"
  r5=$((NOW + 17280)) # 経過 4%(session)
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":5,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r5")"
  )"
  state_f="$dir/s5.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 5%→$(hm_at "$r5")"
  check "序盤ガード: 経過 5% 未満は着地なし" "$expected" "$out"

  # --- 窓開始ちょうど(経過 0%): 除算エラーで他セグメントまで消えない -----
  usage_f="$dir/u6.json"
  r6a=$((NOW + 18000)) # session 窓開始ちょうど(経過 0%)
  r6b=$((NOW + 302400)) # weekly 経過 50%
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":5,"severity":"normal","resets_at":"%s","scope":null},{"kind":"weekly","group":"weekly","percent":20,"severity":"normal","resets_at":"%s"}]' \
      "$(iso_at "$r6a")" "$(iso_at "$r6b")"
  )"
  state_f="$dir/s6.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  # weekly: 着地=20/0.5=40 → ▼40%
  expected="5h 5%→$(hm_at "$r6a") · wk 20%→$(md_at "$r6b") ▼40%"
  check "窓開始ちょうど: 他セグメントは正常描画・着地は伏せる" "$expected" "$out"

  # --- resets_at が過去(経過 >100%): 着地なし ----------------------------
  usage_f="$dir/u7.json"
  r7=$((NOW - 100))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":50,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r7")"
  )"
  state_f="$dir/s7.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="5h 50%→$(hm_at "$r7")"
  check "resets_at が過去: 着地なし" "$expected" "$out"

  # --- 上限到達(percent>=100) --------------------------------------------
  usage_f="$dir/u8.json"
  r8=$((NOW + 3600))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":100,"severity":"normal","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r8")"
  )"
  state_f="$dir/s8.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="!5h 100%→$(hm_at "$r8")"
  check "上限到達(percent>=100): ! 表示・着地なし" "$expected" "$out"

  # --- 上限到達(severity ベース、percent<100) -----------------------------
  usage_f="$dir/u9.json"
  r9=$((NOW + 3600))
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":95,"severity":"blocked","resets_at":"%s","scope":null}]' \
      "$(iso_at "$r9")"
  )"
  state_f="$dir/s9.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="!5h 95%→$(hm_at "$r9")"
  check "上限到達(severity=blocked): ! 表示" "$expected" "$out"

  # --- weekly ラベルのフォールバック(display_name 欠落) ------------------
  usage_f="$dir/u10.json"
  r10=$((NOW + 600000)) # 経過 <5%(weekly 窓): 着地は伏せる、フォールバックのみ検証
  mkusage "$usage_f" "$(
    printf '[{"kind":"weekly_scoped","group":"weekly","percent":10,"severity":"normal","resets_at":"%s","scope":{"model":{"display_name":null}}}]' \
      "$(iso_at "$r10")"
  )"
  state_f="$dir/s10.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="wk 10%→$(md_at "$r10")"
  check "weekly_scoped: display_name 欠落は wk にフォールバック" "$expected" "$out"

  # --- 未知 kind: 着地を予測せず group にフォールバックしつつ描画を継続 ----
  usage_f="$dir/u11.json"
  r11=$((NOW + 3 * 86400))
  mkusage "$usage_f" "$(
    printf '[{"kind":"seven_day_opus","group":"opus_weekly","percent":33,"severity":"normal","resets_at":"%s"}]' \
      "$(iso_at "$r11")"
  )"
  state_f="$dir/s11.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="opus_weekly 33%→$(md_at "$r11")"
  check "未知 kind: 着地なし・group ラベルで描画される" "$expected" "$out"

  # --- percent/resets_at 欠落エントリはスキップ、他は描画継続 --------------
  usage_f="$dir/u12.json"
  r12=$((NOW + 590000)) # 経過 <5%(weekly 窓): 着地は伏せる
  mkusage "$usage_f" "$(
    printf '[{"kind":"session","group":"session","percent":null,"resets_at":"%s"},{"kind":"weekly","group":"weekly","percent":5,"severity":"normal","resets_at":"%s"}]' \
      "$(iso_at "$r12")" "$(iso_at "$r12")"
  )"
  state_f="$dir/s12.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  expected="wk 5%→$(md_at "$r12")"
  check "percent 欠落エントリはスキップ・他は描画" "$expected" "$out"

  # --- limits 空・欠落 → 空出力 --------------------------------------------
  usage_f="$dir/u13.json"
  printf '{"limits":[]}' >"$usage_f"
  state_f="$dir/s13.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "limits 空: 空出力" "" "$out"

  usage_f="$dir/u13b.json"
  printf '{}' >"$usage_f"
  state_f="$dir/s13b.json"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "limits キー欠落: 空出力" "" "$out"

  # --- 不正 JSON → 空出力、state file は変更しない -------------------------
  usage_f="$dir/u14.json"
  printf '{not valid json' >"$usage_f"
  state_f="$dir/s14.json"
  printf 'SENTINEL' >"$state_f"
  out="$("$self" __render "$usage_f" "$state_f" "$NOW")"
  check "不正 JSON: 空出力" "" "$out"
  check "不正 JSON: state file は書き換えない" "SENTINEL" "$(cat "$state_f")"

  # --- state file に予測用の series は残らない -----------------------------
  has_series="$(jq -r 'has("series")' "$dir/s1.json" 2>/dev/null)" || has_series='error'
  check "state file: series キーを持たない" "false" "$has_series"

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
