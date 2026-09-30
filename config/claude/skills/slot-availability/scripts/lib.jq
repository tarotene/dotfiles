# lib.jq — slot-hit.sh の共通関数。
#
# 日時計算は「ISO8601 のオフセット付き文字列を Unix epoch 秒に変換する」
# 純粋な算術のみで行い、IANA タイムゾーン DB のオフセット解決は bash 側
# (`TZ=<tz> date +%z`、slot-hit.sh 参照)に一度だけ委ねる。Google Calendar
# API の `dateTime` は既にオフセット付き文字列で返るため、イベント側は
# タイムゾーン DB を再解決する必要が無い。config 側のコマ時刻(HH:MM)だけ
# `local_tz_offset`(slot-hit.sh が埋め込む "+09:00" 形式)を使って同じ
# 関数でエポック秒に変換する。実測(2026-09-27、Python `datetime.fromisoformat
# (...).timestamp()` との比較)で完全一致を確認済み。

# "2026-10-18T18:00:00+09:00" や "...Z" → Unix epoch 秒(整数)。
def parse_offset_dt:
  . as $s
  | ($s[0:19] | strptime("%Y-%m-%dT%H:%M:%S") | mktime) as $naive_utc
  | ($s[19:]) as $offstr
  | (
      if $offstr == "Z" or $offstr == "" then 0
      else
        ($offstr[0:1]) as $sign
        | ($offstr[1:3] | tonumber) as $oh
        | ($offstr[4:6] | tonumber) as $om
        | (($oh * 3600 + $om * 60) * (if $sign == "-" then -1 else 1 end))
      end
    ) as $off_seconds
  | $naive_utc - $off_seconds;

# "YYYY-MM-DD" + "HH:MM" + tz_offset("+09:00") → Unix epoch 秒。
def local_dt_epoch(tz_offset):
  . as [$date, $time]
  | ($date + "T" + $time + ":00" + tz_offset) | parse_offset_dt;

# 終日イベントの date フィールド("YYYY-MM-DD" または
# "YYYY-MM-DDT00:00:00Z" — MCP コネクタの実測フォーマット)から
# "YYYY-MM-DD" だけを取り出す。
def all_day_date:
  .[0:10];

# "YYYY-MM-DD" を Unix epoch 秒(UTC 正午基準、日付演算専用 — 時刻比較には
# 使わない)に変換する。日数差の計算にのみ使う。
def date_to_epoch_days:
  (. + "T00:00:00Z") | fromdateiso8601 | (. / 86400 | floor);

def epoch_days_to_date:
  (. * 86400) | strftime("%Y-%m-%d");

# "HH:MM" → その日の 0 時からの分数。
def hm_to_minutes:
  split(":") | (.[0] | tonumber) * 60 + (.[1] | tonumber);

# jq 1.7 未満には `abs` 組み込みが無いため自前定義する(CI のランナー標準
# jq のバージョンに依存しないため)。
def jq_abs:
  if . < 0 then -. else . end;

def minutes_to_hm:
  (. / 60 | floor) as $h
  | (. % 60) as $m
  | ($h | tostring | if length == 1 then "0" + . else . end) + ":" +
    ($m | tostring | if length == 1 then "0" + . else . end);

# Unix epoch 秒 → その epoch が指すローカル日付("YYYY-MM-DD")。
# tz_offset_seconds を加算してから UTC として strftime する古典的な手法
# (IANA DB を再解決しない — config.tz_offset_seconds は bash 側が一度だけ
# `TZ=<tz> date +%z` で解決した固定オフセット)。
def epoch_to_local_date(tz_offset_seconds):
  (. + tz_offset_seconds) | strftime("%Y-%m-%d");

# 候補時刻(分)を config.slots(name -> {start,end} は "HH:MM")の
# どの区間に入れるか判定する。どの区間にも入らなければ、区間開始時刻との
# 差が最小の区間に丸める(slot-hit.py の match_slot と同じフォールバック)。
def match_slot(slots):
  . as $t_minutes
  | (slots | to_entries) as $entries
  | ($entries | map(select(
      ($t_minutes >= (.value.start | hm_to_minutes)) and
      ($t_minutes < (.value.end | hm_to_minutes))
    ))) as $hit
  | if ($hit | length) > 0 then $hit[0].key
    else
      ($entries | min_by(($t_minutes - (.value.start | hm_to_minutes)) | jq_abs)).key
    end;

# 曜日インデックス(月=0..日=6、slot-hit.py の WEEKDAY_JA と同じ規約)を
# gmtime の wday(日=0..土=6)から変換する。
def gmtime_wday_to_ja_index($wday):
  ($wday + 6) % 7;

# "YYYY-MM-DD" が構文・実在(2月30日等を弾く)ともに妥当か。
def valid_iso_date($s):
  (($s | type) == "string") and
  ($s | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}$")) and
  (
    ($s + "T00:00:00Z" | strptime("%Y-%m-%dT%H:%M:%SZ") | mktime | strftime("%Y-%m-%d")) == $s
  );

def valid_hm($s):
  (($s | type) == "string") and
  ($s | test("^([01][0-9]|2[0-3]):[0-5][0-9]$"));

# 候補日程一覧の入力契約({"date": "YYYY-MM-DD", "time": "HH:MM"})を検証
# する(slot-hit.py の parse_structured_candidates と同じ閉語彙の入力契約
# — 自由記述+事後 lint ではなく、型で弾く)。異常があれば error() で
# 即座に止める。
def validate_candidates($items):
  $items
  | map(
      if (valid_iso_date(.date // null)) and (valid_hm(.time // null)) then
        .
      else
        error(
          "候補日程の形式が想定外です(date は YYYY-MM-DD、time は HH:MM の" +
          "構造化 JSON が必要): " + (. | tostring)
        )
      end
    );

def slot_epoch_range($day; $slot_name; $config):
  ($config.slots[$slot_name].start | hm_to_minutes | minutes_to_hm) as $start_hm
  | ($config.slots[$slot_name].end | hm_to_minutes | minutes_to_hm) as $end_hm
  | {
      start: ([$day, $start_hm] | local_dt_epoch($config.tz_offset)),
      end: ([$day, $end_hm] | local_dt_epoch($config.tz_offset))
    };

def fmt_hm_epoch($epoch; $tz_offset_seconds):
  ($epoch + $tz_offset_seconds) | strftime("%H:%M");

def ja_weekday_char(idx):
  "月火水木金土日"[idx:idx+1];

# 年推定(infer-year の中核、slot-hit.py の resolve_year と同じ規約)。
# mktime は不正な日付(存在しない 2/30 等)をロールオーバーで正規化して
# しまう(Python の date() は ValueError で拒否する)ため、strftime で
# 逆変換して要求した年月日と一致するかを明示的に検証する。
def resolve_year($month; $day; $weekday_ja; $today):
  ($today | strptime("%Y-%m-%d") | mktime) as $today_epoch
  | ($today[0:4] | tonumber) as $today_year
  | (
      [range(0; 4)]
      | map(
          ($today_year + .) as $year
          | (
              ($year | tostring) + "-" +
              ($month | tostring | if length == 1 then "0" + . else . end) + "-" +
              ($day | tostring | if length == 1 then "0" + . else . end)
            ) as $ymd
          | ($ymd + "T00:00:00Z" | strptime("%Y-%m-%dT%H:%M:%SZ") | mktime) as $epoch
          | ($epoch | strftime("%Y-%m-%d")) as $actual_ymd
          | select($actual_ymd == $ymd)
          | select($epoch >= $today_epoch)
          | ($epoch | gmtime[6]) as $wday
          | select(ja_weekday_char(gmtime_wday_to_ja_index($wday)) == $weekday_ja)
          | $year
        )
    ) as $candidates
  | if ($candidates | length) > 0 then $candidates[0]
    else
      error(
        "\($month)/\($day)(\($weekday_ja)) — \($today) 以降で曜日が一致する年が" +
        "見つかりません(3年先まで探索)。候補日程一覧の表記を確認してください。"
      )
    end;

# 期間 [$from, $to](両端含む)× 設定の全コマを、judge の入力契約
# [{"date": "YYYY-MM-DD", "time": "HH:MM"}] として列挙する(候補一覧が
# 無く、自分のカレンダーの空きコマを数え上げて相手に提示するとき用)。
# 判定のロジックは持たない — 出力をそのまま judge に流せば、既存の
# ○/△/× 判定がそのまま使える(判定コアを 1 つに保つ)。time は各コマの
# 開始時刻で、match_slot がそのコマに解決する。
#   $weekdays: 空なら全曜日。それ以外は曜日番号(0=日〜6=土)で絞る。
#   $slots:    空なら全コマ。それ以外はコマ名で絞る(設定に無い名前はエラー)。
def enumerate_candidates($config; $from; $to; $weekdays; $slots):
  if (valid_iso_date($from) | not) or (valid_iso_date($to) | not) then
    error("--from / --to は実在する YYYY-MM-DD で指定してください")
  elif ($from | date_to_epoch_days) > ($to | date_to_epoch_days) then
    error("--from が --to より後です")
  elif (($to | date_to_epoch_days) - ($from | date_to_epoch_days)) > 400 then
    error("期間が 400 日を超えています(--from / --to を狭めてください)")
  elif any($weekdays[]; type != "number" or . < 0 or . > 6 or . != floor) then
    error("--weekdays は 0(日)〜6(土)の整数のカンマ区切りで指定してください")
  elif (($slots - ($config.slots | keys)) | length) > 0 then
    error("設定に無いコマ名です: \(($slots - ($config.slots | keys)) | join(","))")
  else
    ($config.slots | to_entries | sort_by(.value.start | hm_to_minutes)
      | map(select(($slots | length) == 0 or (.key as $k | $slots | index($k) != null)))) as $entries
    | [
        range(($from | date_to_epoch_days); ($to | date_to_epoch_days) + 1) as $n
        | select(($weekdays | length) == 0 or (($n + 4) % 7 | . as $w | $weekdays | index($w) != null))
        | ($n | epoch_days_to_date) as $d
        | $entries[]
        | {date: $d, time: (.value.start | hm_to_minutes | minutes_to_hm)}
      ]
  end;
