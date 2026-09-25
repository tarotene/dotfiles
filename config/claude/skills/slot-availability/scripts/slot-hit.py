#!/usr/bin/env python3
"""slot-hit.py — 候補日程一覧(調整さん等)と Google Calendar の予定を
突き合わせ、朝・昼・夜の3コマ単位で当たり判定(○/△/×)を出す純関数。

ネットワークアクセスもファイル書き込み(出力先を除く)も行わない。
Google Calendar の読み書きは呼び出し側(Claude の MCP 呼び出し)が担う —
このスクリプトは「list_events の生レスポンス」と「候補日程一覧の文字列
配列」を入力に取り、判定表とマーカー作成計画(JSON)を出力するだけ。

標準ライブラリのみで動く(python3.11+、tomllib を使うため)。

設定ファイル(TOML)のスキーマ:

    timezone = "Asia/Tokyo"
    buffer_minutes = 60
    marker_prefix = "【調整中】"
    marker_calendar = "tarotene@gmail.com"

    [slots]
    morning = { start = "09:00", end = "12:00" }
    noon    = { start = "13:00", end = "17:00" }
    evening = { start = "18:00", end = "21:00" }

    [all_day]
    ignore_prefixes   = ["【勉強期間】"]
    soft_day_prefixes = ["【試験本番】"]

    [[calendars]]
    id = "tarotene@gmail.com"
    summary = "本体"

候補日程の文字列形式: "M/D(曜) H:MM〜"(調整さんの表記そのまま)。年は
明記されないため、今日以降で最初に曜日が一致する年を採用する。

終日イベントのうち `soft_day_prefixes` に一致するもの(例:【試験本番】)
は、同じ日に時間指定の確定予定が1件も無いとエラー終了する — 「拘束時間が
まだ Google Calendar に正規化されていない」ことを検出のみで済ませず、
実行を止めて知らせるため。該当日に時間指定の予定があれば、その日の
非重複コマは(直接の重複・近接が無くても)△ に丸める("試験日はある程度
余力を見て判定してほしい"という運用裁定による)。
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass, field
from datetime import date, datetime, time, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

WEEKDAY_JA = "月火水木金土日"  # index == datetime.weekday() (Mon=0)

CANDIDATE_RE = re.compile(
    r"^(?P<m>\d{1,2})/(?P<d>\d{1,2})\((?P<wd>[月火水木金土日])\)\s*"
    r"(?P<h>\d{1,2}):(?P<mi>\d{2})〜\s*$"
)


class SlotHitError(Exception):
    """設定不備・未正規化データなど、ユーザーの裁定を要するエラー。"""


@dataclass
class Slot:
    name: str
    start: time
    end: time


@dataclass
class Config:
    tz: ZoneInfo
    buffer_minutes: int
    marker_prefix: str
    marker_calendar: str
    slots: dict[str, Slot]
    ignore_prefixes: list[str]
    soft_day_prefixes: list[str]
    calendars: list[dict]

    @classmethod
    def load(cls, path: Path) -> "Config":
        raw = tomllib.loads(path.read_text(encoding="utf-8"))
        return cls.from_dict(raw)

    @classmethod
    def from_dict(cls, raw: dict) -> "Config":
        slots = {
            name: Slot(name, _parse_hm(v["start"]), _parse_hm(v["end"]))
            for name, v in raw["slots"].items()
        }
        all_day = raw.get("all_day", {})
        return cls(
            tz=ZoneInfo(raw["timezone"]),
            buffer_minutes=int(raw["buffer_minutes"]),
            marker_prefix=raw["marker_prefix"],
            marker_calendar=raw["marker_calendar"],
            slots=slots,
            ignore_prefixes=list(all_day.get("ignore_prefixes", [])),
            soft_day_prefixes=list(all_day.get("soft_day_prefixes", [])),
            calendars=list(raw.get("calendars", [])),
        )


def _parse_hm(s: str) -> time:
    h, m = s.split(":")
    return time(int(h), int(m))


@dataclass
class Busy:
    start: datetime
    end: datetime
    calendar: str
    summary: str


@dataclass
class SoftMarker:
    start: datetime
    end: datetime
    calendar: str
    summary: str


@dataclass
class Candidate:
    raw: str
    day: date
    slot_time: time
    slot_name: str


def resolve_year(month: int, day: int, weekday_ja: str, today: date) -> int:
    """今日以降で最初に曜日が一致する年を返す。3年先まで見つからなければ
    入力そのものが壊れている(構造的にありえない)とみなしエラーにする。"""
    for delta in range(0, 4):
        year = today.year + delta
        try:
            d = date(year, month, day)
        except ValueError:
            continue
        if d >= today and WEEKDAY_JA[d.weekday()] == weekday_ja:
            return year
    raise SlotHitError(
        f"{month}/{day}({weekday_ja}) — {today} 以降で曜日が一致する年が"
        f"見つかりません(3年先まで探索)。候補日程一覧の表記を確認してください。"
    )


def match_slot(t: time, slots: dict[str, Slot]) -> str:
    for name, slot in slots.items():
        if slot.start <= t < slot.end:
            return name
    # フォールバック: どのコマ区間にも入らない開始時刻は、区間開始時刻との
    # 差が最小のコマに丸める。
    best = min(
        slots.items(),
        key=lambda kv: abs(
            (datetime.combine(date.min, t) - datetime.combine(date.min, kv[1].start))
        ),
    )
    return best[0]


def parse_candidates(texts: list[str], today: date) -> list[Candidate]:
    out = []
    for text in texts:
        m = CANDIDATE_RE.match(text)
        if not m:
            raise SlotHitError(f"候補日程の形式が想定外です: {text!r}")
        month, day, wd = int(m["m"]), int(m["d"]), m["wd"]
        t = time(int(m["h"]), int(m["mi"]))
        year = resolve_year(month, day, wd, today)
        d = date(year, month, day)
        out.append(Candidate(text, d, t, ""))
    return out


def calendar_summary(calendars: list[dict], cal_id: str) -> str:
    for c in calendars:
        if c.get("id") == cal_id:
            return c.get("summary", cal_id)
    return cal_id


def load_events_by_calendar(events_dir: Path, calendars: list[dict]) -> dict[str, list[dict]]:
    result: dict[str, list[dict]] = {}
    for cal in calendars:
        cal_id = cal["id"]
        f = events_dir / f"{cal_id}.json"
        if not f.exists():
            print(
                f"warning: {cal_id}({cal.get('summary', '')}) の events ファイルが"
                f"見つかりません({f})。このカレンダーは判定から除外されます。",
                file=sys.stderr,
            )
            continue
        data = json.loads(f.read_text(encoding="utf-8"))
        result[cal_id] = data.get("events", [])
    return result


def classify_events(
    events_by_cal: dict[str, list[dict]],
    config: Config,
    source_url: str,
) -> tuple[list[Busy], list[SoftMarker], set[date], set[date]]:
    busies: list[Busy] = []
    soft_markers: list[SoftMarker] = []
    soft_days: set[date] = set()
    validated_days: set[date] = set()

    for cal_id, events in events_by_cal.items():
        cal_name = calendar_summary(config.calendars, cal_id)
        for ev in events:
            if ev.get("status") == "cancelled":
                continue
            start = ev.get("start", {})
            end = ev.get("end", {})
            title = ev.get("summary", "") or ""

            if "date" in start:
                d0 = _parse_all_day_date(start["date"])
                d1 = _parse_all_day_date(end["date"]) if "date" in end else d0 + timedelta(days=1)
                if any(title.startswith(p) for p in config.ignore_prefixes):
                    continue
                if any(title.startswith(p) for p in config.soft_day_prefixes):
                    d = d0
                    while d < d1:
                        soft_days.add(d)
                        d += timedelta(days=1)
                    continue
                # それ以外の終日イベントは既定で無視(裁定)。
                continue

            if "dateTime" not in start or "dateTime" not in end:
                continue

            s = datetime.fromisoformat(start["dateTime"])
            e = datetime.fromisoformat(end["dateTime"])
            if ev.get("transparency") == "transparent":
                continue

            if title.startswith(config.marker_prefix):
                description = ev.get("description", "") or ""
                if source_url and source_url in description:
                    continue  # 今回の自分のマーカー(再実行時)は無視
                soft_markers.append(SoftMarker(s, e, cal_name, title))
                continue

            busies.append(Busy(s, e, cal_name, title or "(非公開)"))
            d = s.date()
            while d <= e.date():
                validated_days.add(d)
                d += timedelta(days=1)

    return busies, soft_markers, soft_days, validated_days


def validate_soft_days(soft_days: set[date], validated_days: set[date]) -> None:
    missing = sorted(soft_days - validated_days)
    if missing:
        dates = ", ".join(d.isoformat() for d in missing)
        raise SlotHitError(
            "以下の日は終日イベント(soft_day_prefixes 一致)だけが登録されて"
            "おり、時間指定の確定予定がありません。実際の拘束時間を一次情報で"
            f"確認し、Google Calendar に時間指定の予定として書き戻してから"
            f"再実行してください: {dates}"
        )


def _parse_all_day_date(s: str) -> date:
    """終日イベントの `date` フィールドを解釈する。Google Calendar API 本来の
    `YYYY-MM-DD` 形式と、一部の MCP コネクタが返す `YYYY-MM-DDT00:00:00Z`
    形式の両方を許容する(実測: claude.ai Google Calendar コネクタは後者)。"""
    return date.fromisoformat(s[:10])


def fmt_hm(dt: datetime) -> str:
    return dt.strftime("%H:%M")


def judge(
    day: date,
    slot_name: str,
    config: Config,
    busies: list[Busy],
    soft_markers: list[SoftMarker],
    soft_days: set[date],
) -> tuple[str, str]:
    slot = config.slots[slot_name]
    s0 = datetime.combine(day, slot.start, config.tz)
    s1 = datetime.combine(day, slot.end, config.tz)
    buf = timedelta(minutes=config.buffer_minutes)

    overlaps = [b for b in busies if b.start < s1 and b.end > s0]
    if overlaps:
        reasons = [f"{b.calendar}: {b.summary} {fmt_hm(b.start)}–{fmt_hm(b.end)}" for b in overlaps]
        return "×", "; ".join(reasons)

    near: list[str] = []
    for b in busies:
        if b.end <= s0 and (s0 - b.end) <= buf:
            near.append(f"{b.calendar}: {b.summary} が{int((s0 - b.end).total_seconds() // 60)}分前に終了")
        elif b.start >= s1 and (b.start - s1) <= buf:
            near.append(f"{b.calendar}: {b.summary} が{int((b.start - s1).total_seconds() // 60)}分後に開始")
    if near:
        return "△", "; ".join(near)

    sm = [m for m in soft_markers if m.start < s1 and m.end > s0]
    if sm:
        reasons = [f"{m.calendar}: {m.summary}" for m in sm]
        return "△", "別候補日程のマーカーと重複: " + "; ".join(reasons)

    if day in soft_days:
        return "△", "試験日等のため保守的に判定(直接の重複・近接予定なし)"

    return "○", ""


def build_plan(
    candidates: list[Candidate],
    judgments: list[tuple[str, str]],
    config: Config,
    event_title: str,
    source_url: str,
) -> dict:
    markers = []
    for cand, (verdict, reason) in zip(candidates, judgments):
        if verdict == "×":
            continue
        slot = config.slots[cand.slot_name]
        start_dt = datetime.combine(cand.day, slot.start, config.tz)
        end_dt = datetime.combine(cand.day, slot.end, config.tz)
        suffix = " [△]" if verdict == "△" else ""
        description_lines = [f"候補日程一覧: {source_url}" if source_url else "候補日程一覧より仮押さえ"]
        if reason:
            description_lines.append(f"判定理由: {reason}")
        markers.append(
            {
                "title": f"{config.marker_prefix}{event_title}{suffix}",
                "start": start_dt.isoformat(),
                "end": end_dt.isoformat(),
                "description": "\n".join(description_lines),
                "calendar": config.marker_calendar,
            }
        )
    return {
        "markers": markers,
        "answers": [v for v, _ in judgments],
    }


def render_table(candidates: list[Candidate], judgments: list[tuple[str, str]]) -> str:
    lines = ["| 日程 | コマ | 判定 | 理由 |", "|---|---|---|---|"]
    for cand, (verdict, reason) in zip(candidates, judgments):
        lines.append(f"| {cand.raw} | {cand.slot_name} | {verdict} | {reason or '-'} |")
    counts = {"○": 0, "△": 0, "×": 0}
    for v, _ in judgments:
        counts[v] += 1
    summary = f"○:{counts['○']} △:{counts['△']} ×:{counts['×']}"
    return summary + "\n\n" + "\n".join(lines)


def run(
    config_path: Path,
    candidates_path: Path,
    events_dir: Path,
    today: date,
    event_title: str,
    source_url: str,
    out_plan: Path | None,
) -> str:
    config = Config.load(config_path)
    candidate_texts = json.loads(candidates_path.read_text(encoding="utf-8"))
    candidates = parse_candidates(candidate_texts, today)
    for c in candidates:
        c.slot_name = match_slot(c.slot_time, config.slots)

    events_by_cal = load_events_by_calendar(events_dir, config.calendars)
    busies, soft_markers, soft_days, validated_days = classify_events(events_by_cal, config, source_url)
    validate_soft_days(soft_days, validated_days)

    judgments = [judge(c.day, c.slot_name, config, busies, soft_markers, soft_days) for c in candidates]

    plan = build_plan(candidates, judgments, config, event_title, source_url)
    if out_plan:
        out_plan.write_text(json.dumps(plan, ensure_ascii=False, indent=2), encoding="utf-8")

    table = render_table(candidates, judgments)
    return table + "\n\n---\nplan.json:\n" + json.dumps(plan, ensure_ascii=False, indent=2)


# ---------------------------------------------------------------------------
# 自己検査
# ---------------------------------------------------------------------------

def _test_config() -> Config:
    return Config.from_dict(
        {
            "timezone": "Asia/Tokyo",
            "buffer_minutes": 60,
            "marker_prefix": "【調整中】",
            "marker_calendar": "primary",
            "slots": {
                "morning": {"start": "09:00", "end": "12:00"},
                "noon": {"start": "13:00", "end": "17:00"},
                "evening": {"start": "18:00", "end": "21:00"},
            },
            "all_day": {
                "ignore_prefixes": ["【勉強期間】"],
                "soft_day_prefixes": ["【試験本番】"],
            },
            "calendars": [{"id": "primary", "summary": "本体"}],
        }
    )


def _ev_timed(start: str, end: str, summary: str = "予定", transparency: str | None = None, description: str = "") -> dict:
    ev = {
        "status": "confirmed",
        "start": {"dateTime": start, "timeZone": "Asia/Tokyo"},
        "end": {"dateTime": end, "timeZone": "Asia/Tokyo"},
        "summary": summary,
        "description": description,
    }
    if transparency:
        ev["transparency"] = transparency
    return ev


def _ev_allday(start_date: str, end_date: str, summary: str) -> dict:
    return {
        "status": "confirmed",
        "start": {"date": start_date},
        "end": {"date": end_date},
        "summary": summary,
    }


def run_selftest() -> None:
    tz = ZoneInfo("Asia/Tokyo")
    config = _test_config()

    # 1) 19:00 は evening コマ区間 [18:00,21:00) に含まれる。
    assert match_slot(time(19, 0), config.slots) == "evening"
    assert match_slot(time(9, 0), config.slots) == "morning"
    assert match_slot(time(13, 0), config.slots) == "noon"

    # 2) 年跨ぎ: 1/10 は今日(2026-09-25)以降で最初に曜日が一致する年を選ぶ。
    today = date(2026, 9, 25)
    target_weekday = WEEKDAY_JA[date(2027, 1, 10).weekday()]
    assert resolve_year(1, 10, target_weekday, today) == 2027

    # 3) バッファ境界: gap 60分 -> △、gap 61分 -> ○。
    busies = [Busy(datetime(2026, 10, 25, 9, 0, tzinfo=tz), datetime(2026, 10, 25, 17, 0, tzinfo=tz), "本体", "全奏")]
    v, _ = judge(date(2026, 10, 25), "evening", config, busies, [], set())
    assert v == "△", v
    busies61 = [Busy(datetime(2026, 10, 25, 9, 0, tzinfo=tz), datetime(2026, 10, 25, 16, 59, tzinfo=tz), "本体", "全奏")]
    v, _ = judge(date(2026, 10, 25), "evening", config, busies61, [], set())
    assert v == "○", v

    # 4) 直接重なり -> ×。
    busies_overlap = [Busy(datetime(2026, 10, 18, 6, 0, tzinfo=tz), datetime(2026, 10, 18, 21, 0, tzinfo=tz), "本体", "全奏")]
    v, _ = judge(date(2026, 10, 18), "evening", config, busies_overlap, [], set())
    assert v == "×", v

    # 5) soft_day で時間指定の確定予定が無ければ検証エラー。
    try:
        validate_soft_days({date(2026, 11, 15)}, set())
        raise AssertionError("should have raised")
    except SlotHitError:
        pass
    # 確定予定があれば通過。
    validate_soft_days({date(2026, 11, 15)}, {date(2026, 11, 15)})

    # 6) soft_day 上限: 直接重複も近接もない離れたコマは △ に丸める。
    exam_busies = [
        Busy(datetime(2026, 11, 15, 9, 40, tzinfo=tz), datetime(2026, 11, 15, 12, 0, tzinfo=tz), "本体", "電力・管理"),
        Busy(datetime(2026, 11, 15, 13, 0, tzinfo=tz), datetime(2026, 11, 15, 14, 20, tzinfo=tz), "本体", "機械・制御"),
    ]
    v, _ = judge(date(2026, 11, 15), "evening", config, exam_busies, [], {date(2026, 11, 15)})
    assert v == "△", v
    # soft_day でない日の同条件なら ○(上限が掛からない)。
    v, _ = judge(date(2026, 11, 16), "evening", config, exam_busies, [], set())
    assert v == "○", v

    # 7) 終日 ignore_prefixes は無視される(候補判定に影響しない)。
    events = {"primary": [_ev_allday("2026-09-28", "2026-10-31", "【勉強期間】令和8年度 技術士第一次試験")]}
    busies_c, soft_markers_c, soft_days_c, validated_c = classify_events(events, config, "")
    assert busies_c == [] and soft_markers_c == [] and soft_days_c == set()

    # 8) 自分のマーカー(同一 source_url)は無視、別 URL のマーカーは △。
    url = "https://chouseisan.com/s?h=example"
    own_marker_events = {"primary": [_ev_timed("2026-10-18T18:00:00+09:00", "2026-10-18T21:00:00+09:00", "【調整中】テスト", description=f"候補日程一覧: {url}")]}
    b, sm, _, _ = classify_events(own_marker_events, config, url)
    assert b == [] and sm == []
    foreign_marker_events = {"primary": [_ev_timed("2026-10-18T18:00:00+09:00", "2026-10-18T21:00:00+09:00", "【調整中】別件", description="候補日程一覧: https://chouseisan.com/s?h=other")]}
    b, sm, _, _ = classify_events(foreign_marker_events, config, url)
    assert b == [] and len(sm) == 1
    v, _ = judge(date(2026, 10, 18), "evening", config, [], sm, set())
    assert v == "△", v

    # 9) transparent な時間指定予定は無視。
    transparent_events = {"primary": [_ev_timed("2026-10-18T18:00:00+09:00", "2026-10-18T21:00:00+09:00", "自由", transparency="transparent")]}
    b, _, _, _ = classify_events(transparent_events, config, "")
    assert b == []

    # 10) MCP コネクタが終日イベントの date を `YYYY-MM-DDT00:00:00Z` 形式で
    # 返す実測フォーマットを許容する。
    quirky_events = {"primary": [
        {"status": "confirmed", "summary": "【試験本番】テスト", "start": {"date": "2026-11-15T00:00:00Z"}, "end": {"date": "2026-11-16T00:00:00Z"}}
    ]}
    _, _, soft_days_q, _ = classify_events(quirky_events, config, "")
    assert soft_days_q == {date(2026, 11, 15)}, soft_days_q

    print("OK: slot-hit.py 自己検査 10 項目すべて通過")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--candidates", type=Path)
    parser.add_argument("--events-dir", type=Path)
    parser.add_argument("--today", type=str, default=None, help="YYYY-MM-DD(省略時はシステム今日)")
    parser.add_argument("--event-title", type=str, default="", help="マーカー件名に使うイベント名")
    parser.add_argument("--source-url", type=str, default="", help="候補日程一覧の URL(自分のマーカー識別・説明欄記載に使用)")
    parser.add_argument("--out-plan", type=Path, default=None, help="marker/answers 計画 JSON の出力先")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        run_selftest()
        return 0

    if not (args.config and args.candidates and args.events_dir):
        parser.error("--config / --candidates / --events-dir は --selftest でない限り必須です")

    today = date.fromisoformat(args.today) if args.today else date.today()
    try:
        output = run(
            args.config,
            args.candidates,
            args.events_dir,
            today,
            args.event_title,
            args.source_url,
            args.out_plan,
        )
    except SlotHitError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2

    print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
