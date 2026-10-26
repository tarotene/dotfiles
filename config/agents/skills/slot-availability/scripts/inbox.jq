# inbox.jq — 受付箱検査(#768)。
#
# 主カレンダーを「手入力の受付箱」とみなし、型付きデータへ取り込まれていない予定
# (取り込み待ち)が残っていれば、空き判定を止める。取り込み待ちがあるうちに判定すると、
# まだ型付きデータに無い予定を見落として、重なる候補日程を ○ と答えてしまう。
#
# 入力(すべて --argjson):
#   $config — config.toml を JSON 化し、tz_offset_seconds を足したもの。`inbox.exclude_prefixes`
#             (除外する接頭辞の配列)は省略できる。
#   $events — 受付箱のカレンダーの list_events の生レスポンス({"events": [...]})。
#   $now    — いまの Unix epoch 秒(整数)。終了済みの予定を数えないために使う。
#
# 取り込み待ちに数えない予定:
#   - 印の付いたもの: marker_prefix・marker_prefix_aliases(旧印)・inbox.exclude_prefixes。
#     別の仕組みが招待者なしで直接書く予定(例: `[outing] `)も、設定の除外接頭辞に並べる。
#   - キャンセル済み、終了済み(end <= now)。
#   - 他人が主催の招待(organizer.self が false)。主催者の情報が無い予定は、手入力とみなして
#     数える(取りこぼしを避ける側に倒す)。
#
# 出力: {"ok": bool, "pending": [{start, end, all_day, summary}], "table": "..."}
include "lib";

def exclude_prefixes($config):
  marker_prefixes($config) + ($config.inbox.exclude_prefixes // []);

def pending_events($events; $config; $now):
  ($config.tz_offset_seconds) as $tz
  | exclude_prefixes($config) as $excluded
  | [
      ($events.events // [])[]
      | select((.status // "") != "cancelled")
      | select((((.organizer // {}) | .self) != false))
      | ((.summary // "") as $title | select(has_prefix($title; $excluded) | not))
      | range_of(.; $tz) as $r
      | select($r != null and $r.end > $now)
      | {start: $r.start, end: $r.end, all_day: $r.all_day, summary: (.summary // "(題名なし)")}
    ]
  | sort_by(.start);

def render($pending; $tz):
  if ($pending | length) == 0 then
    "受付箱: 取り込み待ちの予定はありません。"
  else
    ("受付箱に取り込み待ちの予定が \($pending | length) 件あります。空き判定の前に、型付きデータへ取り込むか、"
     + "除外の印を付けるかを、本人に確認してください:\n")
    + ($pending | map(
        "  - " + (.start | epoch_to_local_date($tz)) + " "
        + (if .all_day then "終日" else (fmt_hm_epoch(.start; $tz) + "–" + fmt_hm_epoch(.end; $tz)) end)
        + "  " + .summary
      ) | join("\n"))
  end;

pending_events($events; $config; $now) as $pending
| {ok: ($pending | length == 0), pending: $pending, table: render($pending; $config.tz_offset_seconds)}
