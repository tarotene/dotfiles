# judge.jq — 候補日程 × カレンダーの当たり判定(slot-hit.py の
# classify_events/judge/build_plan/render_table を移植)。
#
# 入力(すべて --argjson):
#   $config     — config.toml を yq で JSON 化し、tz_offset("+09:00")と
#                 tz_offset_seconds(整数)を bash 側が追加したもの。
#   $candidates — [{"date": "YYYY-MM-DD", "time": "HH:MM"}, ...]
#   $events     — [{"calendar_id", "calendar_summary", "ev": <生イベント>}, ...]
#                 (events-dir 配下の全カレンダーを bash 側でフラット化)
#   $event_title, $source_url — 文字列。
#
# 出力: {"table": "...", "plan": {"markers": [...], "answers": [...]}}
include "lib";

def classify_events($events; $config; $source_url):
  reduce $events[] as $item (
    {busies: [], soft_markers: [], soft_days: [], validated_days: []};
    $item.ev as $ev
    | $item.calendar_summary as $cal_name
    | if ($ev.status // "") == "cancelled" then
        .
      elif ($ev.start.date != null) then
        ($ev.start.date | all_day_date) as $d0
        | (
            if $ev.end.date != null then ($ev.end.date | all_day_date)
            else (($d0 | date_to_epoch_days) + 1) | epoch_days_to_date
            end
          ) as $d1
        | ($ev.summary // "") as $title
        | if any($config.ignore_prefixes[]?; . as $p | $title | startswith($p)) then
            .
          elif any($config.soft_day_prefixes[]?; . as $p | $title | startswith($p)) then
            .soft_days += [
              range(($d0 | date_to_epoch_days); ($d1 | date_to_epoch_days)) | epoch_days_to_date
            ]
          else
            .
          end
      elif ($ev.start.dateTime != null and $ev.end.dateTime != null) then
        ($ev.start.dateTime | parse_offset_dt) as $s
        | ($ev.end.dateTime | parse_offset_dt) as $e
        | ($ev.summary // "") as $title
        | if ($ev.transparency // "") == "transparent" then
            .
          elif ($title | startswith($config.marker_prefix)) then
            if ($source_url != "" and (($ev.description // "") | contains($source_url))) then
              .
            else
              .soft_markers += [{start: $s, end: $e, calendar: $cal_name, summary: $title}]
            end
          else
            .busies += [{
              start: $s, end: $e, calendar: $cal_name,
              summary: (if $title == "" then "(非公開)" else $title end)
            }]
            | ($s | epoch_to_local_date($config.tz_offset_seconds) | date_to_epoch_days) as $sd
            | ($e | epoch_to_local_date($config.tz_offset_seconds) | date_to_epoch_days) as $ed
            | .validated_days += [range($sd; $ed + 1) | epoch_days_to_date]
          end
      else
        .
      end
  )
  | .soft_days |= unique
  | .validated_days |= unique;

# 検証のみを行い、問題が無ければ null を返す(呼び出し側は `as $_` で
# 受けて後続処理を続ける)。jq の `empty` をパイプの途中で返すと、それ
# 以降のパイプ全体が空ストリームになり後続処理が一切実行されなくなる
# ため、意図的に `empty` を避けている。
def validate_soft_days($soft_days; $validated_days):
  ($soft_days - $validated_days) as $missing
  | if ($missing | length) > 0 then
      error(
        "以下の日は終日イベント(soft_day_prefixes 一致)だけが登録されて" +
        "おり、時間指定の確定予定がありません。実際の拘束時間を一次情報で" +
        "確認し、Google Calendar に時間指定の予定として書き戻してから" +
        "再実行してください: " + ($missing | join(", "))
      )
    else
      null
    end;

def judge_one($day; $slot_name; $config; $busies; $soft_markers; $soft_days):
  slot_epoch_range($day; $slot_name; $config) as {start: $s0, end: $s1}
  | ($config.buffer_minutes * 60) as $buf
  | ($busies | map(select(.start < $s1 and .end > $s0))) as $overlaps
  | if ($overlaps | length) > 0 then
      {
        verdict: "×",
        reason: (
          $overlaps
          | map(
              .calendar + ": " + .summary + " " +
              fmt_hm_epoch(.start; $config.tz_offset_seconds) + "–" +
              fmt_hm_epoch(.end; $config.tz_offset_seconds)
            )
          | join("; ")
        )
      }
    else
      (
        $busies
        | map(
            if (.end <= $s0 and ($s0 - .end) <= $buf) then
              .calendar + ": " + .summary + " が" + (($s0 - .end) / 60 | floor | tostring) + "分前に終了"
            elif (.start >= $s1 and (.start - $s1) <= $buf) then
              .calendar + ": " + .summary + " が" + ((.start - $s1) / 60 | floor | tostring) + "分後に開始"
            else
              null
            end
          )
        | map(select(. != null))
      ) as $near
      | if ($near | length) > 0 then
          {verdict: "△", reason: ($near | join("; "))}
        else
          ($soft_markers | map(select(.start < $s1 and .end > $s0))) as $sm
          | if ($sm | length) > 0 then
              {
                verdict: "△",
                reason: "別候補日程のマーカーと重複: " + ($sm | map(.calendar + ": " + .summary) | join("; "))
              }
            elif ($soft_days | index($day) != null) then
              {verdict: "△", reason: "試験日等のため保守的に判定(直接の重複・近接予定なし)"}
            else
              {verdict: "○", reason: ""}
            end
        end
    end;

def build_plan($candidates_resolved; $judgments; $config; $event_title; $source_url):
  {
    markers: (
      [range(0; $candidates_resolved | length)]
      | map(
          $candidates_resolved[.] as $cand
          | $judgments[.] as $j
          | select($j.verdict != "×")
          | slot_epoch_range($cand.date; $cand.slot_name; $config) as {start: $s, end: $e}
          | ($config.tz_offset) as $tzoff
          | {
              title: (
                $config.marker_prefix + $event_title +
                (if $j.verdict == "△" then " [△]" else "" end)
              ),
              start: (($s + $config.tz_offset_seconds) | strftime("%Y-%m-%dT%H:%M:%S")) + $tzoff,
              end: (($e + $config.tz_offset_seconds) | strftime("%Y-%m-%dT%H:%M:%S")) + $tzoff,
              description: (
                [
                  (if $source_url != "" then "候補日程一覧: " + $source_url else "候補日程一覧より仮押さえ" end)
                ] + (if $j.reason != "" then ["判定理由: " + $j.reason] else [] end)
                | join("\n")
              ),
              calendar: $config.marker_calendar
            }
        )
    ),
    answers: ($judgments | map(.verdict))
  };

def render_table($candidates_resolved; $judgments):
  (
    [range(0; $candidates_resolved | length)]
    | map(
        $candidates_resolved[.] as $cand
        | $judgments[.] as $j
        | (if $j.reason == "" then "-" else $j.reason end) as $reason_cell
        | "| \($cand.date) \($cand.time) | \($cand.slot_name) | \($j.verdict) | \($reason_cell) |"
      )
  ) as $rows
  | ($judgments | map(select(.verdict == "○")) | length) as $ok
  | ($judgments | map(select(.verdict == "△")) | length) as $warn
  | ($judgments | map(select(.verdict == "×")) | length) as $ng
  | "○:\($ok) △:\($warn) ×:\($ng)\n\n| 日程 | コマ | 判定 | 理由 |\n|---|---|---|---|\n" + ($rows | join("\n"));

# ── main ──
(validate_candidates($candidates) | map(. + {slot_name: (.time | hm_to_minutes | match_slot($config.slots))})) as $candidates_resolved
| classify_events($events; $config; $source_url) as {busies: $busies, soft_markers: $soft_markers, soft_days: $soft_days, validated_days: $validated_days}
| validate_soft_days($soft_days; $validated_days) as $_
| ($candidates_resolved | map(judge_one(.date; .slot_name; $config; $busies; $soft_markers; $soft_days))) as $judgments
| {
    table: render_table($candidates_resolved; $judgments),
    plan: build_plan($candidates_resolved; $judgments; $config; $event_title; $source_url)
  }
