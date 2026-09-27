# finalize.jq — 裁定後の確定化(slot-hit.py の resolve_decided/
# parse_own_markers/build_finalize_plan/render_finalize_table を移植)。
#
# 入力(すべて --argjson / --arg):
#   $config, $decided(judge の --candidates と同じ入力契約)、
#   $markers({"events": [...]} — list_events の生レスポンス)、
#   $event_title, $source_url, $today, $venue
#
# 出力: {"table": "...", "plan": {"update": [...], "create": [...], "delete": [...]}}
include "lib";

def resolve_decided($items; $config):
  validate_candidates($items)
  | map(
      . as $c
      | ($c.time | hm_to_minutes | match_slot($config.slots)) as $slot_name
      | slot_epoch_range($c.date; $slot_name; $config) as {start: $s, end: $e}
      | {day: $c.date, slot_name: $slot_name, start: $s, end: $e}
    );

def parse_own_markers($markers_data; $config; $source_url):
  [
    ($markers_data.events // [])[]
    | select((.status // "") != "cancelled")
    | select((.summary // "") | startswith($config.marker_prefix))
    | select($source_url == "" or ((.description // "") | contains($source_url)))
    | select(.start.dateTime != null and .end.dateTime != null)
    | {
        id: .id,
        start: (.start.dateTime | parse_offset_dt),
        end: (.end.dateTime | parse_offset_dt),
        calendar: $config.marker_calendar,
        title: .summary
      }
  ];

def epoch_iso($epoch; $tz_offset_seconds; $tz_offset):
  ($epoch + $tz_offset_seconds | strftime("%Y-%m-%dT%H:%M:%S")) + $tz_offset;

# own_markers から、区間が重なる最初の1件を取り出す(無ければ null)。
def find_overlap($d; $markers):
  ($markers | map(select(.start < $d.end and .end > $d.start)) | .[0]) // null;

# 戻り値: {plan: {update, create, delete}, rows: [...](表示用)}
def build_finalize_plan(
  $decided; $own_markers; $event_title; $marker_calendar; $source_url;
  $today; $venue; $tz_offset_seconds; $tz_offset
):
  (
    [
      "場所: " + $venue + "(" + $today + " 時点)",
      "裁定: " + $today + "(主催者側で確定)"
    ] + (if $source_url != "" then ["候補日程一覧: " + $source_url] else [] end)
    | join("\n")
  ) as $description
  | reduce $decided[] as $d (
      {pool: $own_markers, update: [], create: [], rows: []};
      find_overlap($d; .pool) as $match
      | if $match != null then
          .pool |= map(select(.id != $match.id))
          | .update += [{
              id: $match.id, calendar: $match.calendar, title: $event_title,
              start: epoch_iso($d.start; $tz_offset_seconds; $tz_offset),
              end: epoch_iso($d.end; $tz_offset_seconds; $tz_offset),
              description: $description
            }]
          | .rows += [{
              day: $d.day, slot_name: $d.slot_name, start: $d.start, end: $d.end,
              op: "update", marker: $match
            }]
        else
          .create += [{
            calendar: $marker_calendar, title: $event_title,
            start: epoch_iso($d.start; $tz_offset_seconds; $tz_offset),
            end: epoch_iso($d.end; $tz_offset_seconds; $tz_offset),
            description: $description
          }]
          | .rows += [{
              day: $d.day, slot_name: $d.slot_name, start: $d.start, end: $d.end,
              op: "create", marker: null
            }]
        end
    )
  | (.pool | map({
      id: .id, calendar: .calendar,
      start: epoch_iso(.start; $tz_offset_seconds; $tz_offset),
      end: epoch_iso(.end; $tz_offset_seconds; $tz_offset),
      title: .title
    })) as $deletes
  | {
      plan: {update: .update, create: .create, delete: $deletes},
      rows: .rows,
      deletes_for_table: $deletes
    };

def render_finalize_table($rows; $deletes; $tz_offset_seconds):
  (
    $rows
    | map(
        (
          if .marker == null then "-"
          else
            .marker.title + " " +
            fmt_hm_epoch(.marker.start; $tz_offset_seconds) + "–" +
            fmt_hm_epoch(.marker.end; $tz_offset_seconds)
          end
        ) as $cell
        | "| \(.day) " + fmt_hm_epoch(.start; $tz_offset_seconds) + "–" +
          fmt_hm_epoch(.end; $tz_offset_seconds) + "(\(.slot_name)) | \(.op) | \($cell) |"
      )
  ) as $rowlines
  | ($deletes | map("| — | delete | \(.title) \(.start)–\(.end) |")) as $deletelines
  | ($rows | map(select(.op == "update")) | length) as $u
  | ($rows | map(select(.op == "create")) | length) as $c
  | ($deletes | length) as $d
  | "update:\($u) create:\($c) delete:\($d)\n\n| 裁定枠 | 操作 | 対象マーカー |\n|---|---|---|\n" +
    (($rowlines + $deletelines) | join("\n"));

# ── main ──
resolve_decided($decided; $config) as $decided_resolved
| parse_own_markers($markers; $config; $source_url) as $own_markers
| build_finalize_plan(
    $decided_resolved; $own_markers; $event_title; $config.marker_calendar; $source_url;
    $today; $venue; $config.tz_offset_seconds; $config.tz_offset
  ) as $result
| {
    table: render_finalize_table($result.rows; $result.deletes_for_table; $config.tz_offset_seconds),
    plan: $result.plan
  }
