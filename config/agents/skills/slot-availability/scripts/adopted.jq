# adopted.jq — 取り込み済みの仮押さえの残り検査。
#
# 仮押さえ(marker_prefix で始まる予定)は、確定するまでの待ち時間を PR のマージ待ちにしない
# ために、主カレンダーへ即時に書く(ADR-0011 D6)。確定したら、確定内容を型付きデータへ
# 取り込み(`adopted_from` に、取り込んだカレンダーの予定の id を並べる)、射影された後に
# マーカーを消す。この「消す」が人手のままだと、確定済みのマーカーが主カレンダーに残り続け、
# 空き判定が実際には空いている枠を塞ぐ。この検査は、取り込み済みなのに残っているマーカーと、
# 同じ候補日程一覧から出した他の仮押さえ(確定したら一括で消す対象)を、判定の前に見つける。
#
# 入力(すべて --argjson):
#   $config  — config.toml を JSON 化し、tz_offset_seconds を足したもの。
#   $markers — 仮押さえを書き込むカレンダー(marker_calendar)の list_events の生レスポンス
#              ({"events": [...]})。
#   $adopted — 型付きデータの adopted_from に並ぶ予定の id の配列(文字列)。
#   $now     — いまの Unix epoch 秒(整数)。終了済みのマーカーは、判定を塞がないので数えない。
#
# 仮押さえに数える予定: 題名が marker_prefix または marker_prefix_aliases(旧印)で始まり、
# キャンセル済みでなく、終了していないもの。
# 同じ候補日程一覧: 説明欄の最初の URL が同じ予定(finalize と judge は、説明欄に候補日程一覧の
# URL を書く)。
#
# 出力: {"ok": bool, "stale": [...], "siblings": [...], "table": "..."}
#   ok は、取り込み済みなのに残っているマーカーが無いこと。siblings は stale が空でないときだけ出る。
include "lib";

def source_url($ev):
  (($ev.description // "") | capture("(?<u>https?://[^\\s)]+)")? | .u) // null;

def markers_in($markers; $config; $now):
  ($config.tz_offset_seconds) as $tz
  | marker_prefixes($config) as $mp
  | [
      ($markers.events // [])[]
      | select((.status // "") != "cancelled")
      | ((.summary // "") as $title | select(has_prefix($title; $mp)))
      | . as $ev
      | range_of($ev; $tz) as $r
      | select($r != null and $r.end > $now)
      | {id: ($ev.id // ""), summary: ($ev.summary // "(題名なし)"), start: $r.start, end: $r.end,
         all_day: $r.all_day, url: source_url($ev)}
    ]
  | sort_by(.start);

def line($m; $tz):
  "  - " + ($m.start | epoch_to_local_date($tz)) + " "
  + (if $m.all_day then "終日" else (fmt_hm_epoch($m.start; $tz) + "–" + fmt_hm_epoch($m.end; $tz)) end)
  + "  " + $m.summary;

def render($stale; $siblings; $tz):
  if ($stale | length) == 0 then
    "仮押さえ: 取り込み済みなのに残っているものはありません。"
  else
    ("取り込み済みなのに、仮押さえの印の予定が主カレンダーに \($stale | length) 件残っています。"
     + "確定した予定は型付きデータにあるので、カレンダーのマーカーは消す対象です"
     + "(削除は本人が決めてください):\n")
    + ($stale | map(line(.; $tz)) | join("\n"))
    + (if ($siblings | length) == 0 then ""
       else
         ("\n\n同じ候補日程一覧から出した他の仮押さえ \($siblings | length) 件も、確定したので一括で消す対象です:\n")
         + ($siblings | map(line(.; $tz)) | join("\n"))
       end)
  end;

markers_in($markers; $config; $now) as $all
| ($adopted | map(select(type == "string")) | unique) as $ids
| [$all[] | select(.id as $i | $ids | index($i))] as $stale
| ([$stale[] | .url | select(. != null)] | unique) as $urls
| [$all[]
   | select((.id as $i | $ids | index($i)) | not)
   | select(.url != null and (.url as $u | $urls | index($u)))] as $siblings
| {ok: ($stale | length == 0), stale: $stale, siblings: $siblings,
   table: render($stale; $siblings; $config.tz_offset_seconds)}
